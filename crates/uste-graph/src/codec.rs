//! Strict canonical graph transaction codec built on the accepted format-1.0 value frame.

use core::fmt;
use uste_policy::{
    NamespaceGrant, NamespacePolicy, PermissionSet, PolicyVersion, PrincipalDigest, QuotaLimits,
};
use uste_types::{
    BoundedBytes, BoundedList, BoundedString, CanonicalMap, CommitRevision, DatabaseId,
    DecodeError, EncodeError, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant, Value,
    decode_map_value, decode_value, encode_value,
};

use crate::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, DurablePolicyMutation,
    EntityLifecycle, EntityRecord, EvidenceRecord, Expected, GraphTransaction, IntervalBound,
    NewAssertion, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation, Predicate, Record,
    RecordVersion, RelationshipRecord, ValidTime,
};

const PROFILE: &str = "uste-graph-request-v1";
const RECORD_RULE_ENTRY_BYTES: usize = 24;
const RECORD_RULE_ENTRIES_PER_CHUNK: usize = uste_types::MAX_INLINE_BYTES / RECORD_RULE_ENTRY_BYTES;
const RECORD_RULE_CHUNK_BYTES: usize = RECORD_RULE_ENTRIES_PER_CHUNK * RECORD_RULE_ENTRY_BYTES;

pub fn encode_transaction(transaction: &GraphTransaction) -> Result<Vec<u8>, GraphCodecError> {
    let operations = transaction
        .operations()
        .iter()
        .map(encode_operation)
        .collect::<Result<Vec<_>, _>>()?;
    encode_value(&map([
        ("operations", list(operations)?),
        (
            "policy_mutation",
            transaction
                .policy_mutation()
                .map(encode_policy_mutation)
                .transpose()?
                .unwrap_or(Value::Null),
        ),
        ("profile", text(PROFILE)?),
        ("scope", encode_scope(transaction.scope())?),
    ])?)
    .map_err(GraphCodecError::Encode)
}

pub fn decode_transaction(input: &[u8]) -> Result<GraphTransaction, GraphCodecError> {
    let mut root = Fields::new(decode_value(input).map_err(GraphCodecError::Decode)?)?;
    expect_text(root.take("profile")?, PROFILE)?;
    let operations = take_list(root.take("operations")?)?
        .into_iter()
        .map(decode_operation)
        .collect::<Result<Vec<_>, _>>()?;
    let policy_mutation = decode_optional(root.take("policy_mutation")?, decode_policy_mutation)?;
    let scope = decode_scope(root.take("scope")?)?;
    root.finish()?;
    Ok(match policy_mutation {
        Some(mutation) => GraphTransaction::with_policy_mutation(scope, operations, mutation),
        None => GraphTransaction::new(scope, operations),
    })
}

fn encode_operation(operation: &Operation) -> Result<Value, GraphCodecError> {
    match operation {
        Operation::Create { expected, record } => map([
            ("expected", encode_expected(expected)?),
            ("kind", text("create")?),
            ("record", encode_new_record(record)?),
        ]),
        Operation::ReplaceEntity {
            target,
            expected,
            properties,
        } => map([
            ("expected", encode_expected(expected)?),
            ("kind", text("replace_entity")?),
            ("properties", properties.clone()),
            ("target", Value::RecordRef(*target)),
        ]),
        Operation::ActOnAssertion {
            target,
            expected,
            action,
            correction,
            correction_expected,
        } => map([
            ("action", text(action_name(*action))?),
            (
                "correction",
                correction
                    .as_ref()
                    .map(encode_assertion)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            (
                "correction_expected",
                correction_expected
                    .as_ref()
                    .map(encode_expected)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            ("expected", encode_expected(expected)?),
            ("kind", text("act_assertion")?),
            ("target", Value::RecordRef(*target)),
        ]),
        Operation::ActOnRelationship {
            target,
            expected,
            action,
            correction,
            correction_expected,
        } => map([
            ("action", text(action_name(*action))?),
            (
                "correction",
                correction
                    .as_ref()
                    .map(encode_relationship)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            (
                "correction_expected",
                correction_expected
                    .as_ref()
                    .map(encode_expected)
                    .transpose()?
                    .unwrap_or(Value::Null),
            ),
            ("expected", encode_expected(expected)?),
            ("kind", text("act_relationship")?),
            ("target", Value::RecordRef(*target)),
        ]),
        Operation::DeleteEntity {
            target,
            expected,
            policy,
            affected,
        } => map([
            ("affected", encode_refs(affected)?),
            ("expected", encode_expected(expected)?),
            ("kind", text("delete_entity")?),
            ("policy", encode_delete_policy(*policy)?),
            ("target", Value::RecordRef(*target)),
        ]),
    }
}

fn decode_operation(value: Value) -> Result<Operation, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let operation = match kind.as_str() {
        "create" => Operation::Create {
            expected: decode_expected(fields.take("expected")?)?,
            record: decode_new_record(fields.take("record")?)?,
        },
        "replace_entity" => Operation::ReplaceEntity {
            target: take_record(fields.take("target")?)?,
            expected: decode_expected(fields.take("expected")?)?,
            properties: fields.take("properties")?,
        },
        "act_assertion" => Operation::ActOnAssertion {
            target: take_record(fields.take("target")?)?,
            expected: decode_expected(fields.take("expected")?)?,
            action: decode_action(fields.take("action")?)?,
            correction: decode_optional(fields.take("correction")?, decode_assertion)?,
            correction_expected: decode_optional(
                fields.take("correction_expected")?,
                decode_expected,
            )?,
        },
        "act_relationship" => Operation::ActOnRelationship {
            target: take_record(fields.take("target")?)?,
            expected: decode_expected(fields.take("expected")?)?,
            action: decode_action(fields.take("action")?)?,
            correction: decode_optional(fields.take("correction")?, decode_relationship)?,
            correction_expected: decode_optional(
                fields.take("correction_expected")?,
                decode_expected,
            )?,
        },
        "delete_entity" => Operation::DeleteEntity {
            affected: decode_refs(fields.take("affected")?)?,
            target: take_record(fields.take("target")?)?,
            expected: decode_expected(fields.take("expected")?)?,
            policy: decode_delete_policy(fields.take("policy")?)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(operation)
}

fn encode_new_record(record: &NewRecord) -> Result<Value, GraphCodecError> {
    match record {
        NewRecord::Entity(entity) => map([
            ("entity_type", Value::String(entity.entity_type.clone())),
            ("id", Value::RecordRef(entity.id)),
            ("kind", text("entity")?),
            ("properties", entity.properties.clone()),
            (
                "schema_version",
                Value::Unsigned(u128::from(entity.schema_version)),
            ),
        ]),
        NewRecord::Evidence(evidence) => map([
            (
                "digest",
                Value::Bytes(
                    BoundedBytes::new(evidence.digest.to_vec())
                        .map_err(|_| GraphCodecError::ResourceLimit)?,
                ),
            ),
            ("id", Value::RecordRef(evidence.id)),
            ("kind", text("evidence")?),
            ("locator", Value::String(evidence.locator.clone())),
        ]),
        NewRecord::Assertion(assertion) => encode_assertion(assertion),
        NewRecord::Relationship(relationship) => encode_relationship(relationship),
    }
}

pub(crate) fn encode_result_record(record: &Record) -> Result<Vec<u8>, GraphCodecError> {
    let value = match record {
        Record::Entity(entity) => map([
            (
                "created_revision",
                Value::Unsigned(u128::from(entity.created_revision.get())),
            ),
            ("entity_type", Value::String(entity.entity_type.clone())),
            ("id", Value::RecordRef(entity.id)),
            ("kind", text("entity")?),
            (
                "lifecycle",
                text(match entity.lifecycle {
                    EntityLifecycle::Active => "active",
                    EntityLifecycle::Deleted => "deleted",
                })?,
            ),
            (
                "modified_revision",
                Value::Unsigned(u128::from(entity.modified_revision.get())),
            ),
            ("properties", entity.properties.clone()),
            (
                "schema_version",
                Value::Unsigned(u128::from(entity.schema_version)),
            ),
            ("version", Value::Unsigned(u128::from(entity.version.get()))),
        ])?,
        Record::Evidence(evidence) => map([
            (
                "created_revision",
                Value::Unsigned(u128::from(evidence.created_revision.get())),
            ),
            (
                "digest",
                Value::Bytes(
                    BoundedBytes::new(evidence.digest.to_vec())
                        .map_err(|_| GraphCodecError::ResourceLimit)?,
                ),
            ),
            ("id", Value::RecordRef(evidence.id)),
            ("kind", text("evidence")?),
            ("locator", Value::String(evidence.locator.clone())),
            (
                "version",
                Value::Unsigned(u128::from(evidence.version.get())),
            ),
        ])?,
        Record::Assertion(assertion) => map([
            (
                "correction_of",
                assertion
                    .correction_of
                    .map_or(Value::Null, Value::RecordRef),
            ),
            ("evidence", encode_refs(&assertion.evidence)?),
            ("id", Value::RecordRef(assertion.id)),
            ("kind", text("assertion")?),
            (
                "modified_revision",
                Value::Unsigned(u128::from(assertion.modified_revision.get())),
            ),
            ("object", assertion.object.clone()),
            ("predicate", Value::String(assertion.predicate.clone())),
            (
                "recorded_revision",
                Value::Unsigned(u128::from(assertion.recorded_revision.get())),
            ),
            ("status", text(assertion_status_name(assertion.status))?),
            ("subject", Value::RecordRef(assertion.subject)),
            ("valid_time", encode_valid_time(assertion.valid_time)?),
            (
                "version",
                Value::Unsigned(u128::from(assertion.version.get())),
            ),
        ])?,
        Record::Relationship(relationship) => map([
            (
                "correction_of",
                relationship
                    .correction_of
                    .map_or(Value::Null, Value::RecordRef),
            ),
            ("evidence", encode_refs(&relationship.evidence)?),
            ("from", Value::RecordRef(relationship.from)),
            ("id", Value::RecordRef(relationship.id)),
            ("kind", text("relationship")?),
            (
                "modified_revision",
                Value::Unsigned(u128::from(relationship.modified_revision.get())),
            ),
            ("properties", relationship.properties.clone()),
            (
                "recorded_revision",
                Value::Unsigned(u128::from(relationship.recorded_revision.get())),
            ),
            (
                "relationship_type",
                Value::String(relationship.relationship_type.clone()),
            ),
            ("status", text(assertion_status_name(relationship.status))?),
            ("to", Value::RecordRef(relationship.to)),
            ("valid_time", encode_valid_time(relationship.valid_time)?),
            (
                "version",
                Value::Unsigned(u128::from(relationship.version.get())),
            ),
        ])?,
    };
    encode_value(&value).map_err(GraphCodecError::Encode)
}

/// Canonical complete stored-record encoding used by authenticated derived projections.
///
/// This is not a transaction request and does not grant publication authority.
pub fn encode_stored_record(record: &Record) -> Result<Vec<u8>, GraphCodecError> {
    encode_result_record(record)
}

pub(crate) fn decode_result_record(input: &[u8]) -> Result<Record, GraphCodecError> {
    decode_record_fields(Fields::new(
        decode_value(input).map_err(GraphCodecError::Decode)?,
    )?)
}

fn decode_record_fields(mut fields: impl RecordFields) -> Result<Record, GraphCodecError> {
    let kind = take_text(fields.take("kind")?)?;
    let record = match kind.as_str() {
        "entity" => Record::Entity(EntityRecord {
            id: take_record(fields.take("id")?)?,
            version: take_record_version(fields.take("version")?)?,
            lifecycle: match take_text(fields.take("lifecycle")?)?.as_str() {
                "active" => EntityLifecycle::Active,
                "deleted" => EntityLifecycle::Deleted,
                _ => return Err(GraphCodecError::InvalidEnum),
            },
            entity_type: take_bounded_text(fields.take("entity_type")?)?,
            schema_version: take_u64(fields.take("schema_version")?)?,
            properties: fields.take("properties")?,
            created_revision: take_revision(fields.take("created_revision")?)?,
            modified_revision: take_revision(fields.take("modified_revision")?)?,
        }),
        "evidence" => Record::Evidence(EvidenceRecord {
            id: take_record(fields.take("id")?)?,
            version: take_record_version(fields.take("version")?)?,
            digest: take_digest(fields.take("digest")?)?,
            locator: take_bounded_text(fields.take("locator")?)?,
            created_revision: take_revision(fields.take("created_revision")?)?,
        }),
        "assertion" => Record::Assertion(AssertionRecord {
            id: take_record(fields.take("id")?)?,
            version: take_record_version(fields.take("version")?)?,
            subject: take_record(fields.take("subject")?)?,
            predicate: take_bounded_text(fields.take("predicate")?)?,
            object: fields.take("object")?,
            evidence: decode_refs(fields.take("evidence")?)?,
            status: decode_assertion_status(fields.take("status")?)?,
            valid_time: decode_valid_time(fields.take("valid_time")?)?,
            correction_of: decode_optional(fields.take("correction_of")?, take_record)?,
            recorded_revision: take_revision(fields.take("recorded_revision")?)?,
            modified_revision: take_revision(fields.take("modified_revision")?)?,
        }),
        "relationship" => Record::Relationship(RelationshipRecord {
            id: take_record(fields.take("id")?)?,
            version: take_record_version(fields.take("version")?)?,
            from: take_record(fields.take("from")?)?,
            to: take_record(fields.take("to")?)?,
            relationship_type: take_bounded_text(fields.take("relationship_type")?)?,
            properties: fields.take("properties")?,
            evidence: decode_refs(fields.take("evidence")?)?,
            status: decode_assertion_status(fields.take("status")?)?,
            valid_time: decode_valid_time(fields.take("valid_time")?)?,
            correction_of: decode_optional(fields.take("correction_of")?, take_record)?,
            recorded_revision: take_revision(fields.take("recorded_revision")?)?,
            modified_revision: take_revision(fields.take("modified_revision")?)?,
        }),
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(record)
}

/// Decode one complete canonical stored record from a trusted derived projection.
pub fn decode_stored_record(input: &[u8]) -> Result<Record, GraphCodecError> {
    let fields = decode_map_value(input)
        .map_err(GraphCodecError::Decode)?
        .ok_or(GraphCodecError::WrongType)?;
    decode_record_fields(BorrowedFields(fields))
}

pub(crate) fn encode_result_policy(
    policy: Option<&NamespacePolicy>,
) -> Result<Vec<u8>, GraphCodecError> {
    let value = policy
        .map(encode_policy)
        .transpose()?
        .unwrap_or(Value::Null);
    encode_value(&value).map_err(GraphCodecError::Encode)
}

pub(crate) fn decode_result_policy(
    input: &[u8],
) -> Result<Option<NamespacePolicy>, GraphCodecError> {
    decode_optional(
        decode_value(input).map_err(GraphCodecError::Decode)?,
        decode_policy,
    )
}

const fn assertion_status_name(status: AssertionStatus) -> &'static str {
    match status {
        AssertionStatus::Proposed => "proposed",
        AssertionStatus::Accepted => "accepted",
        AssertionStatus::Rejected => "rejected",
        AssertionStatus::Disputed => "disputed",
        AssertionStatus::Superseded => "superseded",
        AssertionStatus::Retracted => "retracted",
        AssertionStatus::Expired => "expired",
    }
}

fn decode_assertion_status(value: Value) -> Result<AssertionStatus, GraphCodecError> {
    match take_text(value)?.as_str() {
        "proposed" => Ok(AssertionStatus::Proposed),
        "accepted" => Ok(AssertionStatus::Accepted),
        "rejected" => Ok(AssertionStatus::Rejected),
        "disputed" => Ok(AssertionStatus::Disputed),
        "superseded" => Ok(AssertionStatus::Superseded),
        "retracted" => Ok(AssertionStatus::Retracted),
        "expired" => Ok(AssertionStatus::Expired),
        _ => Err(GraphCodecError::InvalidEnum),
    }
}

fn decode_new_record(value: Value) -> Result<NewRecord, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let record = match kind.as_str() {
        "entity" => NewRecord::Entity(NewEntity {
            id: take_record(fields.take("id")?)?,
            entity_type: take_bounded_text(fields.take("entity_type")?)?,
            schema_version: take_u64(fields.take("schema_version")?)?,
            properties: fields.take("properties")?,
        }),
        "evidence" => NewRecord::Evidence(NewEvidence {
            id: take_record(fields.take("id")?)?,
            digest: take_digest(fields.take("digest")?)?,
            locator: take_bounded_text(fields.take("locator")?)?,
        }),
        "assertion" => return decode_assertion_fields(fields).map(NewRecord::Assertion),
        "relationship" => {
            return decode_relationship_fields(fields).map(NewRecord::Relationship);
        }
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(record)
}

fn encode_assertion(assertion: &NewAssertion) -> Result<Value, GraphCodecError> {
    map([
        ("evidence", encode_refs(&assertion.evidence)?),
        ("id", Value::RecordRef(assertion.id)),
        ("kind", text("assertion")?),
        ("object", assertion.object.clone()),
        ("predicate", Value::String(assertion.predicate.clone())),
        ("subject", Value::RecordRef(assertion.subject)),
        ("valid_time", encode_valid_time(assertion.valid_time)?),
    ])
}

fn decode_assertion(value: Value) -> Result<NewAssertion, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    expect_text(fields.take("kind")?, "assertion")?;
    decode_assertion_fields(fields)
}

fn decode_assertion_fields(mut fields: Fields) -> Result<NewAssertion, GraphCodecError> {
    let assertion = NewAssertion {
        id: take_record(fields.take("id")?)?,
        subject: take_record(fields.take("subject")?)?,
        predicate: take_bounded_text(fields.take("predicate")?)?,
        object: fields.take("object")?,
        evidence: decode_refs(fields.take("evidence")?)?,
        valid_time: decode_valid_time(fields.take("valid_time")?)?,
    };
    fields.finish()?;
    Ok(assertion)
}

fn encode_relationship(relationship: &NewRelationship) -> Result<Value, GraphCodecError> {
    map([
        ("evidence", encode_refs(&relationship.evidence)?),
        ("from", Value::RecordRef(relationship.from)),
        ("id", Value::RecordRef(relationship.id)),
        ("kind", text("relationship")?),
        ("properties", relationship.properties.clone()),
        (
            "relationship_type",
            Value::String(relationship.relationship_type.clone()),
        ),
        ("to", Value::RecordRef(relationship.to)),
        ("valid_time", encode_valid_time(relationship.valid_time)?),
    ])
}

fn decode_relationship(value: Value) -> Result<NewRelationship, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    expect_text(fields.take("kind")?, "relationship")?;
    decode_relationship_fields(fields)
}

fn decode_relationship_fields(mut fields: Fields) -> Result<NewRelationship, GraphCodecError> {
    let relationship = NewRelationship {
        id: take_record(fields.take("id")?)?,
        from: take_record(fields.take("from")?)?,
        to: take_record(fields.take("to")?)?,
        relationship_type: take_bounded_text(fields.take("relationship_type")?)?,
        properties: fields.take("properties")?,
        evidence: decode_refs(fields.take("evidence")?)?,
        valid_time: decode_valid_time(fields.take("valid_time")?)?,
    };
    fields.finish()?;
    Ok(relationship)
}

fn encode_expected(expected: &Expected) -> Result<Value, GraphCodecError> {
    match expected {
        Expected::Absent => map([("kind", text("absent")?)]),
        Expected::Version(version) => map([
            ("kind", text("version")?),
            ("version", Value::Unsigned(u128::from(version.get()))),
        ]),
        Expected::ReadView {
            revision,
            predicate,
        } => map([
            ("kind", text("read_view")?),
            ("predicate", encode_predicate(predicate)?),
            ("revision", Value::Unsigned(u128::from(revision.get()))),
        ]),
    }
}

fn decode_expected(value: Value) -> Result<Expected, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let expected = match kind.as_str() {
        "absent" => Expected::Absent,
        "version" => Expected::Version(
            RecordVersion::new(take_u64(fields.take("version")?)?)
                .map_err(|_| GraphCodecError::InvalidNumber)?,
        ),
        "read_view" => Expected::ReadView {
            revision: CommitRevision::new(take_u64(fields.take("revision")?)?)
                .map_err(|_| GraphCodecError::InvalidNumber)?,
            predicate: decode_predicate(fields.take("predicate")?)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(expected)
}

fn encode_predicate(predicate: &Predicate) -> Result<Value, GraphCodecError> {
    match predicate {
        Predicate::RecordAbsent(record) => map([
            ("kind", text("absent")?),
            ("record", Value::RecordRef(*record)),
        ]),
        Predicate::RecordVisible(record) => map([
            ("kind", text("visible")?),
            ("record", Value::RecordRef(*record)),
        ]),
        Predicate::RecordVersion { record, version } => map([
            ("kind", text("version")?),
            ("record", Value::RecordRef(*record)),
            ("version", Value::Unsigned(u128::from(version.get()))),
        ]),
    }
}

fn decode_predicate(value: Value) -> Result<Predicate, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let record = take_record(fields.take("record")?)?;
    let predicate = match kind.as_str() {
        "absent" => Predicate::RecordAbsent(record),
        "visible" => Predicate::RecordVisible(record),
        "version" => Predicate::RecordVersion {
            record,
            version: RecordVersion::new(take_u64(fields.take("version")?)?)
                .map_err(|_| GraphCodecError::InvalidNumber)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(predicate)
}

fn encode_valid_time(valid_time: ValidTime) -> Result<Value, GraphCodecError> {
    match valid_time {
        ValidTime::Unknown => map([("kind", text("unknown")?)]),
        ValidTime::HalfOpen { start, end } => map([
            ("end", encode_bound(end)?),
            ("kind", text("half_open")?),
            ("start", encode_bound(start)?),
        ]),
    }
}

fn decode_valid_time(value: Value) -> Result<ValidTime, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let valid_time = match kind.as_str() {
        "unknown" => ValidTime::Unknown,
        "half_open" => ValidTime::HalfOpen {
            start: decode_bound(fields.take("start")?)?,
            end: decode_bound(fields.take("end")?)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(valid_time)
}

fn encode_bound(bound: IntervalBound) -> Result<Value, GraphCodecError> {
    match bound {
        IntervalBound::Unbounded => map([("kind", text("unbounded")?)]),
        IntervalBound::Bounded(instant) => map([
            ("instant", Value::Instant(instant)),
            ("kind", text("bounded")?),
        ]),
    }
}

fn decode_bound(value: Value) -> Result<IntervalBound, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let bound = match kind.as_str() {
        "unbounded" => IntervalBound::Unbounded,
        "bounded" => IntervalBound::Bounded(take_instant(fields.take("instant")?)?),
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(bound)
}

fn encode_delete_policy(policy: DeletePolicy) -> Result<Value, GraphCodecError> {
    match policy {
        DeletePolicy::Reject => map([("kind", text("reject")?)]),
        DeletePolicy::CascadeAndRetract { maximum_affected } => map([
            ("kind", text("cascade_retract")?),
            (
                "maximum_affected",
                Value::Unsigned(u128::from(maximum_affected)),
            ),
        ]),
    }
}

fn decode_delete_policy(value: Value) -> Result<DeletePolicy, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let policy = match kind.as_str() {
        "reject" => DeletePolicy::Reject,
        "cascade_retract" => DeletePolicy::CascadeAndRetract {
            maximum_affected: take_u32(fields.take("maximum_affected")?)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(policy)
}

fn decode_action(value: Value) -> Result<AssertionAction, GraphCodecError> {
    match take_text(value)?.as_str() {
        "accept" => Ok(AssertionAction::Accept),
        "reject" => Ok(AssertionAction::Reject),
        "dispute" => Ok(AssertionAction::Dispute),
        "supersede" => Ok(AssertionAction::Supersede),
        "retract" => Ok(AssertionAction::Retract),
        "expire" => Ok(AssertionAction::Expire),
        "correct" => Ok(AssertionAction::Correct),
        _ => Err(GraphCodecError::InvalidEnum),
    }
}

fn encode_policy_mutation(mutation: &DurablePolicyMutation) -> Result<Value, GraphCodecError> {
    match mutation {
        DurablePolicyMutation::Install { policy } => map([
            ("kind", text("install")?),
            ("policy", encode_policy(policy)?),
        ]),
        DurablePolicyMutation::Replace { expected, policy } => map([
            ("expected", Value::Unsigned(u128::from(expected.get()))),
            ("kind", text("replace")?),
            ("policy", encode_policy(policy)?),
        ]),
    }
}

fn decode_policy_mutation(value: Value) -> Result<DurablePolicyMutation, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let mutation = match kind.as_str() {
        "install" => DurablePolicyMutation::Install {
            policy: decode_policy(fields.take("policy")?)?,
        },
        "replace" => DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(take_u64(fields.take("expected")?)?)
                .map_err(|_| GraphCodecError::InvalidPolicy)?,
            policy: decode_policy(fields.take("policy")?)?,
        },
        _ => return Err(GraphCodecError::InvalidEnum),
    };
    fields.finish()?;
    Ok(mutation)
}

fn encode_policy(policy: &NamespacePolicy) -> Result<Value, GraphCodecError> {
    let grants = policy
        .grants()
        .map(|(principal, grant)| {
            map([
                (
                    "permissions",
                    Value::Unsigned(u128::from(grant.permissions().bits())),
                ),
                (
                    "principal",
                    Value::Bytes(
                        BoundedBytes::new(principal.as_bytes().to_vec())
                            .map_err(|_| GraphCodecError::ResourceLimit)?,
                    ),
                ),
                ("quotas", encode_quotas(grant.quotas())?),
                ("record_rules", encode_record_rules(grant)?),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    map([
        ("grants", list(grants)?),
        ("quotas", encode_quotas(policy.quotas())?),
        ("scope", encode_scope(policy.scope())?),
        (
            "version",
            Value::Unsigned(u128::from(policy.version().get())),
        ),
    ])
}

fn decode_policy(value: Value) -> Result<NamespacePolicy, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let scope = decode_scope(fields.take("scope")?)?;
    let version = PolicyVersion::new(take_u64(fields.take("version")?)?)
        .map_err(|_| GraphCodecError::InvalidPolicy)?;
    let quotas = decode_quotas(fields.take("quotas")?)?;
    let grants = take_list(fields.take("grants")?)?;
    fields.finish()?;
    let mut policy = NamespacePolicy::new(scope, version, quotas);
    let mut previous = None;
    for encoded in grants {
        let mut grant_fields = Fields::new(encoded)?;
        let principal =
            PrincipalDigest::from_bytes(take_principal(grant_fields.take("principal")?)?);
        if previous.is_some_and(|value| value >= principal) {
            return Err(GraphCodecError::InvalidPolicy);
        }
        previous = Some(principal);
        let permissions = PermissionSet::from_bits(take_u64(grant_fields.take("permissions")?)?)
            .map_err(|_| GraphCodecError::InvalidPolicy)?;
        let mut grant =
            NamespaceGrant::new(permissions, decode_quotas(grant_fields.take("quotas")?)?);
        decode_record_rules(grant_fields.take("record_rules")?, &mut grant)?;
        grant_fields.finish()?;
        policy
            .grant(principal, grant)
            .map_err(|_| GraphCodecError::InvalidPolicy)?;
    }
    Ok(policy)
}

fn encode_quotas(quotas: QuotaLimits) -> Result<Value, GraphCodecError> {
    map([
        (
            "max_blob_read_bytes_per_call",
            Value::Unsigned(u128::from(quotas.max_blob_read_bytes_per_call())),
        ),
        (
            "max_committed_blob_bytes",
            Value::Unsigned(u128::from(quotas.max_committed_blob_bytes())),
        ),
        (
            "max_live_uploads",
            Value::Unsigned(u128::from(quotas.max_live_uploads())),
        ),
        (
            "max_request_bytes",
            Value::Unsigned(u128::from(quotas.max_request_bytes())),
        ),
        (
            "max_staged_blob_bytes",
            Value::Unsigned(u128::from(quotas.max_staged_blob_bytes())),
        ),
    ])
}

fn decode_quotas(value: Value) -> Result<QuotaLimits, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let quotas = QuotaLimits::new(
        take_u64(fields.take("max_request_bytes")?)?,
        take_u64(fields.take("max_staged_blob_bytes")?)?,
        take_u64(fields.take("max_committed_blob_bytes")?)?,
        take_u32(fields.take("max_live_uploads")?)?,
        take_u32(fields.take("max_blob_read_bytes_per_call")?)?,
    )
    .map_err(|_| GraphCodecError::InvalidPolicy)?;
    fields.finish()?;
    Ok(quotas)
}

fn encode_record_rules(grant: &NamespaceGrant) -> Result<Value, GraphCodecError> {
    let mut chunks = Vec::new();
    let mut bytes = Vec::new();
    for (record, deny) in grant.record_rules() {
        if bytes.len() == RECORD_RULE_CHUNK_BYTES {
            chunks.push(Value::Bytes(
                BoundedBytes::new(core::mem::take(&mut bytes))
                    .map_err(|_| GraphCodecError::ResourceLimit)?,
            ));
        }
        bytes.extend_from_slice(record.as_bytes());
        bytes.extend_from_slice(&deny.bits().to_be_bytes());
    }
    if !bytes.is_empty() {
        chunks.push(Value::Bytes(
            BoundedBytes::new(bytes).map_err(|_| GraphCodecError::ResourceLimit)?,
        ));
    }
    list(chunks)
}

fn decode_record_rules(value: Value, grant: &mut NamespaceGrant) -> Result<(), GraphCodecError> {
    let mut previous = None;
    let chunks = take_list(value)?;
    let chunk_count = chunks.len();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let Value::Bytes(chunk) = chunk else {
            return Err(GraphCodecError::WrongType);
        };
        let bytes = chunk.as_slice();
        if bytes.is_empty()
            || bytes.len() % RECORD_RULE_ENTRY_BYTES != 0
            || (index + 1 < chunk_count && bytes.len() != RECORD_RULE_CHUNK_BYTES)
        {
            return Err(GraphCodecError::InvalidPolicy);
        }
        for entry in bytes.chunks_exact(RECORD_RULE_ENTRY_BYTES) {
            let record = RecordId::from_bytes(
                entry[..16]
                    .try_into()
                    .map_err(|_| GraphCodecError::InvalidPolicy)?,
            );
            if previous.is_some_and(|value| value >= record) {
                return Err(GraphCodecError::InvalidPolicy);
            }
            previous = Some(record);
            let bits = u64::from_be_bytes(
                entry[16..]
                    .try_into()
                    .map_err(|_| GraphCodecError::InvalidPolicy)?,
            );
            let deny =
                PermissionSet::from_bits(bits).map_err(|_| GraphCodecError::InvalidPolicy)?;
            grant
                .deny_record(record, deny)
                .map_err(|_| GraphCodecError::InvalidPolicy)?;
        }
    }
    Ok(())
}

const fn action_name(action: AssertionAction) -> &'static str {
    match action {
        AssertionAction::Accept => "accept",
        AssertionAction::Reject => "reject",
        AssertionAction::Dispute => "dispute",
        AssertionAction::Supersede => "supersede",
        AssertionAction::Retract => "retract",
        AssertionAction::Expire => "expire",
        AssertionAction::Correct => "correct",
    }
}

fn encode_refs(references: &[RecordRef]) -> Result<Value, GraphCodecError> {
    list(references.iter().copied().map(Value::RecordRef).collect())
}

fn encode_scope(scope: NamespaceRef) -> Result<Value, GraphCodecError> {
    map([
        (
            "database",
            Value::Bytes(
                BoundedBytes::new(scope.database().as_bytes().to_vec())
                    .map_err(|_| GraphCodecError::ResourceLimit)?,
            ),
        ),
        (
            "namespace",
            Value::Bytes(
                BoundedBytes::new(scope.namespace().as_bytes().to_vec())
                    .map_err(|_| GraphCodecError::ResourceLimit)?,
            ),
        ),
    ])
}

fn decode_scope(value: Value) -> Result<NamespaceRef, GraphCodecError> {
    let mut fields = Fields::new(value)?;
    let database = take_identity(fields.take("database")?)?;
    let namespace = take_identity(fields.take("namespace")?)?;
    fields.finish()?;
    Ok(NamespaceRef::new(
        DatabaseId::from_bytes(database),
        NamespaceId::from_bytes(namespace),
    ))
}

fn decode_refs(value: Value) -> Result<Vec<RecordRef>, GraphCodecError> {
    take_list(value)?.into_iter().map(take_record).collect()
}

fn decode_optional<T>(
    value: Value,
    decode: impl FnOnce(Value) -> Result<T, GraphCodecError>,
) -> Result<Option<T>, GraphCodecError> {
    if value == Value::Null {
        Ok(None)
    } else {
        decode(value).map(Some)
    }
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Result<Value, GraphCodecError> {
    let entries = entries
        .into_iter()
        .map(|(key, value)| {
            BoundedString::new(key.to_owned())
                .map(|key| (key, value))
                .map_err(|_| GraphCodecError::ResourceLimit)
        })
        .collect::<Result<Vec<_>, _>>()?;
    CanonicalMap::new(entries)
        .map(Value::Map)
        .map_err(|_| GraphCodecError::ResourceLimit)
}

fn list(values: Vec<Value>) -> Result<Value, GraphCodecError> {
    BoundedList::new(values)
        .map(Value::List)
        .map_err(|_| GraphCodecError::ResourceLimit)
}

fn text(value: &str) -> Result<Value, GraphCodecError> {
    BoundedString::new(value.to_owned())
        .map(Value::String)
        .map_err(|_| GraphCodecError::ResourceLimit)
}

/// Consumed canonical map fields. `decode_value` already proved keys unique and ordered, so
/// rebuilding this short schema map as heap-node `String` keys adds no validation. Structured
/// graph records have a small fixed field count; linear removal reuses the decoded allocation.
struct Fields(Vec<(BoundedString, Value)>);

trait RecordFields: Sized {
    fn take(&mut self, name: &'static str) -> Result<Value, GraphCodecError>;
    fn finish(self) -> Result<(), GraphCodecError>;
}

impl Fields {
    fn new(value: Value) -> Result<Self, GraphCodecError> {
        let Value::Map(map) = value else {
            return Err(GraphCodecError::WrongType);
        };
        Ok(Self(map.into_vec()))
    }
}

impl RecordFields for Fields {
    fn take(&mut self, name: &'static str) -> Result<Value, GraphCodecError> {
        let index = self
            .0
            .iter()
            .position(|(key, _)| key.as_str() == name)
            .ok_or(GraphCodecError::MissingField(name))?;
        Ok(self.0.swap_remove(index).1)
    }

    fn finish(self) -> Result<(), GraphCodecError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(GraphCodecError::UnknownField)
        }
    }
}

struct BorrowedFields<'a>(Vec<(&'a str, Value)>);

impl RecordFields for BorrowedFields<'_> {
    fn take(&mut self, name: &'static str) -> Result<Value, GraphCodecError> {
        let index = self
            .0
            .iter()
            .position(|(key, _)| *key == name)
            .ok_or(GraphCodecError::MissingField(name))?;
        Ok(self.0.swap_remove(index).1)
    }

    fn finish(self) -> Result<(), GraphCodecError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(GraphCodecError::UnknownField)
        }
    }
}

fn take_list(value: Value) -> Result<Vec<Value>, GraphCodecError> {
    match value {
        Value::List(values) => Ok(values.into_vec()),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_text(value: Value) -> Result<String, GraphCodecError> {
    take_bounded_text(value).map(BoundedString::into_string)
}

fn take_bounded_text(value: Value) -> Result<BoundedString, GraphCodecError> {
    match value {
        Value::String(value) => Ok(value),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn expect_text(value: Value, expected: &str) -> Result<(), GraphCodecError> {
    if take_text(value)? == expected {
        Ok(())
    } else {
        Err(GraphCodecError::InvalidEnum)
    }
}

fn take_record(value: Value) -> Result<RecordRef, GraphCodecError> {
    match value {
        Value::RecordRef(value) => Ok(value),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_instant(value: Value) -> Result<UtcInstant, GraphCodecError> {
    match value {
        Value::Instant(value) => Ok(value),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_u64(value: Value) -> Result<u64, GraphCodecError> {
    match value {
        Value::Unsigned(value) => u64::try_from(value).map_err(|_| GraphCodecError::InvalidNumber),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_u32(value: Value) -> Result<u32, GraphCodecError> {
    match value {
        Value::Unsigned(value) => u32::try_from(value).map_err(|_| GraphCodecError::InvalidNumber),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_revision(value: Value) -> Result<CommitRevision, GraphCodecError> {
    CommitRevision::new(take_u64(value)?).map_err(|_| GraphCodecError::InvalidNumber)
}

fn take_record_version(value: Value) -> Result<RecordVersion, GraphCodecError> {
    RecordVersion::new(take_u64(value)?).map_err(|_| GraphCodecError::InvalidNumber)
}

fn take_digest(value: Value) -> Result<[u8; 32], GraphCodecError> {
    match value {
        Value::Bytes(value) => value
            .into_vec()
            .try_into()
            .map_err(|_| GraphCodecError::InvalidDigest),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_identity(value: Value) -> Result<[u8; 16], GraphCodecError> {
    match value {
        Value::Bytes(value) => value
            .into_vec()
            .try_into()
            .map_err(|_| GraphCodecError::InvalidIdentity),
        _ => Err(GraphCodecError::WrongType),
    }
}

fn take_principal(value: Value) -> Result<[u8; 32], GraphCodecError> {
    match value {
        Value::Bytes(value) => value
            .into_vec()
            .try_into()
            .map_err(|_| GraphCodecError::InvalidPolicy),
        _ => Err(GraphCodecError::WrongType),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum GraphCodecError {
    Decode(DecodeError),
    Encode(EncodeError),
    MissingField(&'static str),
    UnknownField,
    WrongType,
    InvalidEnum,
    InvalidNumber,
    InvalidDigest,
    InvalidIdentity,
    InvalidPolicy,
    ResourceLimit,
}

impl fmt::Display for GraphCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for GraphCodecError {}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_policy::Action;

    #[test]
    fn record_rule_chunks_have_one_canonical_partition() {
        let quotas = QuotaLimits::new(1024, 1024, 1024, 1, 1024).unwrap();
        let mut source =
            NamespaceGrant::new(PermissionSet::from_actions([Action::ReadRecord]), quotas);
        for value in [1_u8, 2] {
            source
                .deny_record(
                    RecordId::from_bytes([value; 16]),
                    PermissionSet::from_actions([Action::ReadRecord]),
                )
                .unwrap();
        }
        let Value::List(chunks) = encode_record_rules(&source).unwrap() else {
            panic!("record-rule list")
        };
        let [Value::Bytes(bytes)] = chunks.as_slice() else {
            panic!("single canonical chunk")
        };
        let split = Value::List(
            BoundedList::new(
                bytes
                    .as_slice()
                    .chunks_exact(RECORD_RULE_ENTRY_BYTES)
                    .map(|entry| Value::Bytes(BoundedBytes::new(entry.to_vec()).unwrap()))
                    .collect(),
            )
            .unwrap(),
        );
        let mut decoded = NamespaceGrant::new(PermissionSet::default(), quotas);
        assert!(matches!(
            decode_record_rules(split, &mut decoded),
            Err(GraphCodecError::InvalidPolicy)
        ));
    }
}
