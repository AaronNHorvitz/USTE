//! Deterministic per-operation fault scripts and scripted clock/random capabilities.

use std::collections::{BTreeMap, VecDeque};

use crate::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, FileMetadata, FileSystem,
    OwnershipFileSystem, RandomSource, RestartableFileSystem,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Operation {
    CreateDirectory,
    OpenDirectory,
    CreateNew,
    OpenExisting,
    Metadata,
    ReadAt,
    WriteAt,
    SetLen,
    SyncData,
    SyncAll,
    RenameNoReplace,
    RemoveFile,
    SyncDirectory,
    TryLockExclusive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultAction {
    Error(AdapterErrorKind),
    ShortWrite { maximum: usize },
    ShortRead { maximum: usize },
    ZeroProgress,
    OverReport,
    CrashBefore,
    CrashAfter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaultPoint {
    pub operation: Operation,
    /// One-based occurrence count for this operation.
    pub occurrence: u64,
    pub action: FaultAction,
}

#[derive(Clone, Debug, Default)]
pub struct FaultPlan {
    points: BTreeMap<(Operation, u64), FaultAction>,
}

impl FaultPlan {
    pub fn new(points: impl IntoIterator<Item = FaultPoint>) -> Result<Self, AdapterError> {
        let mut plan = Self::default();
        for point in points {
            let invalid_operation = match point.action {
                FaultAction::ShortWrite { .. } => point.operation != Operation::WriteAt,
                FaultAction::ShortRead { .. } => point.operation != Operation::ReadAt,
                FaultAction::ZeroProgress | FaultAction::OverReport => {
                    !matches!(point.operation, Operation::ReadAt | Operation::WriteAt)
                }
                FaultAction::Error(_) | FaultAction::CrashBefore | FaultAction::CrashAfter => false,
            };
            if point.occurrence == 0
                || invalid_operation
                || matches!(
                    point.action,
                    FaultAction::ShortWrite { maximum: 0 } | FaultAction::ShortRead { maximum: 0 }
                )
                || matches!(
                    point.action,
                    FaultAction::Error(AdapterErrorKind::InjectedCrash)
                )
                || plan
                    .points
                    .insert((point.operation, point.occurrence), point.action)
                    .is_some()
            {
                return Err(AdapterErrorKind::AdapterContract.into());
            }
        }
        Ok(plan)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Decorator that injects one-shot deterministic faults before or after exact operation calls.
#[derive(Debug)]
pub struct FaultFileSystem<F> {
    inner: F,
    plan: FaultPlan,
    occurrences: BTreeMap<Operation, u64>,
    crashed: bool,
}

impl<F> FaultFileSystem<F> {
    #[must_use]
    pub fn new(inner: F, plan: FaultPlan) -> Self {
        Self {
            inner,
            plan,
            occurrences: BTreeMap::new(),
            crashed: false,
        }
    }

    #[must_use]
    pub const fn inner(&self) -> &F {
        &self.inner
    }

    #[must_use]
    pub const fn is_crashed(&self) -> bool {
        self.crashed
    }

    #[must_use]
    pub fn pending_faults(&self) -> usize {
        self.plan.points.len()
    }

    /// Calls observed since creation or the last arm, including an injected failing call.
    #[must_use]
    pub fn operation_count(&self, operation: Operation) -> u64 {
        self.occurrences.get(&operation).copied().unwrap_or(0)
    }

    /// Install a fresh one-shot plan after setup and restart operation counters at zero.
    ///
    /// A pending plan or crashed adapter must be consumed/restarted first so tests cannot silently
    /// discard an expected boundary.
    pub fn arm(&mut self, plan: FaultPlan) -> Result<(), AdapterError> {
        if self.crashed || !self.plan.is_empty() {
            return Err(AdapterErrorKind::AdapterContract.into());
        }
        self.plan = plan;
        self.occurrences.clear();
        Ok(())
    }

    fn next_action(&mut self, operation: Operation) -> Result<Option<FaultAction>, AdapterError> {
        if self.crashed {
            return Err(AdapterErrorKind::InjectedCrash.into());
        }
        let occurrence = self.occurrences.entry(operation).or_default();
        *occurrence = occurrence
            .checked_add(1)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(self.plan.points.remove(&(operation, *occurrence)))
    }

    fn ordinary_action(
        &mut self,
        operation: Operation,
    ) -> Result<(bool, Option<AdapterError>), AdapterError> {
        match self.next_action(operation)? {
            None => Ok((false, None)),
            Some(FaultAction::Error(kind)) => Ok((false, Some(kind.into()))),
            Some(FaultAction::CrashBefore) => {
                self.crashed = true;
                Ok((false, Some(AdapterErrorKind::InjectedCrash.into())))
            }
            Some(FaultAction::CrashAfter) => Ok((true, None)),
            Some(
                FaultAction::ShortWrite { .. }
                | FaultAction::ShortRead { .. }
                | FaultAction::ZeroProgress
                | FaultAction::OverReport,
            ) => Err(AdapterErrorKind::AdapterContract.into()),
        }
    }

    fn finish<T>(
        &mut self,
        result: Result<T, AdapterError>,
        crash_after: bool,
    ) -> Result<T, AdapterError> {
        if crash_after && result.is_ok() {
            self.crashed = true;
            Err(AdapterErrorKind::InjectedCrash.into())
        } else {
            result
        }
    }
}

impl<F: RestartableFileSystem> FaultFileSystem<F> {
    /// Restart the owned adapter and clear sticky crash state only after restart succeeds.
    pub fn restart(&mut self) -> Result<(), AdapterError> {
        self.inner.restart()?;
        self.crashed = false;
        Ok(())
    }
}

impl<F: FileSystem> FileSystem for FaultFileSystem<F> {
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
        let (crash_after, error) = self.ordinary_action(Operation::CreateDirectory)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.create_directory(parent, name);
        self.finish(result, crash_after)
    }

    fn open_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::OpenDirectory)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.open_directory(parent, name);
        self.finish(result, crash_after)
    }

    fn create_new(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::CreateNew)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.create_new(directory, name);
        self.finish(result, crash_after)
    }

    fn open_existing(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::OpenExisting)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.open_existing(directory, name);
        self.finish(result, crash_after)
    }

    fn metadata(&mut self, file: &Self::File) -> Result<FileMetadata, AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::Metadata)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.metadata(file);
        self.finish(result, crash_after)
    }

    fn read_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AdapterError> {
        let action = self.next_action(Operation::ReadAt)?;
        let (output, crash_after) = match action {
            None => (output, false),
            Some(FaultAction::Error(kind)) => return Err(kind.into()),
            Some(FaultAction::ShortRead { maximum }) => {
                let length = maximum.min(output.len());
                (&mut output[..length], false)
            }
            Some(FaultAction::ZeroProgress) => return Ok(0),
            Some(FaultAction::OverReport) => return Ok(output.len().saturating_add(1)),
            Some(FaultAction::CrashBefore) => {
                self.crashed = true;
                return Err(AdapterErrorKind::InjectedCrash.into());
            }
            Some(FaultAction::CrashAfter) => (output, true),
            Some(FaultAction::ShortWrite { .. }) => {
                return Err(AdapterErrorKind::AdapterContract.into());
            }
        };
        let result = self.inner.read_at(file, offset, output);
        self.finish(result, crash_after)
    }

    fn write_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        input: &[u8],
    ) -> Result<usize, AdapterError> {
        let action = self.next_action(Operation::WriteAt)?;
        let (input, crash_after) = match action {
            None => (input, false),
            Some(FaultAction::Error(kind)) => return Err(kind.into()),
            Some(FaultAction::ShortWrite { maximum }) => {
                (&input[..maximum.min(input.len())], false)
            }
            Some(FaultAction::ZeroProgress) => return Ok(0),
            Some(FaultAction::OverReport) => return Ok(input.len().saturating_add(1)),
            Some(FaultAction::CrashBefore) => {
                self.crashed = true;
                return Err(AdapterErrorKind::InjectedCrash.into());
            }
            Some(FaultAction::CrashAfter) => (input, true),
            Some(FaultAction::ShortRead { .. }) => {
                return Err(AdapterErrorKind::AdapterContract.into());
            }
        };
        let result = self.inner.write_at(file, offset, input);
        self.finish(result, crash_after)
    }

    fn set_len(&mut self, file: &Self::File, len: u64) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::SetLen)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.set_len(file, len);
        self.finish(result, crash_after)
    }

    fn sync_data(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::SyncData)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.sync_data(file);
        self.finish(result, crash_after)
    }

    fn sync_all(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::SyncAll)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.sync_all(file);
        self.finish(result, crash_after)
    }

    fn rename_no_replace(
        &mut self,
        source_directory: &Self::Directory,
        source: &EntryName,
        destination_directory: &Self::Directory,
        destination: &EntryName,
    ) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::RenameNoReplace)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.rename_no_replace(
            source_directory,
            source,
            destination_directory,
            destination,
        );
        self.finish(result, crash_after)
    }

    fn remove_file(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::RemoveFile)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.remove_file(directory, name);
        self.finish(result, crash_after)
    }

    fn sync_directory(&mut self, directory: &Self::Directory) -> Result<(), AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::SyncDirectory)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.sync_directory(directory);
        self.finish(result, crash_after)
    }
}

impl<F: OwnershipFileSystem> OwnershipFileSystem for FaultFileSystem<F> {
    type OwnershipGuard = F::OwnershipGuard;

    fn try_lock_exclusive(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::OwnershipGuard, AdapterError> {
        let (crash_after, error) = self.ordinary_action(Operation::TryLockExclusive)?;
        if let Some(error) = error {
            return Err(error);
        }
        let result = self.inner.try_lock_exclusive(directory, name);
        self.finish(result, crash_after)
    }
}

/// Deterministic wall/monotonic observation sequence. Wall rollback is preserved verbatim.
#[derive(Clone, Debug)]
pub struct ScriptedClock {
    observations: VecDeque<Result<ClockObservation, AdapterErrorKind>>,
}

impl ScriptedClock {
    #[must_use]
    pub fn new(
        observations: impl IntoIterator<Item = Result<ClockObservation, AdapterErrorKind>>,
    ) -> Self {
        Self {
            observations: observations.into_iter().collect(),
        }
    }
}

impl Clock for ScriptedClock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError> {
        self.observations
            .pop_front()
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ScriptExhausted))?
            .map_err(Into::into)
    }
}

/// Deterministic exact-length random outputs and failures for adapter tests.
#[derive(Clone, Debug)]
pub struct ScriptedRandom {
    outputs: VecDeque<Result<Vec<u8>, AdapterErrorKind>>,
}

impl ScriptedRandom {
    #[must_use]
    pub fn new(outputs: impl IntoIterator<Item = Result<Vec<u8>, AdapterErrorKind>>) -> Self {
        Self {
            outputs: outputs.into_iter().collect(),
        }
    }
}

impl RandomSource for ScriptedRandom {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), AdapterError> {
        let bytes = self
            .outputs
            .pop_front()
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ScriptExhausted))?
            .map_err(AdapterError::from)?;
        if bytes.len() != output.len() {
            return Err(AdapterErrorKind::AdapterContract.into());
        }
        output.copy_from_slice(&bytes);
        Ok(())
    }
}
