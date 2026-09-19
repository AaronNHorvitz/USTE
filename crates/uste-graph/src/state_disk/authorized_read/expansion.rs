use super::*;
use crate::query::record_references_are_authorized;
use crate::{
    AdjacencyDirection, AssertionStatus, GraphError, GraphNeighbor, MAX_TRAVERSAL_RESULTS,
    MAX_TRAVERSAL_VISITS,
};
use uste_storage::{IndexScan, IndexScanLimits};

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

impl<F, W, E, I> Reader<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn scan(&mut self, family: u8, id: RecordRef) -> Result<Option<IndexScan>, GraphDiskError> {
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
        Ok(Some(scan))
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
    match request {
        GraphReadRequest::Adjacent {
            entity,
            direction,
            maximum,
        } => {
            check_maximum(*maximum)?;
            let mut candidates = BTreeMap::<RecordRef, (RecordRef, u8)>::new();
            for (family, bit) in match direction {
                AdjacencyDirection::Outgoing => &[(FAMILY_OUTGOING, 1)][..],
                AdjacencyDirection::Incoming => &[(FAMILY_INCOMING, 2)][..],
                AdjacencyDirection::Either => &[(FAMILY_OUTGOING, 1), (FAMILY_INCOMING, 2)][..],
            } {
                let Some(scan) = reader.scan(*family, *entity)? else {
                    continue;
                };
                for entry in scan.entries {
                    if entry.key.len() != 32
                        || entry.key[..16] != *entity.record().as_bytes()
                        || entry.value.len() != 16
                    {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    let id = reader.reference(&entry.key[16..])?;
                    let neighbor = reader.reference(&entry.value)?;
                    let existing = candidates.entry(id).or_insert((neighbor, 0));
                    if existing.0 != neighbor {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    existing.1 |= bit;
                }
            }
            let mut visible = Vec::new();
            for (id, (neighbor, directions)) in candidates {
                if !authorize(Action::ReadRecord, Target::Record(id))
                    || !authorize(Action::ExpandGraph, Target::Record(id))
                {
                    continue;
                }
                let record = reader.record(id)?;
                let Record::Relationship(relationship) = &record else {
                    return Err(GraphDiskError::IndexCorrupt);
                };
                if relationship.status != AssertionStatus::Accepted
                    || (directions & 1 != 0
                        && (relationship.from != *entity || relationship.to != neighbor))
                    || (directions & 2 != 0
                        && (relationship.to != *entity || relationship.from != neighbor))
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                if !record_references_are_authorized(&record, authorize)
                    || !authorize(Action::ReadRecord, Target::Record(neighbor))
                    || !authorize(Action::ExpandGraph, Target::Record(neighbor))
                {
                    continue;
                }
                let neighbor_record = reader.record(neighbor)?;
                if !record_references_are_authorized(&neighbor_record, authorize) {
                    continue;
                }
                let Record::Entity(entity_record) = neighbor_record else {
                    return Err(GraphDiskError::IndexCorrupt);
                };
                admit_result(visible.len(), *maximum)?;
                let Record::Relationship(relationship) = record else {
                    return Err(GraphDiskError::IndexCorrupt);
                };
                visible.push(GraphNeighbor {
                    relationship,
                    entity: entity_record,
                });
            }
            Ok(GraphReadOutput::Adjacent(visible))
        }
        GraphReadRequest::SupportedBy { evidence, maximum } => {
            check_maximum(*maximum)?;
            let mut visible = Vec::new();
            if let Some(scan) = reader.scan(FAMILY_PROVENANCE, *evidence)? {
                for entry in scan.entries {
                    if entry.key.len() != 32
                        || entry.key[..16] != *evidence.record().as_bytes()
                        || !entry.value.is_empty()
                    {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    let id = reader.reference(&entry.key[16..])?;
                    if !authorize(Action::ReadRecord, Target::Record(id)) {
                        continue;
                    }
                    let record = reader.record(id)?;
                    let supported = match &record {
                        Record::Assertion(claim) => claim.evidence.contains(evidence),
                        Record::Relationship(claim) => claim.evidence.contains(evidence),
                        _ => false,
                    };
                    if !supported {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    if !record_references_are_authorized(&record, authorize) {
                        continue;
                    }
                    admit_result(visible.len(), *maximum)?;
                    visible.push(record);
                }
            }
            Ok(GraphReadOutput::Supported(visible))
        }
        _ => Err(GraphDiskError::UnsupportedRequest),
    }
}

fn check_maximum(maximum: usize) -> Result<(), GraphDiskError> {
    if maximum > MAX_TRAVERSAL_RESULTS {
        return Err(GraphError::ResourceLimit.into());
    }
    Ok(())
}

fn admit_result(current: usize, maximum: usize) -> Result<(), GraphDiskError> {
    if current == maximum {
        return Err(GraphError::ResultLimit {
            actual: current + 1,
            maximum,
        }
        .into());
    }
    Ok(())
}
