//! Bounded adapter-call observation, not authenticated-index or physical-device accounting.
use uste_storage::{AdapterError, EntryName, FileMetadata, FileSystem, OwnershipFileSystem};

const OPERATIONS: [&str; 14] = [
    "create_directory",
    "open_directory",
    "create_new",
    "open_existing",
    "metadata",
    "read_at",
    "write_at",
    "set_len",
    "sync_data",
    "sync_all",
    "rename_no_replace",
    "remove_file",
    "sync_directory",
    "try_lock_exclusive",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::linux_runner) struct IoSnapshot {
    calls: [u64; 14],
    failures: [u64; 14],
    // Read requested/returned, write requested/returned. No payload or path is retained.
    bytes: [u64; 4],
    overflowed: bool,
}

impl IoSnapshot {
    fn add(value: &mut u64, amount: u64, overflowed: &mut bool) {
        match value.checked_add(amount) {
            Some(next) => *value = next,
            None => *overflowed = true,
        }
    }
    fn byte_count(&mut self, field: usize, bytes: usize) {
        match u64::try_from(bytes) {
            Ok(bytes) => Self::add(&mut self.bytes[field], bytes, &mut self.overflowed),
            Err(_) => self.overflowed = true,
        }
    }
    pub fn delta(self, before: Self) -> Result<Self, super::LinuxRunnerError> {
        let mut delta = Self::default();
        if self.overflowed || before.overflowed {
            return Err(super::error("USTE_BM01_IO_COUNTER"));
        }
        for (target, (new, old)) in delta
            .calls
            .iter_mut()
            .chain(&mut delta.failures)
            .chain(&mut delta.bytes)
            .zip(
                self.calls
                    .into_iter()
                    .chain(self.failures)
                    .chain(self.bytes)
                    .zip(
                        before
                            .calls
                            .into_iter()
                            .chain(before.failures)
                            .chain(before.bytes),
                    ),
            )
        {
            *target = new
                .checked_sub(old)
                .ok_or_else(|| super::error("USTE_BM01_IO_COUNTER"))?;
        }
        Ok(delta)
    }
    pub fn accumulate(&mut self, other: Self) -> Result<(), super::LinuxRunnerError> {
        if self.overflowed || other.overflowed {
            return Err(super::error("USTE_BM01_IO_COUNTER"));
        }
        let mut next = *self;
        for (target, amount) in next
            .calls
            .iter_mut()
            .chain(&mut next.failures)
            .chain(&mut next.bytes)
            .zip(
                other
                    .calls
                    .into_iter()
                    .chain(other.failures)
                    .chain(other.bytes),
            )
        {
            *target = target
                .checked_add(amount)
                .ok_or_else(|| super::error("USTE_BM01_IO_COUNTER"))?;
        }
        *self = next;
        Ok(())
    }
    pub fn json(self) -> Result<serde_json::Value, super::LinuxRunnerError> {
        if self.overflowed {
            return Err(super::error("USTE_BM01_IO_COUNTER"));
        }
        let mut operations = serde_json::Map::new();
        for (index, name) in OPERATIONS.into_iter().enumerate() {
            operations.insert(
                name.into(),
                serde_json::json!({"calls": self.calls[index], "failures": self.failures[index]}),
            );
        }
        Ok(serde_json::json!({
            "measurement_scope": "filesystem-adapter-calls",
            "physical_device_io": false, "complete_authenticated_index_io": false,
            "operations": operations, "read_requested_bytes": self.bytes[0],
            "read_returned_bytes": self.bytes[1], "write_requested_bytes": self.bytes[2],
            "write_returned_bytes": self.bytes[3],
        }))
    }
}

pub(in crate::linux_runner) struct ObservedFileSystem<F> {
    inner: F,
    counters: IoSnapshot,
}

impl<F> ObservedFileSystem<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            counters: IoSnapshot::default(),
        }
    }
    pub fn snapshot(&self) -> Result<IoSnapshot, super::LinuxRunnerError> {
        if self.counters.overflowed {
            Err(super::error("USTE_BM01_IO_COUNTER"))
        } else {
            Ok(self.counters)
        }
    }
    fn finish<T>(
        &mut self,
        operation: usize,
        result: Result<T, AdapterError>,
    ) -> Result<T, AdapterError> {
        IoSnapshot::add(
            &mut self.counters.calls[operation],
            1,
            &mut self.counters.overflowed,
        );
        if result.is_err() {
            IoSnapshot::add(
                &mut self.counters.failures[operation],
                1,
                &mut self.counters.overflowed,
            );
        }
        // Observation overflow invalidates reports, never substitutes an adapter result.
        result
    }
}

impl<F: FileSystem> FileSystem for ObservedFileSystem<F> {
    type File = F::File;
    type Directory = F::Directory;
    fn root(&self) -> Self::Directory {
        self.inner.root()
    }
    fn create_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        let result = self.inner.create_directory(parent, name);
        self.finish(0, result)
    }
    fn open_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        let result = self.inner.open_directory(parent, name);
        self.finish(1, result)
    }
    fn create_new(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        let result = self.inner.create_new(directory, name);
        self.finish(2, result)
    }
    fn open_existing(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        let result = self.inner.open_existing(directory, name);
        self.finish(3, result)
    }
    fn metadata(&mut self, file: &Self::File) -> Result<FileMetadata, AdapterError> {
        let result = self.inner.metadata(file);
        self.finish(4, result)
    }
    fn read_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AdapterError> {
        self.counters.byte_count(0, output.len());
        let result = self.inner.read_at(file, offset, output);
        if let Ok(count) = result {
            self.counters.overflowed |= count > output.len();
            self.counters.byte_count(1, count);
        }
        self.finish(5, result)
    }
    fn write_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        input: &[u8],
    ) -> Result<usize, AdapterError> {
        self.counters.byte_count(2, input.len());
        let result = self.inner.write_at(file, offset, input);
        if let Ok(count) = result {
            self.counters.overflowed |= count > input.len();
            self.counters.byte_count(3, count);
        }
        self.finish(6, result)
    }
    fn set_len(&mut self, file: &Self::File, len: u64) -> Result<(), AdapterError> {
        let result = self.inner.set_len(file, len);
        self.finish(7, result)
    }
    fn sync_data(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        let result = self.inner.sync_data(file);
        self.finish(8, result)
    }
    fn sync_all(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        let result = self.inner.sync_all(file);
        self.finish(9, result)
    }
    fn rename_no_replace(
        &mut self,
        source_directory: &Self::Directory,
        source: &EntryName,
        destination_directory: &Self::Directory,
        destination: &EntryName,
    ) -> Result<(), AdapterError> {
        let result = self.inner.rename_no_replace(
            source_directory,
            source,
            destination_directory,
            destination,
        );
        self.finish(10, result)
    }
    fn remove_file(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<(), AdapterError> {
        let result = self.inner.remove_file(directory, name);
        self.finish(11, result)
    }
    fn sync_directory(&mut self, directory: &Self::Directory) -> Result<(), AdapterError> {
        let result = self.inner.sync_directory(directory);
        self.finish(12, result)
    }
}
impl<F: OwnershipFileSystem> OwnershipFileSystem for ObservedFileSystem<F> {
    type OwnershipGuard = F::OwnershipGuard;
    fn try_lock_exclusive(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::OwnershipGuard, AdapterError> {
        let result = self.inner.try_lock_exclusive(directory, name);
        self.finish(13, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_storage::{
        AdapterErrorKind,
        fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation},
        memory::MemoryFileSystem,
    };

    #[test]
    fn observer_forwards_every_capability_and_counts_short_reads_eof_and_failures() {
        let mut fs = ObservedFileSystem::new(MemoryFileSystem::default());
        let root = fs.root();
        let name = EntryName::new("synthetic-directory").unwrap();
        let directory = fs.create_directory(&root, &name).unwrap();
        fs.open_directory(&root, &name).unwrap();
        let name = EntryName::new("synthetic-file").unwrap();
        let file = fs.create_new(&directory, &name).unwrap();
        assert_eq!(
            fs.create_new(&directory, &name).unwrap_err().kind(),
            AdapterErrorKind::AlreadyExists
        );
        fs.open_existing(&directory, &name).unwrap();
        assert_eq!(fs.write_at(&file, 0, b"abcd").unwrap(), 4);
        let before = fs.snapshot().unwrap();
        let mut output = [0; 8];
        assert_eq!(fs.read_at(&file, 0, &mut output).unwrap(), 4);
        assert_eq!(&output[..4], b"abcd");
        assert_eq!(fs.read_at(&file, 4, &mut output).unwrap(), 0);
        let delta = fs.snapshot().unwrap().delta(before).unwrap();
        assert_eq!(delta.calls[5], 2);
        assert_eq!(delta.bytes, [16, 4, 0, 0]);
        fs.set_len(&file, 2).unwrap();
        assert_eq!(fs.metadata(&file).unwrap().len, 2);
        fs.sync_data(&file).unwrap();
        fs.sync_all(&file).unwrap();
        let renamed = EntryName::new("renamed-file").unwrap();
        fs.rename_no_replace(&directory, &name, &directory, &renamed)
            .unwrap();
        fs.remove_file(&directory, &renamed).unwrap();
        fs.sync_directory(&directory).unwrap();
        let lock = EntryName::new("LOCK").unwrap();
        fs.create_new(&directory, &lock).unwrap();
        let guard = fs.try_lock_exclusive(&directory, &lock).unwrap();
        assert!(fs.try_lock_exclusive(&directory, &lock).is_err());
        drop(guard);
        drop(fs.try_lock_exclusive(&directory, &lock).unwrap());
        let report = fs.snapshot().unwrap();
        assert_eq!(report.calls, [1, 1, 3, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 3]);
        assert_eq!(report.failures[2], 1);
        assert_eq!(report.failures[13], 1);
        let json = report.json().unwrap();
        assert_eq!(json["measurement_scope"], "filesystem-adapter-calls");
        assert_eq!(json["complete_authenticated_index_io"], false);
        assert_eq!(json["physical_device_io"], false);
        assert!(!json.to_string().contains("synthetic"));
    }

    #[test]
    fn observer_does_not_retry_errors_or_change_partial_io_results() {
        let plan = FaultPlan::new([
            FaultPoint {
                operation: Operation::WriteAt,
                occurrence: 1,
                action: FaultAction::ShortWrite { maximum: 1 },
            },
            FaultPoint {
                operation: Operation::ReadAt,
                occurrence: 1,
                action: FaultAction::ShortRead { maximum: 1 },
            },
            FaultPoint {
                operation: Operation::ReadAt,
                occurrence: 2,
                action: FaultAction::Error(AdapterErrorKind::Interrupted),
            },
            FaultPoint {
                operation: Operation::ReadAt,
                occurrence: 3,
                action: FaultAction::OverReport,
            },
        ])
        .unwrap();
        let mut fs =
            ObservedFileSystem::new(FaultFileSystem::new(MemoryFileSystem::default(), plan));
        let file = fs
            .create_new(&fs.root(), &EntryName::new("file").unwrap())
            .unwrap();
        assert_eq!(fs.write_at(&file, 0, b"abcd").unwrap(), 1);
        let mut bytes = [0; 4];
        assert_eq!(fs.read_at(&file, 0, &mut bytes).unwrap(), 1);
        assert_eq!(
            fs.read_at(&file, 0, &mut bytes).unwrap_err().kind(),
            AdapterErrorKind::Interrupted
        );
        let snapshot = fs.snapshot().unwrap();
        assert_eq!(snapshot.calls[5], 2);
        assert_eq!(snapshot.failures[5], 1);
        assert_eq!(snapshot.bytes, [8, 1, 4, 1]);
        assert_eq!(fs.read_at(&file, 0, &mut bytes).unwrap(), 5);
        assert!(fs.snapshot().is_err());
        assert_eq!(fs.inner.operation_count(Operation::ReadAt), 3);
    }

    #[test]
    fn observation_overflow_invalidates_reports_without_changing_durable_operations() {
        let mut fs = ObservedFileSystem::new(MemoryFileSystem::default());
        let file = fs
            .create_new(&fs.root(), &EntryName::new("file").unwrap())
            .unwrap();
        fs.counters.calls[6] = u64::MAX;
        assert_eq!(fs.write_at(&file, 0, b"ab").unwrap(), 2);
        fs.sync_all(&file).unwrap();
        assert_eq!(fs.inner.metadata(&file).unwrap().len, 2);
        assert!(fs.snapshot().is_err());
        assert!(fs.counters.json().is_err());
        let mut full = IoSnapshot::default();
        full.calls[0] = u64::MAX;
        let mut one = IoSnapshot::default();
        one.calls[0] = 1;
        let previous = full;
        assert!(full.accumulate(one).is_err());
        assert_eq!(full, previous);
        assert!(one.delta(full).is_err());
        assert_eq!(full.delta(full).unwrap(), IoSnapshot::default());
    }
}
