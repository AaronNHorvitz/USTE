mod common;

use common::{geometry, observation, point, record, reference, timestamp, world_and_root};
use uste_spatial::{
    GeographicPoint, LatitudeNanodegrees, LongitudeNanodegrees, ObservationReferences,
    PositionKnowledge, SpatialError, SpatialPosition, SpatialRecord, decode_record, encode_record,
    record_from_value, record_to_value,
};
use uste_time::TimeInput;
use uste_types::{BoundedString, CanonicalMap, Value};

#[test]
fn every_spatial_record_round_trips_canonically() {
    let [world, root] = world_and_root();
    let records = [
        world,
        root,
        SpatialRecord::Geometry(geometry(20, reference(11, 1))),
        SpatialRecord::Observation(Box::new(observation(50, 51, point(0, 0)))),
    ];
    for record in records {
        let bytes = encode_record(&record).unwrap();
        let decoded = decode_record(&bytes).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(encode_record(&decoded).unwrap(), bytes);
    }
}

#[test]
fn every_observation_truncation_and_trailing_byte_fails() {
    let bytes = encode_record(&SpatialRecord::Observation(Box::new(observation(
        50,
        51,
        point(1, -1),
    ))))
    .unwrap();
    for end in 0..bytes.len() {
        assert!(decode_record(&bytes[..end]).is_err(), "accepted cut {end}");
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert_eq!(decode_record(&trailing), Err(SpatialError::InvalidEncoding));
}

#[test]
fn closed_root_schema_rejects_unknown_fields_profiles_and_versions() {
    let record = SpatialRecord::Geometry(geometry(20, reference(11, 1)));
    let Value::Map(root) = record_to_value(&record).unwrap() else {
        panic!("record root must be a map");
    };
    let mut entries = root.into_vec();
    entries.push((
        BoundedString::new("unexpected".to_owned()).unwrap(),
        Value::Null,
    ));
    assert_eq!(
        record_from_value(Value::Map(CanonicalMap::new(entries).unwrap())),
        Err(SpatialError::InvalidEncoding)
    );

    for (field, replacement, expected) in [
        (
            "profile",
            Value::string("uste-spatial-record-v2".to_owned()).unwrap(),
            SpatialError::UnsupportedProfile,
        ),
        (
            "schema_version",
            Value::Unsigned(2),
            SpatialError::UnsupportedProfile,
        ),
    ] {
        let Value::Map(root) = record_to_value(&record).unwrap() else {
            unreachable!();
        };
        let entries = root
            .into_vec()
            .into_iter()
            .map(|(key, value)| {
                if key.as_str() == field {
                    (key, replacement.clone())
                } else {
                    (key, value)
                }
            })
            .collect();
        assert_eq!(
            record_from_value(Value::Map(CanonicalMap::new(entries).unwrap())),
            Err(expected)
        );
    }
}

#[test]
fn unresolved_source_time_round_trips_without_inventing_an_instant() {
    let mut value = observation(50, 51, point(0, 0));
    let evidence = record(40);
    value = uste_spatial::PositionObservation::new(
        value.id(),
        value.entity(),
        value.world(),
        value.frame(),
        value.key().clone(),
        evidence,
        timestamp(evidence, TimeInput::Missing),
        value.position(),
        value.uncertainty(),
        None,
    )
    .unwrap();
    let decoded =
        decode_record(&encode_record(&SpatialRecord::Observation(Box::new(value))).unwrap())
            .unwrap();
    let SpatialRecord::Observation(decoded) = decoded else {
        unreachable!();
    };
    assert_eq!(decoded.source_time().resolution().instant(), None);
}

#[test]
fn unknown_position_has_no_default_origin_and_zero_is_valid_observed_data() {
    assert!(PositionKnowledge::Unknown.is_unknown());
    let zero = observation(50, 51, point(0, 0));
    assert_eq!(zero.position(), point(0, 0));
    assert_eq!(
        ObservationReferences::new(vec![record(1), record(1)]),
        Err(SpatialError::InvalidObservation)
    );
}

#[test]
fn noncanonical_longitude_aliases_are_rejected_on_decode() {
    let geographic = SpatialPosition::Geographic(GeographicPoint::new(
        LongitudeNanodegrees::new(0).unwrap(),
        LatitudeNanodegrees::new(0).unwrap(),
        None,
    ));
    let record = SpatialRecord::Observation(Box::new(observation(50, 51, geographic)));
    let root = record_to_value(&record).unwrap();
    let body = map_field(&root, "body");
    let position = map_field(&body, "position");
    let bad_position = replace_map_field(position, "longitude", Value::Signed(180_000_000_000));
    let bad_body = replace_map_field(body, "position", bad_position);
    assert_eq!(
        record_from_value(replace_map_field(root, "body", bad_body)),
        Err(SpatialError::InvalidEncoding)
    );

    let pole = SpatialPosition::Geographic(GeographicPoint::new(
        LongitudeNanodegrees::new(0).unwrap(),
        LatitudeNanodegrees::new(90_000_000_000).unwrap(),
        None,
    ));
    let root = record_to_value(&SpatialRecord::Observation(Box::new(observation(
        50, 51, pole,
    ))))
    .unwrap();
    let body = map_field(&root, "body");
    let position = map_field(&body, "position");
    let bad_position = replace_map_field(position, "longitude", Value::Signed(42_000_000_000));
    let bad_body = replace_map_field(body, "position", bad_position);
    assert_eq!(
        record_from_value(replace_map_field(root, "body", bad_body)),
        Err(SpatialError::InvalidEncoding)
    );
}

fn map_field(value: &Value, name: &str) -> Value {
    let Value::Map(map) = value else {
        panic!("expected map")
    };
    map.as_slice()
        .iter()
        .find(|(key, _)| key.as_str() == name)
        .map(|(_, value)| value.clone())
        .unwrap()
}

fn replace_map_field(value: Value, name: &str, replacement: Value) -> Value {
    let Value::Map(map) = value else {
        panic!("expected map")
    };
    let entries = map
        .into_vec()
        .into_iter()
        .map(|(key, value)| {
            if key.as_str() == name {
                (key, replacement.clone())
            } else {
                (key, value)
            }
        })
        .collect();
    Value::Map(CanonicalMap::new(entries).unwrap())
}
