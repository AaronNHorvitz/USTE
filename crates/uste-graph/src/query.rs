//! Reducer-owned graph projections used by the mandatory authorization facade.

use std::collections::BTreeSet;

use uste_policy::{Action, AuthorizationRequirement, AuthorizationRequirements, Target};
use uste_txn::{ApplyError, AuthorizedReadState};
use uste_types::{CommitRevision, RecordRef, Value};

use crate::{
    AdjacencyDirection, EntityRecord, GraphError, GraphSnapshot, GraphState, MAX_TRAVERSAL_RESULTS,
    Record, RelationshipRecord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphReadRequest {
    Record {
        id: RecordRef,
    },
    RecordAt {
        id: RecordRef,
        revision: CommitRevision,
    },
    Adjacent {
        entity: RecordRef,
        direction: AdjacencyDirection,
        maximum: usize,
    },
    SupportedBy {
        evidence: RecordRef,
        maximum: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphNeighbor {
    pub relationship: RelationshipRecord,
    pub entity: EntityRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphReadOutput {
    Record(Option<Box<Record>>),
    Adjacent(Vec<GraphNeighbor>),
    Supported(Vec<Record>),
}

impl AuthorizedReadState for GraphState {
    type ReadRequest = GraphReadRequest;
    type ReadOutput = GraphReadOutput;
    type ReadError = GraphError;

    fn read_authorization_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let mut requirements = BTreeSet::new();
        match request {
            GraphReadRequest::Record { id } => {
                requirements.insert((Action::ReadRecord, *id));
            }
            GraphReadRequest::RecordAt { id, .. } => {
                requirements.insert((Action::ReadRecord, *id));
                requirements.insert((Action::ReadHistory, *id));
            }
            GraphReadRequest::Adjacent { entity, .. } => {
                requirements.insert((Action::ReadRecord, *entity));
                requirements.insert((Action::ExpandGraph, *entity));
            }
            GraphReadRequest::SupportedBy { evidence, .. } => {
                requirements.insert((Action::ReadRecord, *evidence));
                requirements.insert((Action::ExpandGraph, *evidence));
            }
        }
        AuthorizationRequirements::new(requirements.into_iter().map(|(action, record)| {
            AuthorizationRequirement {
                action,
                target: Target::Record(record),
            }
        }))
        .map_err(|_| ApplyError::ResourceLimit)
    }

    fn read_authorized(
        snapshot: &GraphSnapshot,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError> {
        match request {
            GraphReadRequest::Record { id } => Ok(GraphReadOutput::Record(visible_record(
                snapshot.record(*id),
                authorize_candidate,
            ))),
            GraphReadRequest::RecordAt { id, revision } => Ok(GraphReadOutput::Record(
                visible_record(snapshot.record_at(*revision, *id)?, authorize_candidate),
            )),
            GraphReadRequest::Adjacent {
                entity,
                direction,
                maximum,
            } => {
                if *maximum > MAX_TRAVERSAL_RESULTS {
                    return Err(GraphError::ResourceLimit);
                }
                let mut visible = Vec::new();
                for relationship in snapshot.adjacent_candidates(*entity, *direction)? {
                    if !record_references_are_authorized(
                        &Record::Relationship(relationship.clone()),
                        authorize_candidate,
                    ) {
                        continue;
                    }
                    let relationship_target = Target::Record(relationship.id);
                    if !authorize_candidate(Action::ReadRecord, relationship_target)
                        || !authorize_candidate(Action::ExpandGraph, relationship_target)
                    {
                        continue;
                    }
                    let neighbor = if relationship.from == *entity {
                        relationship.to
                    } else {
                        relationship.from
                    };
                    let neighbor_target = Target::Record(neighbor);
                    if !authorize_candidate(Action::ReadRecord, neighbor_target)
                        || !authorize_candidate(Action::ExpandGraph, neighbor_target)
                    {
                        continue;
                    }
                    let Some(Record::Entity(entity_record)) = snapshot.record(neighbor) else {
                        return Err(GraphError::IndexCorrupt(neighbor));
                    };
                    if !record_references_are_authorized(
                        &Record::Entity(entity_record.clone()),
                        authorize_candidate,
                    ) {
                        continue;
                    }
                    if visible.len() == *maximum {
                        return Err(GraphError::ResultLimit {
                            actual: visible.len().saturating_add(1),
                            maximum: *maximum,
                        });
                    }
                    visible.push(GraphNeighbor {
                        relationship: relationship.clone(),
                        entity: entity_record.clone(),
                    });
                }
                Ok(GraphReadOutput::Adjacent(visible))
            }
            GraphReadRequest::SupportedBy { evidence, maximum } => {
                if *maximum > MAX_TRAVERSAL_RESULTS {
                    return Err(GraphError::ResourceLimit);
                }
                let mut visible = Vec::new();
                for record in snapshot.supported_candidates(*evidence)? {
                    if authorize_candidate(Action::ReadRecord, Target::Record(record.id()))
                        && record_references_are_authorized(record, authorize_candidate)
                    {
                        if visible.len() == *maximum {
                            return Err(GraphError::ResultLimit {
                                actual: visible.len().saturating_add(1),
                                maximum: *maximum,
                            });
                        }
                        visible.push(record.clone());
                    }
                }
                Ok(GraphReadOutput::Supported(visible))
            }
        }
    }
}

pub(crate) fn visible_record(
    record: Option<&Record>,
    authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
) -> Option<Box<Record>> {
    record
        .filter(|record| record_references_are_authorized(record, authorize_candidate))
        .cloned()
        .map(Box::new)
}

pub(crate) fn record_references_are_authorized(
    record: &Record,
    authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
) -> bool {
    let mut authorize =
        |reference| authorize_candidate(Action::ReadRecord, Target::Record(reference));
    match record {
        Record::Entity(entity) => {
            value_references_are_authorized(&entity.properties, &mut authorize)
        }
        Record::Evidence(_) => true,
        Record::Assertion(assertion) => {
            authorize(assertion.subject)
                && assertion.evidence.iter().copied().all(&mut authorize)
                && assertion.correction_of.is_none_or(&mut authorize)
                && value_references_are_authorized(&assertion.object, &mut authorize)
        }
        Record::Relationship(relationship) => {
            authorize(relationship.from)
                && authorize(relationship.to)
                && relationship.evidence.iter().copied().all(&mut authorize)
                && relationship.correction_of.is_none_or(&mut authorize)
                && value_references_are_authorized(&relationship.properties, &mut authorize)
        }
    }
}

fn value_references_are_authorized(
    value: &Value,
    authorize: &mut dyn FnMut(RecordRef) -> bool,
) -> bool {
    match value {
        Value::List(values) => values
            .as_slice()
            .iter()
            .all(|value| value_references_are_authorized(value, authorize)),
        Value::Map(entries) => entries
            .as_slice()
            .iter()
            .all(|(_, value)| value_references_are_authorized(value, authorize)),
        Value::RecordRef(reference) => authorize(*reference),
        _ => true,
    }
}
