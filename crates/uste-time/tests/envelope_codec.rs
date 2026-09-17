use sha2::{Digest, Sha256};
use uste_time::{
    AuthorizedAssumption, ClockUncertainty, Resolution, SourceDescriptor, TZDB_PROFILE_DIGEST,
    TimeError, TimeInput, TimeNormalizer, TimestampRole, decode_envelope, encode_envelope,
    envelope_from_value, envelope_to_value,
};
use uste_types::{
    BoundedBytes, BoundedString, CanonicalMap, DatabaseId, NamespaceId, RecordId, RecordRef, Value,
    encode_value,
};

const GOLDENS: &str = include_str!("../../../acceptance/r1/time-envelope-v1.tsv");

fn record(byte: u8) -> RecordRef {
    RecordRef::new(
        DatabaseId::from_bytes([byte; 16]),
        NamespaceId::from_bytes([byte.wrapping_add(1); 16]),
        RecordId::from_bytes([byte.wrapping_add(2); 16]),
    )
}

fn envelope() -> uste_time::TimestampEnvelope {
    TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(record(1), 9, "asset:v9/json:/observed_at").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Local {
                text: "2026-11-01T01:30:00.120000000",
                zone: Some("America/Chicago"),
                offset_seconds: Some(-18_000),
                fold: Some(uste_time::FoldChoice::Earlier),
            },
            ClockUncertainty::BoundedNanoseconds {
                before: 2_000,
                after: 3_000,
            },
            vec![AuthorizedAssumption::new("source column uses venue zone", record(8)).unwrap()],
        )
        .unwrap()
}

#[test]
fn canonical_envelope_round_trips_and_every_truncation_fails() {
    let envelope = envelope();
    let encoded = encode_envelope(&envelope).unwrap();
    assert_eq!(decode_envelope(&encoded).unwrap(), envelope);
    assert_eq!(
        encode_envelope(&decode_envelope(&encoded).unwrap()).unwrap(),
        encoded
    );
    for cut in 0..encoded.len() {
        assert!(
            decode_envelope(&encoded[..cut]).is_err(),
            "accepted cut {cut}"
        );
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(decode_envelope(&trailing), Err(TimeError::InvalidEncoding));

    // Golden digest binds the complete canonical bytes without depending on debug formatting.
    let digest: [u8; 32] = Sha256::digest(&encoded).into();
    assert_eq!(encoded.len(), 849);
    assert_eq!(
        digest,
        [
            0x52, 0x92, 0x00, 0x90, 0x90, 0x0e, 0x58, 0xdf, 0x57, 0xda, 0x9f, 0xb8, 0x16, 0xa0,
            0x32, 0xb7, 0x6c, 0x3a, 0x9d, 0x6c, 0x22, 0x7f, 0x94, 0xa9, 0xc9, 0x60, 0x57, 0x90,
            0x92, 0x3f, 0x5c, 0xc9,
        ]
    );
    let rows: Vec<Vec<&str>> = GOLDENS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows[0][0], "fold_envelope");
    assert_eq!(rows[0][1], "uste-time-envelope-v1");
    assert_eq!(rows[0][2].parse::<usize>().unwrap(), encoded.len());
    assert_eq!(rows[0][3], hex(&digest));
    assert_eq!(rows[1][0], "tzdb");
    assert_eq!(rows[1][2], "bundled-2026c");
    assert_eq!(rows[1][3], hex(&TZDB_PROFILE_DIGEST));
}

#[test]
fn replay_decode_uses_stored_pair_and_does_not_reparse_original() {
    let original = envelope();
    let accepted = original.resolution().instant().unwrap();
    let mut value = envelope_to_value(&original).unwrap();
    replace_root(
        &mut value,
        "original",
        Value::string("not-a-time".to_owned()).unwrap(),
    );
    let encoded = encode_value(&value).unwrap();
    let replayed = decode_envelope(&encoded).unwrap();
    assert_eq!(replayed.original(), "not-a-time");
    assert_eq!(replayed.resolution().instant(), Some(accepted));
    assert_eq!(replayed.status(), uste_time::ResolutionStatus::Resolved);
}

#[test]
fn unknown_fields_profiles_and_inconsistent_status_are_rejected() {
    let valid = envelope_to_value(&envelope()).unwrap();

    let mut extra = valid.clone();
    insert_root(&mut extra, "future_required", Value::Bool(true));
    assert_eq!(envelope_from_value(extra), Err(TimeError::InvalidEnvelope));

    let mut missing = valid.clone();
    remove_root(&mut missing, "resolution");
    assert_eq!(
        envelope_from_value(missing),
        Err(TimeError::InvalidEnvelope)
    );

    let mut profile = valid.clone();
    replace_root(
        &mut profile,
        "normalization_profile",
        Value::string("future-profile".to_owned()).unwrap(),
    );
    assert_eq!(
        envelope_from_value(profile),
        Err(TimeError::UnsupportedProfile)
    );

    let mut bad_tz = valid.clone();
    replace_nested(
        &mut bad_tz,
        "timezone_profile",
        "digest",
        Value::Bytes(BoundedBytes::new(vec![0; TZDB_PROFILE_DIGEST.len()]).unwrap()),
    );
    assert_eq!(
        envelope_from_value(bad_tz),
        Err(TimeError::UnsupportedProfile)
    );

    let mut inconsistent = valid;
    replace_nested(
        &mut inconsistent,
        "resolution",
        "status",
        Value::string("invalid".to_owned()).unwrap(),
    );
    assert_eq!(
        envelope_from_value(inconsistent),
        Err(TimeError::InvalidEnvelope)
    );

    let mut offset_conflict = envelope_to_value(&envelope()).unwrap();
    replace_nested(
        &mut offset_conflict,
        "interpretation",
        "supplied_offset_seconds",
        Value::Signed(-21_600),
    );
    assert_eq!(
        envelope_from_value(offset_conflict),
        Err(TimeError::InvalidEnvelope)
    );

    let mut resolved_date = envelope_to_value(&envelope()).unwrap();
    replace_nested(
        &mut resolved_date,
        "precision",
        "kind",
        Value::string("date".to_owned()).unwrap(),
    );
    replace_nested(&mut resolved_date, "precision", "detail", Value::Null);
    assert_eq!(
        envelope_from_value(resolved_date),
        Err(TimeError::InvalidEnvelope)
    );

    let numeric = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(record(1), 1, "numeric").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Numeric {
                token: "0",
                value: 0,
                unit: Some(uste_time::EpochUnit::Seconds),
                scale: uste_time::TimeScale::PosixUtc,
            },
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    let mut numeric_offset = envelope_to_value(&numeric).unwrap();
    replace_nested(
        &mut numeric_offset,
        "resolution",
        "effective_offset_seconds",
        Value::Signed(60),
    );
    assert_eq!(
        envelope_from_value(numeric_offset),
        Err(TimeError::InvalidEnvelope)
    );

    let mut false_scale = envelope_to_value(&numeric).unwrap();
    replace_nested(&mut false_scale, "resolution", "instant", Value::Null);
    replace_nested(
        &mut false_scale,
        "resolution",
        "effective_offset_seconds",
        Value::Null,
    );
    replace_nested(
        &mut false_scale,
        "resolution",
        "status",
        Value::string("unsupported".to_owned()).unwrap(),
    );
    replace_nested(
        &mut false_scale,
        "resolution",
        "reason",
        Value::string("unsupported_time_scale".to_owned()).unwrap(),
    );
    assert_eq!(
        envelope_from_value(false_scale),
        Err(TimeError::InvalidEnvelope)
    );

    let explicit = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(record(1), 1, "explicit").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Rfc3339("2026-09-16T14:00:00Z"),
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    let mut naive_resolved = envelope_to_value(&explicit).unwrap();
    replace_nested(
        &mut naive_resolved,
        "interpretation",
        "supplied_offset_seconds",
        Value::Null,
    );
    assert_eq!(
        envelope_from_value(naive_resolved),
        Err(TimeError::InvalidEnvelope)
    );

    let missing_zone = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(record(1), 1, "local").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Local {
                text: "2026-09-16T09:00:00",
                zone: None,
                offset_seconds: None,
                fold: None,
            },
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    let mut false_missing_zone = envelope_to_value(&missing_zone).unwrap();
    replace_nested(
        &mut false_missing_zone,
        "interpretation",
        "zone",
        Value::string("America/Chicago".to_owned()).unwrap(),
    );
    replace_root(
        &mut false_missing_zone,
        "timezone_profile",
        child(&envelope_to_value(&envelope()).unwrap(), "timezone_profile"),
    );
    assert_eq!(
        envelope_from_value(false_missing_zone),
        Err(TimeError::InvalidEnvelope)
    );

    let mut false_conflict = envelope_to_value(&missing_zone).unwrap();
    replace_nested(
        &mut false_conflict,
        "resolution",
        "status",
        Value::string("invalid".to_owned()).unwrap(),
    );
    replace_nested(
        &mut false_conflict,
        "resolution",
        "reason",
        Value::string("offset_zone_conflict".to_owned()).unwrap(),
    );
    assert_eq!(
        envelope_from_value(false_conflict),
        Err(TimeError::InvalidEnvelope)
    );
}

#[test]
fn envelope_bounds_are_enforced_before_canonical_allocation() {
    let oversized_locator = "x".repeat(4_097);
    assert!(matches!(
        SourceDescriptor::new(record(1), 1, oversized_locator.as_str()),
        Err(TimeError::FieldTooLarge {
            field: "locator",
            ..
        })
    ));
    let oversized_assumption = "x".repeat(1_025);
    assert!(matches!(
        AuthorizedAssumption::new(oversized_assumption.as_str(), record(2)),
        Err(TimeError::FieldTooLarge {
            field: "assumption",
            ..
        })
    ));
    assert!(matches!(
        TimeNormalizer::posix_utc_v1().unwrap().normalize(
            SourceDescriptor::new(record(1), 1, "field").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Rfc3339(&"x".repeat(4_097)),
            ClockUncertainty::Unknown,
            Vec::new(),
        ),
        Err(TimeError::FieldTooLarge {
            field: "original",
            ..
        })
    ));
    assert!(matches!(
        TimeNormalizer::posix_utc_v1().unwrap().normalize(
            SourceDescriptor::new(record(1), 1, "field").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Local {
                text: "2026-01-01T00:00:00",
                zone: Some(&"x".repeat(256)),
                offset_seconds: None,
                fold: None,
            },
            ClockUncertainty::Unknown,
            Vec::new(),
        ),
        Err(TimeError::FieldTooLarge { field: "zone", .. })
    ));
}

fn child(value: &Value, key: &str) -> Value {
    let Value::Map(map) = value else {
        panic!("map")
    };
    map.as_slice()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == key)
        .map(|(_, child)| child.clone())
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn replace_nested(value: &mut Value, parent: &str, key: &str, replacement: Value) {
    let mut nested = child(value, parent);
    replace_root(&mut nested, key, replacement);
    replace_root(value, parent, nested);
}

fn replace_root(value: &mut Value, key: &str, replacement: Value) {
    let Value::Map(map) = value else {
        panic!("map")
    };
    let mut entries = map.clone().into_vec();
    let entry = entries
        .iter_mut()
        .find(|(candidate, _)| candidate.as_str() == key)
        .unwrap();
    entry.1 = replacement;
    *value = Value::Map(CanonicalMap::new(entries).unwrap());
}

fn insert_root(value: &mut Value, key: &str, child: Value) {
    let Value::Map(map) = value else {
        panic!("map")
    };
    let mut entries = map.clone().into_vec();
    entries.push((BoundedString::new(key.to_owned()).unwrap(), child));
    *value = Value::Map(CanonicalMap::new(entries).unwrap());
}

fn remove_root(value: &mut Value, key: &str) {
    let Value::Map(map) = value else {
        panic!("map")
    };
    let mut entries = map.clone().into_vec();
    entries.retain(|(candidate, _)| candidate.as_str() != key);
    *value = Value::Map(CanonicalMap::new(entries).unwrap());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn unresolved_envelopes_also_round_trip_without_an_instant() {
    let unresolved = TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            SourceDescriptor::new(record(1), 1, "field").unwrap(),
            TimestampRole::SourceEvent,
            TimeInput::Missing,
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap();
    assert_eq!(unresolved.resolution().instant(), None);
    assert!(matches!(unresolved.resolution(), Resolution::Missing(_)));
    assert_eq!(
        decode_envelope(&encode_envelope(&unresolved).unwrap()).unwrap(),
        unresolved
    );
    let mut false_missing = envelope_to_value(&unresolved).unwrap();
    replace_nested(
        &mut false_missing,
        "interpretation",
        "scale",
        Value::string("tai".to_owned()).unwrap(),
    );
    assert_eq!(
        envelope_from_value(false_missing),
        Err(TimeError::InvalidEnvelope)
    );
}
