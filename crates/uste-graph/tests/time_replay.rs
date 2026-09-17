use uste_graph::{
    Expected, GraphState, GraphTransaction, NewEntity, NewRecord, Operation, Record,
    encode_transaction,
};
use uste_replay::{ReplayEvent, cold_replay};
use uste_time::{
    ClockUncertainty, SourceDescriptor, TimeInput, TimeNormalizer, TimestampRole,
    envelope_from_value, envelope_to_value,
};
use uste_txn::TransactionState;
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef,
    Value,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x61; 16]),
        NamespaceId::from_bytes([0x62; 16]),
    )
}

fn record(value: u8) -> RecordRef {
    let scope = scope();
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

#[test]
fn graph_cold_replay_preserves_the_accepted_time_envelope_without_resolution() {
    let source = record(1);
    let event = record(2);
    let envelope = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(source, 1, "source.json:/event_time").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Local {
                text: "2026-11-01T01:30:00",
                zone: Some("America/Chicago"),
                offset_seconds: Some(-18_000),
                fold: Some(uste_time::FoldChoice::Earlier),
            },
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    let accepted = envelope.resolution().instant().unwrap();
    let transaction = GraphTransaction::new(
        scope(),
        vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: source,
                    entity_type: BoundedString::new("source-artifact".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: event,
                    entity_type: BoundedString::new("event".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: envelope_to_value(&envelope).unwrap(),
                }),
            },
        ],
    );
    let canonical_request = encode_transaction(&transaction).unwrap();
    let revision = CommitRevision::FIRST;
    let prepared = TransactionState::prepare(
        &GraphState::new(scope()),
        &canonical_request,
        None,
        revision,
    )
    .unwrap();
    let expected_result_digest = GraphState::result_digest(&prepared);
    let (replayed, report) = cold_replay(
        GraphState::new(scope()),
        [ReplayEvent {
            revision,
            canonical_request: &canonical_request,
            blob_inventory: None,
            expected_result_digest,
        }],
    )
    .unwrap();
    assert_eq!(report.frontier, Some(revision));
    let snapshot = replayed.snapshot();
    let Some(Record::Entity(entity)) = snapshot.record(event) else {
        panic!("replayed event entity")
    };
    let replayed_envelope = envelope_from_value(entity.properties.clone()).unwrap();
    assert_eq!(replayed_envelope, envelope);
    assert_eq!(replayed_envelope.resolution().instant(), Some(accepted));
}
