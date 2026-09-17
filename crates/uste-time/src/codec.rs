use std::collections::BTreeMap;

use uste_types::{BoundedBytes, BoundedString, CanonicalMap, Value, decode_value, encode_value};

use crate::model::{
    AuthorizedAssumption, CALENDAR_PROFILE, ClockUncertainty, ENVELOPE_PROFILE, EpochUnit,
    FoldChoice, MAX_ASSUMPTIONS, NORMALIZATION_PROFILE, PARSER_PROFILE, Resolution,
    ResolutionReason, ResolutionStatus, SourceDescriptor, SourcePrecision, TZDB_PROFILE_DIGEST,
    TZDB_VERSION, TimeError, TimeInterpretation, TimeScale, TimestampEnvelope, TimestampRole,
    TimezoneProfile, bounded,
};

pub fn encode_envelope(envelope: &TimestampEnvelope) -> Result<Vec<u8>, TimeError> {
    encode_value(&envelope_to_value(envelope)?).map_err(|error| match error {
        uste_types::EncodeError::ResourceLimit => TimeError::ResourceLimit,
        _ => TimeError::InvalidEnvelope,
    })
}

/// Decode a stored accepted interpretation without parsing its original token or resolving a zone.
pub fn decode_envelope(input: &[u8]) -> Result<TimestampEnvelope, TimeError> {
    envelope_from_value(decode_value(input).map_err(|_| TimeError::InvalidEncoding)?)
}

pub fn envelope_to_value(envelope: &TimestampEnvelope) -> Result<Value, TimeError> {
    map([
        ("assumptions", encode_assumptions(envelope.assumptions())?),
        ("calendar_profile", text(CALENDAR_PROFILE)?),
        ("envelope_profile", text(ENVELOPE_PROFILE)?),
        (
            "interpretation",
            encode_interpretation(envelope.interpretation())?,
        ),
        ("normalization_profile", text(NORMALIZATION_PROFILE)?),
        ("original", text(envelope.original())?),
        ("parser_profile", text(PARSER_PROFILE)?),
        ("precision", encode_precision(envelope.precision())?),
        ("resolution", encode_resolution(envelope.resolution())?),
        ("role", text(role_name(envelope.role()))?),
        ("source", encode_source(envelope.source())?),
        (
            "timezone_profile",
            encode_timezone(envelope.timezone_profile())?,
        ),
        ("uncertainty", encode_uncertainty(envelope.uncertainty())?),
    ])
}

pub fn envelope_from_value(value: Value) -> Result<TimestampEnvelope, TimeError> {
    let mut root = Fields::new(value)?;
    expect_text(root.take("calendar_profile")?, CALENDAR_PROFILE)?;
    expect_text(root.take("envelope_profile")?, ENVELOPE_PROFILE)?;
    expect_text(root.take("normalization_profile")?, NORMALIZATION_PROFILE)?;
    expect_text(root.take("parser_profile")?, PARSER_PROFILE)?;
    let assumptions = decode_assumptions(root.take("assumptions")?)?;
    let interpretation = decode_interpretation(root.take("interpretation")?)?;
    let original = take_text(root.take("original")?)?;
    let precision = decode_precision(root.take("precision")?)?;
    let resolution = decode_resolution(root.take("resolution")?)?;
    let role = decode_role(&take_text(root.take("role")?)?)?;
    let source = decode_source(root.take("source")?)?;
    let timezone_profile = decode_timezone(root.take("timezone_profile")?)?;
    let uncertainty = decode_uncertainty(root.take("uncertainty")?)?;
    root.finish()?;
    TimestampEnvelope::checked(
        source,
        role,
        original,
        interpretation,
        resolution,
        precision,
        uncertainty,
        assumptions,
        timezone_profile,
    )
}

fn encode_source(source: &SourceDescriptor) -> Result<Value, TimeError> {
    map([
        ("artifact", Value::RecordRef(source.artifact())),
        (
            "artifact_version",
            Value::Unsigned(u128::from(source.artifact_version())),
        ),
        ("locator", text(source.locator())?),
    ])
}

fn decode_source(value: Value) -> Result<SourceDescriptor, TimeError> {
    let mut fields = Fields::new(value)?;
    let artifact = take_record(fields.take("artifact")?)?;
    let version = take_u64(fields.take("artifact_version")?)?;
    let locator = take_text(fields.take("locator")?)?;
    fields.finish()?;
    SourceDescriptor::new(artifact, version, locator)
}

fn encode_interpretation(interpretation: &TimeInterpretation) -> Result<Value, TimeError> {
    map([
        (
            "epoch_unit",
            optional_text(interpretation.epoch_unit().map(epoch_unit_name))?,
        ),
        ("fold", optional_text(interpretation.fold().map(fold_name))?),
        ("scale", text(scale_name(interpretation.scale()))?),
        (
            "supplied_offset_seconds",
            interpretation
                .supplied_offset_seconds()
                .map_or(Value::Null, |offset| Value::Signed(i128::from(offset))),
        ),
        ("zone", optional_text(interpretation.zone())?),
    ])
}

fn decode_interpretation(value: Value) -> Result<TimeInterpretation, TimeError> {
    let mut fields = Fields::new(value)?;
    let epoch_unit = take_optional_text(fields.take("epoch_unit")?)?
        .as_deref()
        .map(decode_epoch_unit)
        .transpose()?;
    let fold = take_optional_text(fields.take("fold")?)?
        .as_deref()
        .map(decode_fold)
        .transpose()?;
    let scale = decode_scale(&take_text(fields.take("scale")?)?)?;
    let supplied_offset_seconds = take_optional_i32(fields.take("supplied_offset_seconds")?)?;
    let zone = take_optional_text(fields.take("zone")?)?
        .map(|value| bounded(value, crate::MAX_ZONE_BYTES, "zone"))
        .transpose()?;
    fields.finish()?;
    Ok(TimeInterpretation {
        scale,
        epoch_unit,
        supplied_offset_seconds,
        zone,
        fold,
    })
}

fn encode_resolution(resolution: Resolution) -> Result<Value, TimeError> {
    map([
        (
            "effective_offset_seconds",
            resolution
                .effective_offset_seconds()
                .map_or(Value::Null, |offset| Value::Signed(i128::from(offset))),
        ),
        (
            "instant",
            resolution.instant().map_or(Value::Null, Value::Instant),
        ),
        ("reason", text(reason_name(resolution.reason()))?),
        ("status", text(status_name(resolution.status()))?),
    ])
}

fn decode_resolution(value: Value) -> Result<Resolution, TimeError> {
    let mut fields = Fields::new(value)?;
    let offset = take_optional_i32(fields.take("effective_offset_seconds")?)?;
    let instant = take_optional_instant(fields.take("instant")?)?;
    let reason = decode_reason(&take_text(fields.take("reason")?)?)?;
    let status = decode_status(&take_text(fields.take("status")?)?)?;
    fields.finish()?;
    match (status, reason, instant, offset) {
        (
            ResolutionStatus::Resolved,
            ResolutionReason::Accepted,
            Some(instant),
            Some(effective_offset_seconds),
        ) => Ok(Resolution::Resolved {
            instant,
            effective_offset_seconds,
        }),
        (ResolutionStatus::Missing, reason, None, None) => Ok(Resolution::Missing(reason)),
        (ResolutionStatus::Ambiguous, reason, None, None) => Ok(Resolution::Ambiguous(reason)),
        (ResolutionStatus::Invalid, reason, None, None) => Ok(Resolution::Invalid(reason)),
        (ResolutionStatus::Unsupported, reason, None, None) => Ok(Resolution::Unsupported(reason)),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn encode_precision(precision: SourcePrecision) -> Result<Value, TimeError> {
    let (kind, detail) = match precision {
        SourcePrecision::Unknown => ("unknown", Value::Null),
        SourcePrecision::Date => ("date", Value::Null),
        SourcePrecision::Second => ("second", Value::Null),
        SourcePrecision::FractionDigits(digits) => {
            ("fraction_digits", Value::Unsigned(u128::from(digits)))
        }
        SourcePrecision::EpochUnit(unit) => ("epoch_unit", text(epoch_unit_name(unit))?),
    };
    map([("detail", detail), ("kind", text(kind)?)])
}

fn decode_precision(value: Value) -> Result<SourcePrecision, TimeError> {
    let mut fields = Fields::new(value)?;
    let detail = fields.take("detail")?;
    let kind = take_text(fields.take("kind")?)?;
    fields.finish()?;
    match (kind.as_str(), detail) {
        ("unknown", Value::Null) => Ok(SourcePrecision::Unknown),
        ("date", Value::Null) => Ok(SourcePrecision::Date),
        ("second", Value::Null) => Ok(SourcePrecision::Second),
        ("fraction_digits", Value::Unsigned(digits)) => u8::try_from(digits)
            .map(SourcePrecision::FractionDigits)
            .map_err(|_| TimeError::InvalidEnvelope),
        ("epoch_unit", Value::String(unit)) => {
            decode_epoch_unit(unit.as_str()).map(SourcePrecision::EpochUnit)
        }
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn encode_uncertainty(uncertainty: ClockUncertainty) -> Result<Value, TimeError> {
    let (kind, before, after) = match uncertainty {
        ClockUncertainty::Unknown => ("unknown", Value::Null, Value::Null),
        ClockUncertainty::BoundedNanoseconds { before, after } => (
            "bounded_nanoseconds",
            Value::Unsigned(u128::from(before)),
            Value::Unsigned(u128::from(after)),
        ),
    };
    map([("after", after), ("before", before), ("kind", text(kind)?)])
}

fn decode_uncertainty(value: Value) -> Result<ClockUncertainty, TimeError> {
    let mut fields = Fields::new(value)?;
    let after = fields.take("after")?;
    let before = fields.take("before")?;
    let kind = take_text(fields.take("kind")?)?;
    fields.finish()?;
    match (kind.as_str(), before, after) {
        ("unknown", Value::Null, Value::Null) => Ok(ClockUncertainty::Unknown),
        ("bounded_nanoseconds", Value::Unsigned(before), Value::Unsigned(after)) => {
            Ok(ClockUncertainty::BoundedNanoseconds {
                before: u64::try_from(before).map_err(|_| TimeError::InvalidEnvelope)?,
                after: u64::try_from(after).map_err(|_| TimeError::InvalidEnvelope)?,
            })
        }
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn encode_assumptions(assumptions: &[AuthorizedAssumption]) -> Result<Value, TimeError> {
    let values = assumptions
        .iter()
        .map(|assumption| {
            map([
                ("authority", Value::RecordRef(assumption.authority())),
                ("statement", text(assumption.statement())?),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    Value::list(values).map_err(|_| TimeError::ResourceLimit)
}

fn decode_assumptions(value: Value) -> Result<Vec<AuthorizedAssumption>, TimeError> {
    let Value::List(values) = value else {
        return Err(TimeError::InvalidEnvelope);
    };
    if values.as_slice().len() > MAX_ASSUMPTIONS {
        return Err(TimeError::InvalidEnvelope);
    }
    values
        .into_vec()
        .into_iter()
        .map(|value| {
            let mut fields = Fields::new(value)?;
            let authority = take_record(fields.take("authority")?)?;
            let statement = take_text(fields.take("statement")?)?;
            fields.finish()?;
            AuthorizedAssumption::new(statement, authority)
        })
        .collect()
}

fn encode_timezone(profile: Option<TimezoneProfile>) -> Result<Value, TimeError> {
    match profile {
        None => Ok(Value::Null),
        Some(profile) if profile == TimezoneProfile::PINNED => map([
            (
                "digest",
                Value::Bytes(
                    BoundedBytes::new(profile.digest.to_vec())
                        .map_err(|_| TimeError::ResourceLimit)?,
                ),
            ),
            ("version", text(profile.version)?),
        ]),
        Some(_) => Err(TimeError::UnsupportedProfile),
    }
}

fn decode_timezone(value: Value) -> Result<Option<TimezoneProfile>, TimeError> {
    if value == Value::Null {
        return Ok(None);
    }
    let mut fields = Fields::new(value)?;
    let digest = take_bytes(fields.take("digest")?)?;
    let version = take_text(fields.take("version")?)?;
    fields.finish()?;
    if version != TZDB_VERSION || digest.as_slice() != TZDB_PROFILE_DIGEST {
        return Err(TimeError::UnsupportedProfile);
    }
    Ok(Some(TimezoneProfile::PINNED))
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Result<Value, TimeError> {
    let entries = entries
        .into_iter()
        .map(|(key, value)| {
            BoundedString::new(key.to_owned())
                .map(|key| (key, value))
                .map_err(|_| TimeError::ResourceLimit)
        })
        .collect::<Result<Vec<_>, _>>()?;
    CanonicalMap::new(entries)
        .map(Value::Map)
        .map_err(|_| TimeError::ResourceLimit)
}

fn text(value: &str) -> Result<Value, TimeError> {
    Value::string(value.to_owned()).map_err(|_| TimeError::ResourceLimit)
}

fn optional_text(value: Option<&str>) -> Result<Value, TimeError> {
    value.map_or(Ok(Value::Null), text)
}

struct Fields(BTreeMap<String, Value>);

impl Fields {
    fn new(value: Value) -> Result<Self, TimeError> {
        let Value::Map(map) = value else {
            return Err(TimeError::InvalidEnvelope);
        };
        Ok(Self(
            map.into_vec()
                .into_iter()
                .map(|(key, value)| (key.into_string(), value))
                .collect(),
        ))
    }

    fn take(&mut self, key: &str) -> Result<Value, TimeError> {
        self.0.remove(key).ok_or(TimeError::InvalidEnvelope)
    }

    fn finish(self) -> Result<(), TimeError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(TimeError::InvalidEnvelope)
        }
    }
}

fn take_text(value: Value) -> Result<String, TimeError> {
    let Value::String(value) = value else {
        return Err(TimeError::InvalidEnvelope);
    };
    Ok(value.into_string())
}

fn take_optional_text(value: Value) -> Result<Option<String>, TimeError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.into_string())),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn take_bytes(value: Value) -> Result<Vec<u8>, TimeError> {
    let Value::Bytes(value) = value else {
        return Err(TimeError::InvalidEnvelope);
    };
    Ok(value.into_vec())
}

fn take_record(value: Value) -> Result<uste_types::RecordRef, TimeError> {
    let Value::RecordRef(value) = value else {
        return Err(TimeError::InvalidEnvelope);
    };
    Ok(value)
}

fn take_u64(value: Value) -> Result<u64, TimeError> {
    let Value::Unsigned(value) = value else {
        return Err(TimeError::InvalidEnvelope);
    };
    u64::try_from(value).map_err(|_| TimeError::InvalidEnvelope)
}

fn take_optional_i32(value: Value) -> Result<Option<i32>, TimeError> {
    match value {
        Value::Null => Ok(None),
        Value::Signed(value) => i32::try_from(value)
            .map(Some)
            .map_err(|_| TimeError::InvalidEnvelope),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn take_optional_instant(value: Value) -> Result<Option<uste_types::UtcInstant>, TimeError> {
    match value {
        Value::Null => Ok(None),
        Value::Instant(value) => Ok(Some(value)),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn expect_text(value: Value, expected: &str) -> Result<(), TimeError> {
    if take_text(value)? == expected {
        Ok(())
    } else {
        Err(TimeError::UnsupportedProfile)
    }
}

fn role_name(value: TimestampRole) -> &'static str {
    match value {
        TimestampRole::SourceEvent => "source_event",
        TimestampRole::SourceCreated => "source_created",
        TimestampRole::SourceModified => "source_modified",
        TimestampRole::SourcePublished => "source_published",
        TimestampRole::Received => "received",
        TimestampRole::CommitObservation => "commit_observation",
        TimestampRole::DerivationAvailable => "derivation_available",
        TimestampRole::SimulationAnchor => "simulation_anchor",
    }
}

fn decode_role(value: &str) -> Result<TimestampRole, TimeError> {
    match value {
        "source_event" => Ok(TimestampRole::SourceEvent),
        "source_created" => Ok(TimestampRole::SourceCreated),
        "source_modified" => Ok(TimestampRole::SourceModified),
        "source_published" => Ok(TimestampRole::SourcePublished),
        "received" => Ok(TimestampRole::Received),
        "commit_observation" => Ok(TimestampRole::CommitObservation),
        "derivation_available" => Ok(TimestampRole::DerivationAvailable),
        "simulation_anchor" => Ok(TimestampRole::SimulationAnchor),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn epoch_unit_name(value: EpochUnit) -> &'static str {
    match value {
        EpochUnit::Seconds => "seconds",
        EpochUnit::Milliseconds => "milliseconds",
        EpochUnit::Microseconds => "microseconds",
        EpochUnit::Nanoseconds => "nanoseconds",
    }
}

fn decode_epoch_unit(value: &str) -> Result<EpochUnit, TimeError> {
    match value {
        "seconds" => Ok(EpochUnit::Seconds),
        "milliseconds" => Ok(EpochUnit::Milliseconds),
        "microseconds" => Ok(EpochUnit::Microseconds),
        "nanoseconds" => Ok(EpochUnit::Nanoseconds),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn scale_name(value: TimeScale) -> &'static str {
    match value {
        TimeScale::PosixUtc => "posix_utc",
        TimeScale::UtcLeapAware => "utc_leap_aware",
        TimeScale::Tai => "tai",
        TimeScale::Gps => "gps",
        TimeScale::Smeared => "smeared",
        TimeScale::Unknown => "unknown",
    }
}

fn decode_scale(value: &str) -> Result<TimeScale, TimeError> {
    match value {
        "posix_utc" => Ok(TimeScale::PosixUtc),
        "utc_leap_aware" => Ok(TimeScale::UtcLeapAware),
        "tai" => Ok(TimeScale::Tai),
        "gps" => Ok(TimeScale::Gps),
        "smeared" => Ok(TimeScale::Smeared),
        "unknown" => Ok(TimeScale::Unknown),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn fold_name(value: FoldChoice) -> &'static str {
    match value {
        FoldChoice::Earlier => "earlier",
        FoldChoice::Later => "later",
    }
}

fn decode_fold(value: &str) -> Result<FoldChoice, TimeError> {
    match value {
        "earlier" => Ok(FoldChoice::Earlier),
        "later" => Ok(FoldChoice::Later),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn status_name(value: ResolutionStatus) -> &'static str {
    match value {
        ResolutionStatus::Resolved => "resolved",
        ResolutionStatus::Missing => "missing",
        ResolutionStatus::Ambiguous => "ambiguous",
        ResolutionStatus::Invalid => "invalid",
        ResolutionStatus::Unsupported => "unsupported",
    }
}

fn decode_status(value: &str) -> Result<ResolutionStatus, TimeError> {
    match value {
        "resolved" => Ok(ResolutionStatus::Resolved),
        "missing" => Ok(ResolutionStatus::Missing),
        "ambiguous" => Ok(ResolutionStatus::Ambiguous),
        "invalid" => Ok(ResolutionStatus::Invalid),
        "unsupported" => Ok(ResolutionStatus::Unsupported),
        _ => Err(TimeError::InvalidEnvelope),
    }
}

fn reason_name(value: ResolutionReason) -> &'static str {
    match value {
        ResolutionReason::Accepted => "accepted",
        ResolutionReason::MissingValue => "missing_value",
        ResolutionReason::MissingZone => "missing_zone",
        ResolutionReason::DateOnlyWithoutPolicy => "date_only_without_policy",
        ResolutionReason::UnitRequired => "unit_required",
        ResolutionReason::InvalidSyntax => "invalid_syntax",
        ResolutionReason::OutOfRange => "out_of_range",
        ResolutionReason::UnknownOffset => "unknown_offset",
        ResolutionReason::LeapSecond => "leap_second",
        ResolutionReason::UnsupportedTimeScale => "unsupported_time_scale",
        ResolutionReason::UnknownZone => "unknown_zone",
        ResolutionReason::FoldChoiceRequired => "fold_choice_required",
        ResolutionReason::UnnecessaryFoldChoice => "unnecessary_fold_choice",
        ResolutionReason::NonexistentLocalTime => "nonexistent_local_time",
        ResolutionReason::OffsetZoneConflict => "offset_zone_conflict",
    }
}

fn decode_reason(value: &str) -> Result<ResolutionReason, TimeError> {
    match value {
        "accepted" => Ok(ResolutionReason::Accepted),
        "missing_value" => Ok(ResolutionReason::MissingValue),
        "missing_zone" => Ok(ResolutionReason::MissingZone),
        "date_only_without_policy" => Ok(ResolutionReason::DateOnlyWithoutPolicy),
        "unit_required" => Ok(ResolutionReason::UnitRequired),
        "invalid_syntax" => Ok(ResolutionReason::InvalidSyntax),
        "out_of_range" => Ok(ResolutionReason::OutOfRange),
        "unknown_offset" => Ok(ResolutionReason::UnknownOffset),
        "leap_second" => Ok(ResolutionReason::LeapSecond),
        "unsupported_time_scale" => Ok(ResolutionReason::UnsupportedTimeScale),
        "unknown_zone" => Ok(ResolutionReason::UnknownZone),
        "fold_choice_required" => Ok(ResolutionReason::FoldChoiceRequired),
        "unnecessary_fold_choice" => Ok(ResolutionReason::UnnecessaryFoldChoice),
        "nonexistent_local_time" => Ok(ResolutionReason::NonexistentLocalTime),
        "offset_zone_conflict" => Ok(ResolutionReason::OffsetZoneConflict),
        _ => Err(TimeError::InvalidEnvelope),
    }
}
