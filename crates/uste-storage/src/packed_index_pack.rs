//! Bounded immutable encrypted pack I/O. No root publication or typed payload admission.
use crate::packed_index_page::{
    ENCODED_PAGE_BYTES, MAX_PAGES, MAX_RECORD_PAYLOAD, MAX_SLOTS, PackedPage, PackedPageBuilder,
    PackedPageContext, PackedRecordKind,
};
use crate::{EntryName, FileSystem, journal::StorageError, read_exact_at, write_all_at};
use uste_crypto::{EntropySource, KeyVault};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub struct PackWriteLimits {
    pub maximum_pages: u64,
    pub maximum_records: u64,
    pub maximum_payload_bytes: u64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedRecordAddress {
    context: PackedPageContext,
    slot: u16,
}
impl PackedRecordAddress {
    pub fn context(self) -> PackedPageContext {
        self.context
    }
    pub fn slot(self) -> u16 {
        self.slot
    }
}

/// Successful durable transport only; not canonical-state or authorization admission.
#[derive(Clone, Copy)]
pub struct ImmutablePack {
    context: PackedPageContext,
    pages: u64,
    records: u64,
    payload_bytes: u64,
}
impl ImmutablePack {
    pub fn context(self) -> PackedPageContext {
        self.context
    }
    pub fn pages(self) -> u64 {
        self.pages
    }
    pub fn records(self) -> u64 {
        self.records
    }
    pub fn payload_bytes(self) -> u64 {
        self.payload_bytes
    }
}

pub struct ImmutablePackWriter<F: FileSystem> {
    directory: F::Directory,
    file: F::File,
    context: PackedPageContext,
    page: PackedPageBuilder,
    limits: PackWriteLimits,
    page_index: u64,
    page_records: u16,
    page_payload: usize,
    records: u64,
    payload_bytes: u64,
    failed: bool,
}

impl<F: FileSystem> ImmutablePackWriter<F> {
    /// `context.object` and `context.page` must be zero; a fresh identity is generated here.
    pub fn create<I: EntropySource>(
        filesystem: &mut F,
        directory: &F::Directory,
        mut context: PackedPageContext,
        limits: PackWriteLimits,
        entropy: &mut I,
    ) -> Result<Self, StorageError> {
        if context.object != [0; 16] || context.page != 0 || context.family == 0 {
            return Err(StorageError::InvalidState);
        }
        if limits.maximum_pages == 0
            || limits.maximum_pages > MAX_PAGES
            || limits.maximum_records == 0
            || limits.maximum_records > MAX_PAGES * MAX_SLOTS as u64
            || limits.maximum_payload_bytes == 0
            || limits.maximum_payload_bytes > MAX_PAGES * MAX_RECORD_PAYLOAD as u64
        {
            return Err(StorageError::ResourceLimit);
        }
        entropy
            .fill(&mut context.object)
            .map_err(|_| StorageError::Crypto(uste_crypto::CryptoError::RetryableUnavailable))?;
        if context.object == [0; 16] {
            return Err(StorageError::IntegrityFailure);
        }
        let page = PackedPageBuilder::new(context)?;
        let file = filesystem.create_new(directory, &pack_name(context.object)?)?;
        Ok(Self {
            directory: directory.clone(),
            file,
            context,
            page,
            limits,
            page_index: 0,
            page_records: 0,
            page_payload: 0,
            records: 0,
            payload_bytes: 0,
            failed: false,
        })
    }

    pub fn append<W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
        kind: PackedRecordKind,
        payload: &[u8],
    ) -> Result<PackedRecordAddress, StorageError> {
        if self.failed {
            return Err(StorageError::NeedsRecovery);
        }
        let result = self.append_inner(filesystem, vault, kind, payload);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn append_inner<W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
        kind: PackedRecordKind,
        payload: &[u8],
    ) -> Result<PackedRecordAddress, StorageError> {
        if payload.is_empty() {
            return Err(StorageError::InvalidState);
        }
        if payload.len() > MAX_RECORD_PAYLOAD
            || self.records >= self.limits.maximum_records
            || payload.len() as u64 > self.limits.maximum_payload_bytes - self.payload_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        // The fixed framing area leaves MAX_RECORD_PAYLOAD + 1 bytes for kinds and payloads.
        let rollover = self.page_records == MAX_SLOTS
            || self.page_payload + self.page_records as usize + payload.len() + 1
                > MAX_RECORD_PAYLOAD + 1;
        if rollover {
            if self.page_index + 1 >= self.limits.maximum_pages {
                return Err(StorageError::ResourceLimit);
            }
            self.flush_page(filesystem, vault)?;
            self.page_index += 1;
            self.page = PackedPageBuilder::new(PackedPageContext {
                page: self.page_index,
                ..self.context
            })?;
            self.page_records = 0;
            self.page_payload = 0;
        }
        let slot = self.page.push(kind, payload)?;
        self.page_records += 1;
        self.page_payload += payload.len();
        self.records += 1;
        self.payload_bytes += payload.len() as u64;
        Ok(PackedRecordAddress {
            context: PackedPageContext {
                page: self.page_index,
                ..self.context
            },
            slot,
        })
    }

    fn flush_page<W, E: EntropySource>(
        &self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
    ) -> Result<(), StorageError> {
        let encoded = self.page.seal(vault)?;
        write_all_at(
            filesystem,
            &self.file,
            self.page_index * ENCODED_PAGE_BYTES as u64,
            &encoded,
        )?;
        Ok(())
    }

    pub fn finish<W, E: EntropySource>(
        self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
    ) -> Result<ImmutablePack, StorageError> {
        if self.failed {
            return Err(StorageError::NeedsRecovery);
        }
        if self.records == 0 {
            return Err(StorageError::InvalidState);
        }
        self.flush_page(filesystem, vault)?;
        let pages = self.page_index + 1;
        filesystem.set_len(&self.file, pages * ENCODED_PAGE_BYTES as u64)?;
        filesystem.sync_all(&self.file)?;
        filesystem.sync_directory(&self.directory)?;
        Ok(ImmutablePack {
            context: self.context,
            pages,
            records: self.records,
            payload_bytes: self.payload_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackReadReport {
    pub pages: u64,
    pub encoded_bytes: u64,
}

/// One-page raw maintenance read, requiring a descriptor and an exact bound address.
pub fn read_record_page<F: FileSystem, W, E: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    pack: ImmutablePack,
    address: PackedRecordAddress,
    maximum_encoded_bytes: u64,
) -> Result<(PackedPage, PackReadReport), StorageError> {
    if address.context.page >= pack.pages
        || address.slot >= MAX_SLOTS
        || address.context
            != (PackedPageContext {
                page: address.context.page,
                ..pack.context
            })
    {
        return Err(StorageError::InvalidState);
    }
    if maximum_encoded_bytes < ENCODED_PAGE_BYTES as u64 {
        return Err(StorageError::ResourceLimit);
    }
    let file = filesystem.open_existing(directory, &pack_name(pack.context.object)?)?;
    if filesystem.metadata(&file)?.len != pack.pages * ENCODED_PAGE_BYTES as u64 {
        return Err(StorageError::IntegrityFailure);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(ENCODED_PAGE_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    bytes.resize(ENCODED_PAGE_BYTES, 0);
    read_exact_at(
        filesystem,
        &file,
        address.context.page * ENCODED_PAGE_BYTES as u64,
        &mut bytes,
    )?;
    let page = PackedPage::open(vault, address.context, &bytes)?;
    if page.record(address.slot).is_none() {
        return Err(StorageError::IntegrityFailure);
    }
    Ok((
        page,
        PackReadReport {
            pages: 1,
            encoded_bytes: ENCODED_PAGE_BYTES as u64,
        },
    ))
}

fn pack_name(object: [u8; 16]) -> Result<EntryName, StorageError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(37);
    name.push_str("pack-");
    for byte in object {
        name.push(HEX[(byte >> 4) as usize] as char);
        name.push(HEX[(byte & 15) as usize] as char);
    }
    EntryName::new(name).map_err(|_| StorageError::InvalidState)
}
