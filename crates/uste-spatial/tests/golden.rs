mod common;

use std::collections::BTreeMap;

use common::{
    child_frame, observation, point, record, reference, revision, timestamp, world_and_root,
};
use sha2::{Digest, Sha256};
use uste_spatial::{
    GeographicBox, GeographicPoint, Geometry, GeometryVersion, LatitudeNanodegrees, LocalBox2,
    LocalBox3, LocalPoint2, LocalPoint3, LongitudeNanodegrees, Nanometres, NonNegativeNanometres,
    PositionObservation, PositionUncertainty, SpatialPosition, SpatialRecord, SpatialState,
    SpatialTransaction, SpatialVersion, decode_record, decode_transaction, encode_record,
    encode_transaction,
};
use uste_time::TimeInput;
use uste_txn::{CheckpointState, TransactionState};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

const GOLDEN: &str = include_str!("../../../acceptance/r1/spatial-record-v1.tsv");
const RECORD_GOLDEN_COUNT: usize = 11;

#[test]
fn canonical_spatial_records_transaction_and_checkpoint_match_goldens() {
    let expected: BTreeMap<_, _> = GOLDEN
        .lines()
        .skip(1)
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap();
            let length = fields.next().unwrap();
            let digest = fields.next().unwrap();
            assert!(fields.next().is_none());
            (name, format!("{length}\t{digest}"))
        })
        .collect();
    let cases = golden_cases();
    assert_eq!(expected.len(), cases.len());
    for (name, bytes) in cases {
        let digest = Sha256::digest(&bytes);
        let actual = format!("{}\t{}", bytes.len(), lower_hex(&digest));
        if std::env::var_os("USTE_PRINT_SPATIAL_GOLDENS").is_some() {
            println!("{name}\t{actual}");
            continue;
        }
        assert_eq!(
            expected.get(name),
            Some(&actual),
            "actual golden: {name}\t{actual}"
        );
    }
}

#[test]
fn every_golden_record_and_transaction_rejects_truncation_and_trailing_bytes() {
    let cases = golden_cases();
    for (name, bytes) in cases.iter().take(RECORD_GOLDEN_COUNT) {
        for end in 0..bytes.len() {
            assert!(
                decode_record(&bytes[..end]).is_err(),
                "{name} accepted cut {end}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(
            decode_record(&trailing).is_err(),
            "{name} accepted trailing data"
        );
    }
    let (name, transaction) = &cases[RECORD_GOLDEN_COUNT];
    assert_eq!(*name, "transaction_world_root");
    for end in 0..transaction.len() {
        assert!(
            decode_transaction(&transaction[..end]).is_err(),
            "transaction accepted cut {end}"
        );
    }
    let mut trailing = transaction.clone();
    trailing.push(0);
    assert!(decode_transaction(&trailing).is_err());
}

fn golden_cases() -> Vec<(&'static str, Vec<u8>)> {
    let [world, root] = world_and_root();
    let local2 = geometry_record(
        20,
        reference(11, 1),
        Geometry::LocalPoint2(LocalPoint2 {
            x: Nanometres::new(-3),
            y: Nanometres::new(4),
        }),
    );
    let local3 = geometry_record(
        21,
        reference(12, 1),
        Geometry::LocalPoint3(LocalPoint3 {
            x: Nanometres::new(-3),
            y: Nanometres::new(4),
            z: Nanometres::new(5),
        }),
    );
    let geographic = geometry_record(
        22,
        reference(13, 1),
        Geometry::GeographicPoint(GeographicPoint::new(
            LongitudeNanodegrees::new(179_000_000_000).unwrap(),
            LatitudeNanodegrees::new(-45_000_000_000).unwrap(),
            None,
        )),
    );
    let box2 = geometry_record(
        23,
        reference(11, 1),
        Geometry::LocalBox2(
            LocalBox2::new(
                LocalPoint2 {
                    x: Nanometres::new(-1),
                    y: Nanometres::new(-2),
                },
                LocalPoint2 {
                    x: Nanometres::new(3),
                    y: Nanometres::new(4),
                },
            )
            .unwrap(),
        ),
    );
    let box3 = geometry_record(
        24,
        reference(12, 1),
        Geometry::LocalBox3(
            LocalBox3::new(
                LocalPoint3 {
                    x: Nanometres::new(-1),
                    y: Nanometres::new(-2),
                    z: Nanometres::new(-3),
                },
                LocalPoint3 {
                    x: Nanometres::new(4),
                    y: Nanometres::new(5),
                    z: Nanometres::new(6),
                },
            )
            .unwrap(),
        ),
    );
    let geographic_box = geometry_record(
        25,
        reference(13, 1),
        Geometry::GeographicBox(
            GeographicBox::new(
                LongitudeNanodegrees::new(170_000_000_000).unwrap(),
                LongitudeNanodegrees::new(-170_000_000_000).unwrap(),
                LatitudeNanodegrees::new(-10_000_000_000).unwrap(),
                LatitudeNanodegrees::new(10_000_000_000).unwrap(),
            )
            .unwrap(),
        ),
    );
    let resolved = SpatialRecord::Observation(Box::new(observation(50, 51, point(0, -7))));
    let base = observation(52, 53, point(1, 2));
    let unresolved = SpatialRecord::Observation(Box::new(
        PositionObservation::new(
            base.id(),
            base.entity(),
            base.world(),
            base.frame(),
            base.key().clone(),
            base.evidence(),
            timestamp(base.evidence(), TimeInput::Missing),
            SpatialPosition::Geographic(GeographicPoint::new(
                LongitudeNanodegrees::new(0).unwrap(),
                LatitudeNanodegrees::new(90_000_000_000).unwrap(),
                Some(Nanometres::new(0)),
            )),
            PositionUncertainty::Radial(NonNegativeNanometres::new(9)),
            Some(record(50)),
        )
        .unwrap(),
    ));
    let transaction = SpatialTransaction::new(scope(), vec![world.clone(), root.clone()]).unwrap();
    let transaction_bytes = encode_transaction(&transaction).unwrap();
    let mut state = SpatialState::new(scope());
    let prepared = state
        .prepare(&transaction_bytes, None, revision(1))
        .unwrap();
    state.publish(prepared);

    vec![
        ("world", encode_record(&world).unwrap()),
        ("frame_root", encode_record(&root).unwrap()),
        (
            "frame_child",
            encode_record(&SpatialRecord::Frame(child_frame(14, 1, reference(11, 1)))).unwrap(),
        ),
        ("geometry_local_point2", encode_record(&local2).unwrap()),
        ("geometry_local_point3", encode_record(&local3).unwrap()),
        (
            "geometry_geographic_point",
            encode_record(&geographic).unwrap(),
        ),
        ("geometry_local_box2", encode_record(&box2).unwrap()),
        ("geometry_local_box3", encode_record(&box3).unwrap()),
        (
            "geometry_geographic_box",
            encode_record(&geographic_box).unwrap(),
        ),
        ("observation_resolved", encode_record(&resolved).unwrap()),
        (
            "observation_unresolved_corrected",
            encode_record(&unresolved).unwrap(),
        ),
        ("transaction_world_root", transaction_bytes),
        (
            "checkpoint_world_root",
            SpatialState::encode_checkpoint(&state.snapshot()).unwrap(),
        ),
    ]
}

fn geometry_record(
    id: u8,
    frame: uste_spatial::VersionedRecordRef,
    geometry: Geometry,
) -> SpatialRecord {
    SpatialRecord::Geometry(
        GeometryVersion::new(
            record(id),
            SpatialVersion::FIRST,
            record(10),
            frame,
            geometry,
            PositionUncertainty::Unknown,
            None,
        )
        .unwrap(),
    )
}

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use core::fmt::Write;
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}
