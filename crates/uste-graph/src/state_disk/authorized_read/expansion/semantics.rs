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
            let mut visible = Vec::new();
            match direction {
                AdjacencyDirection::Outgoing | AdjacencyDirection::Incoming => {
                    let (family, directions) = match direction {
                        AdjacencyDirection::Outgoing => (FAMILY_OUTGOING, 1),
                        AdjacencyDirection::Incoming => (FAMILY_INCOMING, 2),
                        AdjacencyDirection::Either => unreachable!(),
                    };
                    if let Some(scan) = reader.scan(family, *entity)? {
                        for entry in scan {
                            let (id, neighbor) = adjacent_candidate(reader, *entity, entry)?;
                            admit_adjacent(
                                reader,
                                *entity,
                                id,
                                neighbor,
                                directions,
                                *maximum,
                                &mut visible,
                                authorize,
                            )?;
                        }
                    }
                }
                AdjacencyDirection::Either => {
                    // Both authenticated scans are strictly ordered by their 16-byte relationship
                    // suffix, which is also RecordRef order inside this fixed namespace. Merge them
                    // directly so self-loops retain both direction bits without rebuilding a map.
                    let outgoing = reader.scan(FAMILY_OUTGOING, *entity)?.unwrap_or_default();
                    let incoming = reader.scan(FAMILY_INCOMING, *entity)?.unwrap_or_default();
                    let mut outgoing = outgoing.into_iter().peekable();
                    let mut incoming = incoming.into_iter().peekable();
                    loop {
                        let ordering = match (outgoing.peek(), incoming.peek()) {
                            (Some(outgoing), Some(incoming)) => {
                                adjacent_id_bytes(*entity, outgoing)?
                                    .cmp(adjacent_id_bytes(*entity, incoming)?)
                            }
                            (Some(outgoing), None) => {
                                adjacent_id_bytes(*entity, outgoing)?;
                                core::cmp::Ordering::Less
                            }
                            (None, Some(incoming)) => {
                                adjacent_id_bytes(*entity, incoming)?;
                                core::cmp::Ordering::Greater
                            }
                            (None, None) => break,
                        };
                        let (id, neighbor, directions) = match ordering {
                            core::cmp::Ordering::Less => {
                                let (id, neighbor) = adjacent_candidate(
                                    reader,
                                    *entity,
                                    outgoing.next().expect("peeked outgoing candidate"),
                                )?;
                                (id, neighbor, 1)
                            }
                            core::cmp::Ordering::Greater => {
                                let (id, neighbor) = adjacent_candidate(
                                    reader,
                                    *entity,
                                    incoming.next().expect("peeked incoming candidate"),
                                )?;
                                (id, neighbor, 2)
                            }
                            core::cmp::Ordering::Equal => {
                                let (id, neighbor) = adjacent_candidate(
                                    reader,
                                    *entity,
                                    outgoing.next().expect("peeked outgoing candidate"),
                                )?;
                                let (incoming_id, incoming_neighbor) = adjacent_candidate(
                                    reader,
                                    *entity,
                                    incoming.next().expect("peeked incoming candidate"),
                                )?;
                                if incoming_id != id || incoming_neighbor != neighbor {
                                    return Err(GraphDiskError::IndexCorrupt);
                                }
                                (id, neighbor, 3)
                            }
                        };
                        admit_adjacent(
                            reader,
                            *entity,
                            id,
                            neighbor,
                            directions,
                            *maximum,
                            &mut visible,
                            authorize,
                        )?;
                    }
                }
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

fn adjacent_id_bytes(entity: RecordRef, entry: &IndexScanEntry) -> Result<&[u8], GraphDiskError> {
    if entry.key.len() != 32
        || entry.key[..16] != *entity.record().as_bytes()
        || entry.value.len() != 16
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(&entry.key[16..])
}

fn adjacent_candidate(
    reader: &impl ExpansionRead,
    entity: RecordRef,
    entry: IndexScanEntry,
) -> Result<(RecordRef, RecordRef), GraphDiskError> {
    adjacent_id_bytes(entity, &entry)?;
    Ok((
        reader.reference(&entry.key[16..])?,
        reader.reference(&entry.value)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn admit_adjacent(
    reader: &mut impl ExpansionRead,
    entity: RecordRef,
    id: RecordRef,
    neighbor: RecordRef,
    directions: u8,
    maximum: usize,
    visible: &mut Vec<GraphNeighbor>,
    authorize: &mut dyn FnMut(Action, Target) -> bool,
) -> Result<(), GraphDiskError> {
    if !authorize(Action::ReadRecord, Target::Record(id))
        || !authorize(Action::ExpandGraph, Target::Record(id))
    {
        return Ok(());
    }
    let record = reader.record(id)?;
    let Record::Relationship(relationship) = &record else {
        return Err(GraphDiskError::IndexCorrupt);
    };
    if relationship.status != AssertionStatus::Accepted
        || (directions & 1 != 0 && (relationship.from != entity || relationship.to != neighbor))
        || (directions & 2 != 0 && (relationship.to != entity || relationship.from != neighbor))
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    if !record_references_are_authorized(&record, authorize)
        || !authorize(Action::ReadRecord, Target::Record(neighbor))
        || !authorize(Action::ExpandGraph, Target::Record(neighbor))
    {
        return Ok(());
    }
    let neighbor_record = reader.record(neighbor)?;
    if !record_references_are_authorized(&neighbor_record, authorize) {
        return Ok(());
    }
    let Record::Entity(entity_record) = neighbor_record else {
        return Err(GraphDiskError::IndexCorrupt);
    };
    admit_result(visible.len(), maximum)?;
    let Record::Relationship(relationship) = record else {
        return Err(GraphDiskError::IndexCorrupt);
    };
    visible.push(GraphNeighbor {
        relationship,
        entity: entity_record,
    });
    Ok(())
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
