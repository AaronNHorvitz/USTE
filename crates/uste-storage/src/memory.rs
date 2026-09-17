//! Deterministic volatile/durable filesystem model for publication and restart tests.

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use crate::{
    AdapterError, AdapterErrorKind, EntryName, FileMetadata, FileSystem, OwnershipFileSystem,
    RestartableFileSystem,
};

const ROOT_NODE: u64 = 1;
pub const DEFAULT_MEMORY_FILE_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryFile {
    node: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryDirectory {
    node: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Node {
    File(u64),
    Directory(u64),
}

#[derive(Clone, Debug)]
struct MemoryFileState {
    volatile: Vec<u8>,
    durable: Vec<u8>,
    exclusive_lock_generation: Rc<Cell<u64>>,
}

impl Default for MemoryFileState {
    fn default() -> Self {
        Self {
            volatile: Vec::new(),
            durable: Vec::new(),
            exclusive_lock_generation: Rc::new(Cell::new(0)),
        }
    }
}

/// Non-cloneable process-local ownership guard for the deterministic model.
#[derive(Debug)]
pub struct MemoryOwnershipGuard {
    lock_generation: Rc<Cell<u64>>,
    generation: u64,
}

impl Drop for MemoryOwnershipGuard {
    fn drop(&mut self) {
        if self.lock_generation.get() == self.generation {
            self.lock_generation.set(0);
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MemoryDirectoryState {
    volatile: BTreeMap<EntryName, Node>,
    durable: BTreeMap<EntryName, Node>,
}

/// Reference filesystem with separate file-data and directory-entry durability.
///
/// `restart` discards every unsynchronized byte and namespace mutation and invalidates all handles.
/// It models the accepted ordering contract; it is not a substitute for supported-filesystem tests.
#[derive(Debug)]
pub struct MemoryFileSystem {
    generation: u64,
    next_node: u64,
    file_limit: usize,
    files: BTreeMap<u64, MemoryFileState>,
    directories: BTreeMap<u64, MemoryDirectoryState>,
}

impl Default for MemoryFileSystem {
    fn default() -> Self {
        Self::new(DEFAULT_MEMORY_FILE_LIMIT)
    }
}

impl MemoryFileSystem {
    #[must_use]
    pub fn new(file_limit: usize) -> Self {
        let mut directories = BTreeMap::new();
        directories.insert(ROOT_NODE, MemoryDirectoryState::default());
        Self {
            generation: 1,
            next_node: ROOT_NODE + 1,
            file_limit,
            files: BTreeMap::new(),
            directories,
        }
    }

    /// Simulate abrupt process loss and reopen from only durable state.
    pub fn restart(&mut self) -> Result<(), AdapterError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        for file in self.files.values_mut() {
            file.volatile.clone_from(&file.durable);
            file.exclusive_lock_generation.set(0);
        }
        for directory in self.directories.values_mut() {
            directory.volatile.clone_from(&directory.durable);
        }
        Ok(())
    }

    fn allocate_node(&mut self) -> Result<u64, AdapterError> {
        let node = self.next_node;
        self.next_node = self
            .next_node
            .checked_add(1)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(node)
    }

    fn directory(&self, handle: &MemoryDirectory) -> Result<&MemoryDirectoryState, AdapterError> {
        if handle.generation != self.generation {
            return Err(AdapterErrorKind::StaleHandle.into());
        }
        self.directories
            .get(&handle.node)
            .ok_or_else(|| AdapterErrorKind::NotFound.into())
    }

    fn directory_mut(
        &mut self,
        handle: &MemoryDirectory,
    ) -> Result<&mut MemoryDirectoryState, AdapterError> {
        if handle.generation != self.generation {
            return Err(AdapterErrorKind::StaleHandle.into());
        }
        self.directories
            .get_mut(&handle.node)
            .ok_or_else(|| AdapterErrorKind::NotFound.into())
    }

    fn file(&self, handle: &MemoryFile) -> Result<&MemoryFileState, AdapterError> {
        if handle.generation != self.generation {
            return Err(AdapterErrorKind::StaleHandle.into());
        }
        self.files
            .get(&handle.node)
            .ok_or_else(|| AdapterErrorKind::NotFound.into())
    }

    fn file_mut(&mut self, handle: &MemoryFile) -> Result<&mut MemoryFileState, AdapterError> {
        if handle.generation != self.generation {
            return Err(AdapterErrorKind::StaleHandle.into());
        }
        self.files
            .get_mut(&handle.node)
            .ok_or_else(|| AdapterErrorKind::NotFound.into())
    }

    #[cfg(test)]
    pub(crate) fn test_child_names(
        &self,
        directory: &MemoryDirectory,
    ) -> Result<Vec<EntryName>, AdapterError> {
        Ok(self
            .directory(directory)?
            .volatile
            .keys()
            .cloned()
            .collect())
    }

    #[cfg(test)]
    pub(crate) fn test_mutate_file(
        &mut self,
        directory: &MemoryDirectory,
        name: &EntryName,
        offset: usize,
    ) -> Result<(), AdapterError> {
        let file = self.open_existing(directory, name)?;
        let file = self.file_mut(&file)?;
        let volatile = file
            .volatile
            .get_mut(offset)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::UnexpectedEof))?;
        *volatile ^= 0x80;
        let durable = file
            .durable
            .get_mut(offset)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::UnexpectedEof))?;
        *durable ^= 0x80;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_append_file(
        &mut self,
        directory: &MemoryDirectory,
        name: &EntryName,
        bytes: &[u8],
    ) -> Result<(), AdapterError> {
        let file = self.open_existing(directory, name)?;
        let file = self.file_mut(&file)?;
        file.volatile.extend_from_slice(bytes);
        file.durable.extend_from_slice(bytes);
        Ok(())
    }
}

impl FileSystem for MemoryFileSystem {
    type File = MemoryFile;
    type Directory = MemoryDirectory;

    fn root(&self) -> Self::Directory {
        MemoryDirectory {
            node: ROOT_NODE,
            generation: self.generation,
        }
    }

    fn create_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        if self.directory(parent)?.volatile.contains_key(name) {
            return Err(AdapterErrorKind::AlreadyExists.into());
        }
        let node = self.allocate_node()?;
        self.directories
            .insert(node, MemoryDirectoryState::default());
        self.directory_mut(parent)?
            .volatile
            .insert(name.clone(), Node::Directory(node));
        Ok(MemoryDirectory {
            node,
            generation: self.generation,
        })
    }

    fn open_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        match self.directory(parent)?.volatile.get(name) {
            Some(Node::Directory(node)) => Ok(MemoryDirectory {
                node: *node,
                generation: self.generation,
            }),
            _ => Err(AdapterErrorKind::NotFound.into()),
        }
    }

    fn create_new(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        if self.directory(directory)?.volatile.contains_key(name) {
            return Err(AdapterErrorKind::AlreadyExists.into());
        }
        let node = self.allocate_node()?;
        self.files.insert(node, MemoryFileState::default());
        self.directory_mut(directory)?
            .volatile
            .insert(name.clone(), Node::File(node));
        Ok(MemoryFile {
            node,
            generation: self.generation,
        })
    }

    fn open_existing(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        match self.directory(directory)?.volatile.get(name) {
            Some(Node::File(node)) => Ok(MemoryFile {
                node: *node,
                generation: self.generation,
            }),
            _ => Err(AdapterErrorKind::NotFound.into()),
        }
    }

    fn metadata(&mut self, file: &Self::File) -> Result<FileMetadata, AdapterError> {
        let len = u64::try_from(self.file(file)?.volatile.len())
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(FileMetadata { len })
    }

    fn read_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AdapterError> {
        let offset = usize::try_from(offset)
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        let bytes = &self.file(file)?.volatile;
        if offset >= bytes.len() {
            return Ok(0);
        }
        let count = output.len().min(bytes.len() - offset);
        output[..count].copy_from_slice(&bytes[offset..offset + count]);
        Ok(count)
    }

    fn write_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        input: &[u8],
    ) -> Result<usize, AdapterError> {
        let offset = usize::try_from(offset)
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        let end = offset
            .checked_add(input.len())
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        if end > self.file_limit {
            return Err(AdapterErrorKind::NoSpace.into());
        }
        let file = self.file_mut(file)?;
        if end > file.volatile.len() {
            file.volatile.resize(end, 0);
        }
        file.volatile[offset..end].copy_from_slice(input);
        Ok(input.len())
    }

    fn set_len(&mut self, file: &Self::File, len: u64) -> Result<(), AdapterError> {
        let len =
            usize::try_from(len).map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        if len > self.file_limit {
            return Err(AdapterErrorKind::NoSpace.into());
        }
        self.file_mut(file)?.volatile.resize(len, 0);
        Ok(())
    }

    fn sync_data(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        let file = self.file_mut(file)?;
        file.durable.clone_from(&file.volatile);
        Ok(())
    }

    fn sync_all(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        self.sync_data(file)
    }

    fn rename_no_replace(
        &mut self,
        source_directory: &Self::Directory,
        source: &EntryName,
        destination_directory: &Self::Directory,
        destination: &EntryName,
    ) -> Result<(), AdapterError> {
        let source_node = self
            .directory(source_directory)?
            .volatile
            .get(source)
            .copied()
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::NotFound))?;
        if self
            .directory(destination_directory)?
            .volatile
            .contains_key(destination)
        {
            return Err(AdapterErrorKind::AlreadyExists.into());
        }
        self.directory_mut(source_directory)?
            .volatile
            .remove(source);
        self.directory_mut(destination_directory)?
            .volatile
            .insert(destination.clone(), source_node);
        Ok(())
    }

    fn sync_directory(&mut self, directory: &Self::Directory) -> Result<(), AdapterError> {
        let directory = self.directory_mut(directory)?;
        directory.durable.clone_from(&directory.volatile);
        Ok(())
    }
}

impl OwnershipFileSystem for MemoryFileSystem {
    type OwnershipGuard = MemoryOwnershipGuard;

    fn try_lock_exclusive(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::OwnershipGuard, AdapterError> {
        let file = self.open_existing(directory, name)?;
        let lock_generation = Rc::clone(&self.file(&file)?.exclusive_lock_generation);
        if lock_generation.get() != 0 {
            return Err(AdapterErrorKind::OwnershipConflict.into());
        }
        lock_generation.set(self.generation);
        Ok(MemoryOwnershipGuard {
            lock_generation,
            generation: self.generation,
        })
    }
}

impl RestartableFileSystem for MemoryFileSystem {
    fn restart(&mut self) -> Result<(), AdapterError> {
        Self::restart(self)
    }
}
