use uste_time::{
    ClockUncertainty, EpochUnit, FoldChoice, Resolution, ResolutionReason, ResolutionStatus,
    SourceDescriptor, SourcePrecision, TZDB_PROFILE_DIGEST, TZDB_VERSION, TimeInput,
    TimeNormalizer, TimeScale, TimestampRole, format_utc,
};
use uste_types::{DatabaseId, NamespaceId, RecordId, RecordRef, UtcInstant};

fn source() -> SourceDescriptor {
    SourceDescriptor::new(
        RecordRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
            RecordId::from_bytes([3; 16]),
        ),
        7,
        "row:42/field:event_time",
    )
    .unwrap()
}

fn normalize(input: TimeInput<'_>) -> uste_time::TimestampEnvelope {
    TimeNormalizer::posix_utc_v1()
        .unwrap()
        .normalize(
            source(),
            TimestampRole::SourceEvent,
            input,
            ClockUncertainty::Unknown,
            Vec::new(),
        )
        .unwrap()
}

#[test]
fn equivalent_offsets_negative_epochs_and_precision_are_exact() {
    let offset = normalize(TimeInput::Rfc3339("2026-09-16T09:00:00-05:00"));
    let utc = normalize(TimeInput::Rfc3339("2026-09-16T14:00:00Z"));
    assert_eq!(offset.resolution().instant(), utc.resolution().instant());
    assert_eq!(
        offset.resolution().instant(),
        UtcInstant::new(1_789_567_200, 0).ok()
    );

    let negative = normalize(TimeInput::Numeric {
        token: "-500",
        value: -500,
        unit: Some(EpochUnit::Milliseconds),
        scale: TimeScale::PosixUtc,
    });
    assert_eq!(
        negative.resolution().instant(),
        UtcInstant::new(-1, 500_000_000).ok()
    );
    assert_eq!(
        format_utc(negative.resolution().instant().unwrap()).unwrap(),
        "1969-12-31T23:59:59.5Z"
    );

    let padded = normalize(TimeInput::Rfc3339("2026-09-16T14:00:00.120000000Z"));
    assert_eq!(padded.precision(), SourcePrecision::FractionDigits(9));
    assert_eq!(
        format_utc(padded.resolution().instant().unwrap()).unwrap(),
        "2026-09-16T14:00:00.12Z"
    );
}

#[test]
fn strict_parser_rejects_guessing_and_preserves_semantic_failures() {
    for invalid in [
        "2026-09-16 14:00:00Z",
        "2026-09-16t14:00:00Z",
        "2026-09-16T14:00:00z",
        "2026-09-16T14:00:00.1234567890Z",
        "2026-02-29T00:00:00Z",
        "0000-01-01T00:00:00Z",
        "2026-09-16T24:00:00Z",
        "2026-09-16T14:00:00+24:00",
        "2026-99-99T99:99:60garbage",
        "2026-02-30T00:00:00-00:00",
        "1789567200",
    ] {
        let envelope = normalize(TimeInput::Rfc3339(invalid));
        assert_eq!(envelope.status(), ResolutionStatus::Invalid, "{invalid}");
        assert_eq!(envelope.original(), invalid);
        assert_eq!(envelope.resolution().instant(), None);
    }

    let unknown_offset = normalize(TimeInput::Rfc3339("2026-09-16T14:00:00-00:00"));
    assert_eq!(
        unknown_offset.resolution(),
        Resolution::Ambiguous(ResolutionReason::UnknownOffset)
    );
    let leap = normalize(TimeInput::Rfc3339("2016-12-31T23:59:60Z"));
    assert_eq!(
        leap.resolution(),
        Resolution::Unsupported(ResolutionReason::LeapSecond)
    );
    let naive = normalize(TimeInput::Local {
        text: "2026-09-16T09:00:00",
        zone: None,
        offset_seconds: None,
        fold: None,
    });
    assert_eq!(
        naive.resolution(),
        Resolution::Ambiguous(ResolutionReason::MissingZone)
    );
    let no_unit = normalize(TimeInput::Numeric {
        token: "1789567200",
        value: 1_789_567_200,
        unit: None,
        scale: TimeScale::PosixUtc,
    });
    assert_eq!(
        no_unit.resolution(),
        Resolution::Invalid(ResolutionReason::UnitRequired)
    );
}

#[test]
fn numeric_units_use_euclidean_floor_and_check_range() {
    for (unit, value) in [
        (EpochUnit::Seconds, -1_i128),
        (EpochUnit::Milliseconds, -1_000),
        (EpochUnit::Microseconds, -1_000_000),
        (EpochUnit::Nanoseconds, -1_000_000_000),
    ] {
        assert_eq!(
            normalize(TimeInput::Numeric {
                token: "-1s",
                value,
                unit: Some(unit),
                scale: TimeScale::PosixUtc,
            })
            .resolution()
            .instant(),
            UtcInstant::new(-1, 0).ok()
        );
    }
    assert_eq!(
        normalize(TimeInput::Numeric {
            token: "-1ns",
            value: -1,
            unit: Some(EpochUnit::Nanoseconds),
            scale: TimeScale::PosixUtc,
        })
        .resolution()
        .instant(),
        UtcInstant::new(-1, 999_999_999).ok()
    );
    assert_eq!(
        normalize(TimeInput::Numeric {
            token: "huge",
            value: i128::MAX,
            unit: Some(EpochUnit::Seconds),
            scale: TimeScale::PosixUtc,
        })
        .resolution(),
        Resolution::Invalid(ResolutionReason::OutOfRange)
    );
    for scale in [
        TimeScale::UtcLeapAware,
        TimeScale::Tai,
        TimeScale::Gps,
        TimeScale::Smeared,
        TimeScale::Unknown,
    ] {
        assert_eq!(
            normalize(TimeInput::Numeric {
                token: "0",
                value: 0,
                unit: Some(EpochUnit::Seconds),
                scale,
            })
            .resolution(),
            Resolution::Unsupported(ResolutionReason::UnsupportedTimeScale)
        );
    }
}

#[test]
fn chicago_fold_gap_and_offset_conflict_are_explicit() {
    let fold = |choice, offset| {
        normalize(TimeInput::Local {
            text: "2026-11-01T01:30:00",
            zone: Some("America/Chicago"),
            offset_seconds: offset,
            fold: choice,
        })
    };
    assert_eq!(
        fold(None, None).resolution(),
        Resolution::Ambiguous(ResolutionReason::FoldChoiceRequired)
    );
    let earlier = fold(Some(FoldChoice::Earlier), Some(-5 * 3_600));
    let later = fold(Some(FoldChoice::Later), Some(-6 * 3_600));
    assert_eq!(
        format_utc(earlier.resolution().instant().unwrap()).unwrap(),
        "2026-11-01T06:30:00Z"
    );
    assert_eq!(
        format_utc(later.resolution().instant().unwrap()).unwrap(),
        "2026-11-01T07:30:00Z"
    );
    assert_ne!(earlier.resolution().instant(), later.resolution().instant());
    assert_eq!(earlier.timezone_profile().unwrap().version, TZDB_VERSION);
    assert_eq!(
        earlier.timezone_profile().unwrap().digest,
        TZDB_PROFILE_DIGEST
    );
    assert_eq!(
        fold(Some(FoldChoice::Earlier), Some(-6 * 3_600)).resolution(),
        Resolution::Invalid(ResolutionReason::OffsetZoneConflict)
    );

    let gap = normalize(TimeInput::Local {
        text: "2026-03-08T02:30:00",
        zone: Some("America/Chicago"),
        offset_seconds: None,
        fold: None,
    });
    assert_eq!(
        gap.resolution(),
        Resolution::Invalid(ResolutionReason::NonexistentLocalTime)
    );
    let unknown = normalize(TimeInput::Local {
        text: "2026-01-01T00:00:00",
        zone: Some("Not/A_Zone"),
        offset_seconds: None,
        fold: None,
    });
    assert_eq!(
        unknown.resolution(),
        Resolution::Invalid(ResolutionReason::UnknownZone)
    );
}

#[test]
fn canonical_format_and_explicit_local_presentation_cover_boundaries() {
    assert_eq!(
        format_utc(UtcInstant::new(-62_135_596_800, 0).unwrap()).unwrap(),
        "0001-01-01T00:00:00Z"
    );
    assert_eq!(
        format_utc(UtcInstant::new(253_402_300_799, 999_999_999).unwrap()).unwrap(),
        "9999-12-31T23:59:59.999999999Z"
    );
    let normalizer = TimeNormalizer::posix_utc_v1().unwrap();
    let presentation = normalizer
        .present_in_zone(
            UtcInstant::new(1_789_567_200, 120_000_000).unwrap(),
            "america/chicago",
        )
        .unwrap();
    assert_eq!(
        presentation.text(),
        "2026-09-16T09:00:00.12-05:00[America/Chicago]"
    );
    assert_eq!(presentation.zone(), "America/Chicago");
    assert_eq!(presentation.offset_seconds(), -18_000);
    assert_eq!(presentation.timezone_profile().version, TZDB_VERSION);
    assert_eq!(presentation.format_profile(), "uste-local-presentation-v1");

    let final_day = normalizer
        .present_in_zone(
            UtcInstant::new(253_402_300_799, 999_999_999).unwrap(),
            "America/Chicago",
        )
        .unwrap();
    assert!(final_day.text().starts_with("9999-12-31T"));
    let first_day = normalizer
        .present_in_zone(UtcInstant::new(-62_135_596_800, 0).unwrap(), "Asia/Tokyo")
        .unwrap();
    assert!(first_day.text().starts_with("0001-01-01T"));
}

#[test]
fn missing_and_date_only_are_not_fabricated_instants() {
    let missing = normalize(TimeInput::Missing);
    assert_eq!(
        missing.resolution(),
        Resolution::Missing(ResolutionReason::MissingValue)
    );
    let date = normalize(TimeInput::DateOnly("2026-09-16"));
    assert_eq!(date.precision(), SourcePrecision::Date);
    assert_eq!(
        date.resolution(),
        Resolution::Ambiguous(ResolutionReason::DateOnlyWithoutPolicy)
    );
    for malformed in ["not-a-date", "2026-02-30", "2026-1-01", "0000-01-01"] {
        let date = normalize(TimeInput::DateOnly(malformed));
        assert_eq!(
            date.resolution(),
            Resolution::Invalid(ResolutionReason::InvalidSyntax),
            "{malformed}"
        );
        assert_eq!(date.original(), malformed);
    }
}

#[test]
fn invalid_separate_offsets_are_preserved_as_invalid_envelopes() {
    for zone in [None, Some("America/Chicago")] {
        let envelope = normalize(TimeInput::Local {
            text: "2026-09-16T09:00:00",
            zone,
            offset_seconds: Some(100_000),
            fold: None,
        });
        assert_eq!(
            envelope.resolution(),
            Resolution::Invalid(ResolutionReason::InvalidSyntax)
        );
        assert_eq!(
            envelope.interpretation().supplied_offset_seconds(),
            Some(100_000)
        );
        assert_eq!(envelope.timezone_profile().is_some(), zone.is_some());
    }
}
