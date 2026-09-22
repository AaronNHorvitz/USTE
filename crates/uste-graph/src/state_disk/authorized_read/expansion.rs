use super::*;
use crate::query::record_references_are_authorized;
use crate::{
    AdjacencyDirection, AssertionStatus, GraphError, GraphNeighbor, MAX_TRAVERSAL_RESULTS,
    MAX_TRAVERSAL_VISITS,
};
use uste_storage::{IndexScanEntry, IndexScanLimits};
mod semantics;
pub(crate) use semantics::{ExpansionRead, read_with};

#[derive(Clone, Copy)]
pub(crate) struct ExpansionScanEntry {
    pub(crate) id: [u8; 16],
    pub(crate) neighbor: Option<[u8; 16]>,
}

fn compact_scan_entry(
    family: u8,
    prefix: RecordRef,
    entry: &IndexScanEntry,
) -> Result<ExpansionScanEntry, GraphDiskError> {
    if entry.key.len() != 32 || entry.key[..16] != *prefix.record().as_bytes() {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let id = entry.key[16..]
        .try_into()
        .map_err(|_| GraphDiskError::IndexCorrupt)?;
    let neighbor = match family {
        FAMILY_PROVENANCE if entry.value.is_empty() => None,
        FAMILY_OUTGOING | FAMILY_INCOMING if entry.value.len() == 16 => Some(
            entry
                .value
                .as_slice()
                .try_into()
                .map_err(|_| GraphDiskError::IndexCorrupt)?,
        ),
        _ => return Err(GraphDiskError::IndexCorrupt),
    };
    Ok(ExpansionScanEntry { id, neighbor })
}

/// Aggregate per-query admission across all secondary scans and record lookups.
#[derive(Clone, Copy, Debug)]
pub struct GraphDiskExpansionLimits {
    pages: u64,
    entries: usize,
    bytes: usize,
    lookups: usize,
}

impl GraphDiskExpansionLimits {
    pub fn new(
        pages: u64,
        entries: usize,
        bytes: usize,
        lookups: usize,
    ) -> Result<Self, StorageError> {
        IndexScanLimits::new(pages, entries, bytes)?;
        if entries > MAX_TRAVERSAL_VISITS || lookups > MAX_TRAVERSAL_VISITS * 2 {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            pages,
            entries,
            bytes,
            lookups,
        })
    }

    fn account(&mut self, stats: &IndexReadStats, entries: usize) -> Result<(), StorageError> {
        self.pages = self
            .pages
            .checked_sub(
                stats
                    .pages_read
                    .checked_add(stats.cache_hits)
                    .ok_or(StorageError::ResourceLimit)?,
            )
            .ok_or(StorageError::ResourceLimit)?;
        self.bytes = self
            .bytes
            .checked_sub(
                usize::try_from(stats.result_bytes).map_err(|_| StorageError::ResourceLimit)?,
            )
            .ok_or(StorageError::ResourceLimit)?;
        self.entries = self
            .entries
            .checked_sub(entries)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }
}

struct Reader<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    coordinator: &'a DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &'a mut F,
    root: &'a RecoveredIndexRoot,
    cache: &'a mut PageCache,
    budget: GraphDiskExpansionLimits,
}

impl<F, W, E, I> ExpansionRead for Reader<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn scan(
        &mut self,
        family: u8,
        id: RecordRef,
    ) -> Result<Option<Vec<ExpansionScanEntry>>, GraphDiskError> {
        if !has_family(self.root, family) {
            return Ok(None);
        }
        let scan = self.coordinator.index_scan_prefix_bounded(
            self.filesystem,
            self.root,
            family,
            id.record().as_bytes(),
            IndexScanLimits::new(self.budget.pages, self.budget.entries, self.budget.bytes)?,
            self.cache,
        )?;
        self.budget.account(&scan.stats, scan.entries.len())?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(scan.entries.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        for entry in &scan.entries {
            entries.push(compact_scan_entry(family, id, entry)?);
        }
        Ok(Some(entries))
    }

    fn record(&mut self, id: RecordRef) -> Result<Record, GraphDiskError> {
        self.budget.lookups = self
            .budget
            .lookups
            .checked_sub(1)
            .ok_or(StorageError::ResourceLimit)?;
        if !has_family(self.root, FAMILY_CURRENT_RECORD) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        let (value, stats) = self.coordinator.index_get_bounded(
            self.filesystem,
            self.root,
            FAMILY_CURRENT_RECORD,
            id.record().as_bytes(),
            IndexGetLimits::new(
                self.budget.pages,
                self.budget.bytes.min(MAX_INDEX_VALUE_BYTES),
            )?,
            self.cache,
        )?;
        self.budget.account(&stats, 0)?;
        let record = decode_stored_record(&value.ok_or(GraphDiskError::IndexCorrupt)?)?;
        if record.id() != id || record.modified_revision() > self.root.revision() {
            return Err(GraphDiskError::IndexCorrupt);
        }
        Ok(record)
    }

    fn reference(&self, bytes: &[u8]) -> Result<RecordRef, GraphDiskError> {
        record_key(self.root.scope(), bytes).map_err(GraphDiskError::Storage)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn read<F, W, E, I>(
    coordinator: &DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    root: &RecoveredIndexRoot,
    request: &GraphReadRequest,
    budget: GraphDiskExpansionLimits,
    cache: &mut PageCache,
    authorize: &mut dyn FnMut(Action, Target) -> bool,
) -> Result<GraphReadOutput, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let mut reader = Reader {
        coordinator,
        filesystem,
        root,
        cache,
        budget,
    };
    read_with(&mut reader, request, authorize)
}
