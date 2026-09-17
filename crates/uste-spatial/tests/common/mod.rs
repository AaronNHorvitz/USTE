#![allow(dead_code)]

use uste_spatial::{
    CoordinateSystem, FrameDefinition, FrameParent, Geometry, GeometryVersion, LocalPoint2,
    Nanometres, ObservationKey, PositionObservation, PositionUncertainty, SpatialPosition,
    SpatialRecord, SpatialVersion, VersionedRecordRef, WorldDefinition,
};
use uste_time::{
    ClockUncertainty, SourceDescriptor, TimeInput, TimeNormalizer, TimestampEnvelope, TimestampRole,
};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, RecordId, RecordRef, SourceEventId, SourceEventRef,
};

pub fn record(byte: u8) -> RecordRef {
    RecordRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
        RecordId::from_bytes([byte; 16]),
    )
}

pub fn foreign_record(byte: u8) -> RecordRef {
    RecordRef::new(
        DatabaseId::from_bytes([9; 16]),
        NamespaceId::from_bytes([2; 16]),
        RecordId::from_bytes([byte; 16]),
    )
}

pub fn event(byte: u8) -> SourceEventRef {
    SourceEventRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
        SourceEventId::from_bytes([byte; 16]),
    )
}

pub fn revision(value: u64) -> CommitRevision {
    CommitRevision::new(value).unwrap()
}

pub fn version(value: u64) -> SpatialVersion {
    SpatialVersion::new(value).unwrap()
}

pub fn reference(byte: u8, version_number: u64) -> VersionedRecordRef {
    VersionedRecordRef::new(record(byte), version(version_number))
}

pub fn timestamp(evidence: RecordRef, input: TimeInput<'_>) -> TimestampEnvelope {
    TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(evidence, 1, "row:1/position_time").unwrap(),
            TimestampRole::SourceEvent,
            input,
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap()
}

pub fn world_and_root() -> [SpatialRecord; 2] {
    [
        SpatialRecord::World(
            WorldDefinition::new(record(10), SpatialVersion::FIRST, reference(11, 1)).unwrap(),
        ),
        SpatialRecord::Frame(
            FrameDefinition::new(
                record(11),
                SpatialVersion::FIRST,
                record(10),
                CoordinateSystem::LocalCartesian2,
                None,
            )
            .unwrap(),
        ),
    ]
}

pub fn child_frame(id: u8, version_number: u64, parent: VersionedRecordRef) -> FrameDefinition {
    FrameDefinition::new(
        record(id),
        version(version_number),
        record(10),
        CoordinateSystem::LocalCartesian2,
        Some(FrameParent {
            frame: parent,
            transform: reference(id.wrapping_add(100), version_number),
        }),
    )
    .unwrap()
}

pub fn point(x: i128, y: i128) -> SpatialPosition {
    SpatialPosition::Local2(LocalPoint2 {
        x: Nanometres::new(x),
        y: Nanometres::new(y),
    })
}

pub fn geometry(id: u8, frame: VersionedRecordRef) -> GeometryVersion {
    GeometryVersion::new(
        record(id),
        SpatialVersion::FIRST,
        record(10),
        frame,
        match point(3, 4) {
            SpatialPosition::Local2(value) => Geometry::LocalPoint2(value),
            _ => unreachable!(),
        },
        PositionUncertainty::Unknown,
        None,
    )
    .unwrap()
}

pub fn observation(id: u8, event_id: u8, position: SpatialPosition) -> PositionObservation {
    let evidence = record(40);
    PositionObservation::new(
        record(id),
        record(30),
        record(10),
        reference(11, 1),
        ObservationKey::new(record(41), "session-a", event(event_id)).unwrap(),
        evidence,
        timestamp(evidence, TimeInput::Rfc3339("2026-09-17T12:00:00Z")),
        position,
        PositionUncertainty::Unknown,
        None,
    )
    .unwrap()
}
