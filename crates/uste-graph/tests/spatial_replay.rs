use uste_graph::{
    Expected, GraphState, GraphTransaction, NewEntity, NewRecord, Operation, Record,
    encode_transaction,
};
use uste_replay::{
    ReplayEvent, capture_reducer_checkpoint, cold_replay, verify_reducer_checkpoint,
};
use uste_spatial::{
    CoordinateSystem, FrameDefinition, LocalPoint2, Nanometres, ObservationKey,
    PositionObservation, PositionUncertainty, SpatialPosition, SpatialRecord, SpatialVersion,
    VersionedRecordRef, WorldDefinition, record_from_value, record_to_value,
};
use uste_time::{ClockUncertainty, SourceDescriptor, TimeInput, TimeNormalizer, TimestampRole};
use uste_txn::TransactionState;
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef,
    SourceEventId, SourceEventRef, Value,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x71; 16]),
        NamespaceId::from_bytes([0x72; 16]),
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

fn entity(id: RecordRef, entity_type: &str, properties: Value) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id,
            entity_type: BoundedString::new(entity_type.to_owned()).unwrap(),
            schema_version: 1,
            properties,
        }),
    }
}

#[test]
fn graph_opaque_values_preserve_spatial_bytes_but_do_not_validate_spatial_references() {
    let world_id = record(10);
    let frame_id = record(11);
    let tracked_id = record(30);
    let evidence_id = record(40);
    let source_id = record(41);
    let observation_id = record(50);
    let frame_ref = VersionedRecordRef::new(frame_id, SpatialVersion::FIRST);
    let world = SpatialRecord::World(
        WorldDefinition::new(world_id, SpatialVersion::FIRST, frame_ref).unwrap(),
    );
    let frame = SpatialRecord::Frame(
        FrameDefinition::new(
            frame_id,
            SpatialVersion::FIRST,
            world_id,
            CoordinateSystem::LocalCartesian2,
            None,
        )
        .unwrap(),
    );
    let source_time = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(evidence_id, 1, "track.csv:row=7/time").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Rfc3339("2026-09-17T12:00:00Z"),
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    let observation = SpatialRecord::Observation(Box::new(
        PositionObservation::new(
            observation_id,
            tracked_id,
            world_id,
            frame_ref,
            ObservationKey::new(
                source_id,
                "offline-import-1",
                SourceEventRef::new(
                    scope().database(),
                    scope().namespace(),
                    SourceEventId::from_bytes([51; 16]),
                ),
            )
            .unwrap(),
            evidence_id,
            source_time,
            SpatialPosition::Local2(LocalPoint2 {
                x: Nanometres::new(125),
                y: Nanometres::new(-250),
            }),
            PositionUncertainty::Unknown,
            None,
        )
        .unwrap(),
    ));
    let transaction = GraphTransaction::new(
        scope(),
        vec![
            entity(world_id, "world", record_to_value(&world).unwrap()),
            entity(frame_id, "spatial-frame", record_to_value(&frame).unwrap()),
            entity(tracked_id, "tracked-item", Value::Null),
            entity(evidence_id, "source-artifact", Value::Null),
            entity(source_id, "observation-source", Value::Null),
            entity(
                observation_id,
                "position-observation",
                record_to_value(&observation).unwrap(),
            ),
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
    let (replayed, _) = cold_replay(
        GraphState::new(scope()),
        [ReplayEvent {
            revision,
            canonical_request: &canonical_request,
            blob_inventory: None,
            expected_result_digest,
        }],
    )
    .unwrap();
    let checkpoint = capture_reducer_checkpoint(&replayed).unwrap();
    let restored = verify_reducer_checkpoint::<GraphState>(scope(), &checkpoint).unwrap();
    let snapshot = restored.snapshot();
    let Some(Record::Entity(entity)) = snapshot.record(observation_id) else {
        panic!("replayed observation entity")
    };
    assert_eq!(
        record_from_value(entity.properties.clone()).unwrap(),
        observation
    );
}
