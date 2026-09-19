//! Shared pure graph expansion semantics; physical readers own work admission.
use super::*;
use uste_storage::IndexScanEntry;

pub(crate) trait ExpansionRead {
    fn scan(
        &mut self,
        family: u8,
        id: RecordRef,
    ) -> Result<Option<Vec<IndexScanEntry>>, GraphDiskError>;
    fn record(&mut self, id: RecordRef) -> Result<Record, GraphDiskError>;
    fn reference(&self, bytes: &[u8]) -> Result<RecordRef, GraphDiskError>;
}
pub(crate) fn read_with(
    reader: &mut impl ExpansionRead,
    request: &GraphReadRequest,
    authorize: &mut dyn FnMut(Action, Target) -> bool,
) -> Result<GraphReadOutput, GraphDiskError> {
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
                for entry in scan {
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
                for entry in scan {
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
