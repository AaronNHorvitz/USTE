mod common;

use common::{
    child_frame, geometry, observation, point, record, reference, revision, version, world_and_root,
};
use uste_spatial::{
    CoordinateSystem, FrameDefinition, FrameParent, Geometry, GeometryVersion, LocalPoint3,
    Nanometres, ObservationOutcome, PositionObservation, PositionUncertainty, SpatialCatalog,
    SpatialError, SpatialRecord, SpatialVersion,
};
use uste_testkit::{ReferenceFrame, ReferenceFrameError, ReferenceFrameHistory};

#[test]
fn forward_world_root_and_exact_reference_history_are_stable() {
    let mut catalog = SpatialCatalog::new();
    let [world, root] = world_and_root();
    catalog.apply_batch(revision(1), &[root, world]).unwrap();

    let frame_v1 = child_frame(12, 1, reference(11, 1));
    catalog
        .apply_batch(revision(2), &[SpatialRecord::Frame(frame_v1.clone())])
        .unwrap();
    let frame_v2 = child_frame(12, 2, reference(11, 1));
    catalog
        .apply_batch(revision(3), &[SpatialRecord::Frame(frame_v2.clone())])
        .unwrap();

    assert_eq!(
        catalog.frame_at(reference(12, 1), revision(2)).unwrap(),
        &frame_v1
    );
    assert_eq!(
        catalog.frame_at(reference(12, 1), revision(3)).unwrap(),
        &frame_v1
    );
    assert_eq!(
        catalog.frame_at(reference(12, 2), revision(2)),
        Err(SpatialError::MissingFrameVersion)
    );
    assert_eq!(catalog.frame_history_len(record(12)), 2);
}

#[test]
fn same_batch_sequential_versions_preserve_request_order_rules() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();
    let frame_v1 = SpatialRecord::Frame(child_frame(12, 1, reference(11, 1)));
    let frame_v2 = SpatialRecord::Frame(child_frame(12, 2, reference(11, 1)));
    catalog
        .apply_batch(revision(2), &[frame_v1.clone(), frame_v2.clone()])
        .unwrap();
    assert_eq!(catalog.frame_history_len(record(12)), 2);

    let mut reversed = SpatialCatalog::new();
    reversed
        .apply_batch(revision(1), &world_and_root())
        .unwrap();
    assert_eq!(
        reversed.apply_batch(revision(2), &[frame_v2, frame_v1]),
        Err(SpatialError::VersionConflict)
    );
    assert_eq!(reversed.frame_history_len(record(12)), 0);
    assert_eq!(reversed.last_revision(), Some(revision(1)));
}

#[test]
fn cycles_and_the_first_over_depth_chain_fail_atomically() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();

    let cycle = [
        SpatialRecord::Frame(child_frame(12, 1, reference(13, 1))),
        SpatialRecord::Frame(child_frame(13, 1, reference(12, 1))),
    ];
    assert_eq!(
        catalog.apply_batch(revision(2), &cycle),
        Err(SpatialError::FrameCycle)
    );
    assert_eq!(catalog.frame_history_len(record(12)), 0);
    let mut oracle = ReferenceFrameHistory::new(record(10), reference(11, 1));
    oracle
        .apply(&[ReferenceFrame {
            id: reference(11, 1),
            world: record(10),
            parent: None,
        }])
        .unwrap();
    assert_eq!(
        oracle.apply(&[
            ReferenceFrame {
                id: reference(12, 1),
                world: record(10),
                parent: Some(reference(13, 1)),
            },
            ReferenceFrame {
                id: reference(13, 1),
                world: record(10),
                parent: Some(reference(12, 1)),
            },
        ]),
        Err(ReferenceFrameError::Cycle)
    );

    let mut parent = reference(11, 1);
    for step in 0_u8..32 {
        let id = 20 + step;
        catalog
            .apply_batch(
                revision(u64::from(step) + 2),
                &[SpatialRecord::Frame(child_frame(id, 1, parent))],
            )
            .unwrap();
        parent = reference(id, 1);
    }
    let too_deep = SpatialRecord::Frame(child_frame(52, 1, parent));
    assert_eq!(
        catalog.apply_batch(revision(34), &[too_deep]),
        Err(SpatialError::ResourceLimit)
    );
    assert_eq!(catalog.frame_history_len(record(52)), 0);
}

#[test]
fn batch_cap_and_cross_scope_references_fail_before_publication() {
    let [world, _] = world_and_root();
    let over_limit = vec![world; uste_spatial::MAX_SPATIAL_BATCH_RECORDS + 1];
    let mut catalog = SpatialCatalog::new();
    assert_eq!(
        catalog.apply_batch(revision(1), &over_limit),
        Err(SpatialError::ResourceLimit)
    );
    assert_eq!(catalog.last_revision(), None);

    assert_eq!(
        FrameDefinition::new(
            record(12),
            SpatialVersion::FIRST,
            common::foreign_record(10),
            CoordinateSystem::LocalCartesian2,
            None,
        ),
        Err(SpatialError::ScopeMismatch)
    );
}

#[test]
fn frame_dimension_scope_and_exact_version_mismatches_reject() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();

    let wrong_dimension = GeometryVersion::new(
        record(20),
        SpatialVersion::FIRST,
        record(10),
        reference(11, 1),
        Geometry::LocalPoint3(LocalPoint3 {
            x: Nanometres::new(0),
            y: Nanometres::new(0),
            z: Nanometres::new(0),
        }),
        PositionUncertainty::Unknown,
        None,
    )
    .unwrap();
    assert_eq!(
        catalog.apply_batch(revision(2), &[SpatialRecord::Geometry(wrong_dimension)]),
        Err(SpatialError::IncompatibleFrame)
    );
    assert_eq!(
        catalog.apply_batch(
            revision(2),
            &[SpatialRecord::Geometry(geometry(20, reference(11, 2)))],
        ),
        Err(SpatialError::MissingFrameVersion)
    );
}

#[test]
fn observation_source_retry_conflict_and_corrections_are_explicit() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();
    let first = observation(50, 51, point(1, 2));
    assert_eq!(
        catalog
            .apply_batch(
                revision(2),
                &[SpatialRecord::Observation(Box::new(first.clone()))]
            )
            .unwrap(),
        vec![ObservationOutcome::Inserted { id: record(50) }]
    );
    assert_eq!(
        catalog
            .apply_batch(
                revision(3),
                &[SpatialRecord::Observation(Box::new(first.clone()))]
            )
            .unwrap(),
        vec![ObservationOutcome::Duplicate {
            existing: record(50)
        }]
    );

    let conflicting = observation(52, 51, point(9, 9));
    assert_eq!(
        catalog.apply_batch(
            revision(4),
            &[SpatialRecord::Observation(Box::new(conflicting))],
        ),
        Err(SpatialError::SourceEventConflict)
    );
    assert_eq!(catalog.last_revision(), Some(revision(3)));

    let reused_id = observation(50, 52, point(9, 9));
    assert_eq!(
        catalog.apply_batch(
            revision(4),
            &[SpatialRecord::Observation(Box::new(reused_id))],
        ),
        Err(SpatialError::DuplicateRecord)
    );
    assert_eq!(
        catalog.observation_at(record(50), revision(99)).unwrap(),
        &first
    );

    let candidate = observation(53, 53, point(5, 6));
    let correction = PositionObservation::new(
        candidate.id(),
        candidate.entity(),
        candidate.world(),
        candidate.frame(),
        candidate.key().clone(),
        candidate.evidence(),
        candidate.source_time().clone(),
        candidate.position(),
        candidate.uncertainty(),
        Some(first.id()),
    )
    .unwrap();
    assert_eq!(
        catalog
            .apply_batch(
                revision(4),
                &[SpatialRecord::Observation(Box::new(correction))],
            )
            .unwrap(),
        vec![ObservationOutcome::Inserted { id: record(53) }]
    );
}

#[test]
fn same_batch_observation_id_collision_is_atomic() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();
    let first = observation(50, 51, point(1, 2));
    let replacement = observation(50, 52, point(3, 4));
    assert_eq!(
        catalog.apply_batch(
            revision(2),
            &[
                SpatialRecord::Observation(Box::new(first)),
                SpatialRecord::Observation(Box::new(replacement)),
            ],
        ),
        Err(SpatialError::DuplicateRecord)
    );
    assert_eq!(
        catalog.observation_at(record(50), revision(99)),
        Err(SpatialError::MissingObservation)
    );
}

#[test]
fn forward_and_cyclic_corrections_are_rejected_atomically() {
    let mut catalog = SpatialCatalog::new();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();
    let left = observation(50, 51, point(1, 2));
    let right = observation(52, 53, point(3, 4));
    let left = PositionObservation::new(
        left.id(),
        left.entity(),
        left.world(),
        left.frame(),
        left.key().clone(),
        left.evidence(),
        left.source_time().clone(),
        left.position(),
        left.uncertainty(),
        Some(right.id()),
    )
    .unwrap();
    let right = PositionObservation::new(
        right.id(),
        right.entity(),
        right.world(),
        right.frame(),
        right.key().clone(),
        right.evidence(),
        right.source_time().clone(),
        right.position(),
        right.uncertainty(),
        Some(left.id()),
    )
    .unwrap();
    assert_eq!(
        catalog.apply_batch(
            revision(2),
            &[
                SpatialRecord::Observation(Box::new(left)),
                SpatialRecord::Observation(Box::new(right)),
            ],
        ),
        Err(SpatialError::InvalidCorrection)
    );
    assert_eq!(catalog.last_revision(), Some(revision(1)));
    assert_eq!(
        catalog.observation_at(record(50), revision(99)),
        Err(SpatialError::MissingObservation)
    );
}

#[test]
fn production_depth_validation_matches_independent_parent_scan() {
    let mut production = SpatialCatalog::new();
    production
        .apply_batch(revision(1), &world_and_root())
        .unwrap();
    let mut oracle = ReferenceFrameHistory::new(record(10), reference(11, 1));
    oracle
        .apply(&[ReferenceFrame {
            id: reference(11, 1),
            world: record(10),
            parent: None,
        }])
        .unwrap();
    let mut parent = reference(11, 1);
    for step in 0_u8..20 {
        let id = 60 + step;
        let frame = child_frame(id, 1, parent);
        production
            .apply_batch(
                revision(u64::from(step) + 2),
                &[SpatialRecord::Frame(frame)],
            )
            .unwrap();
        oracle
            .apply(&[ReferenceFrame {
                id: reference(id, 1),
                world: record(10),
                parent: Some(parent),
            }])
            .unwrap();
        assert!(oracle.contains(reference(id, 1)));
        parent = reference(id, 1);
    }
}

#[test]
fn malformed_root_and_nonroot_shapes_are_rejected() {
    let mut catalog = SpatialCatalog::new();
    let root_with_parent = FrameDefinition::new(
        record(11),
        SpatialVersion::FIRST,
        record(10),
        CoordinateSystem::LocalCartesian2,
        Some(FrameParent {
            frame: reference(12, 1),
            transform: reference(112, 1),
        }),
    )
    .unwrap();
    let world = world_and_root()[0].clone();
    assert_eq!(
        catalog.apply_batch(
            revision(1),
            &[world, SpatialRecord::Frame(root_with_parent)]
        ),
        Err(SpatialError::InvalidRootFrame)
    );

    let no_parent = FrameDefinition::new(
        record(12),
        version(1),
        record(10),
        CoordinateSystem::LocalCartesian2,
        None,
    )
    .unwrap();
    catalog.apply_batch(revision(1), &world_and_root()).unwrap();
    assert_eq!(
        catalog.apply_batch(revision(2), &[SpatialRecord::Frame(no_parent)]),
        Err(SpatialError::MissingFrame)
    );
}
