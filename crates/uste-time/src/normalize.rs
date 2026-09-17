use std::sync::OnceLock;

use jiff::{
    civil::DateTime,
    tz::{AmbiguousOffset, TimeZone},
};
use sha2::{Digest, Sha256};
use uste_types::UtcInstant;

use crate::model::{
    AuthorizedAssumption, ClockUncertainty, EpochUnit, FoldChoice, LocalPresentation,
    MAX_ASSUMPTIONS, MAX_ORIGINAL_BYTES, MAX_ZONE_BYTES, Resolution, ResolutionReason,
    SourceDescriptor, SourcePrecision, TZDB_PROFILE_DIGEST, TZDB_VERSION, TimeError, TimeInput,
    TimeInterpretation, TimeScale, TimestampEnvelope, TimestampRole, TimezoneProfile, bounded,
    valid_offset,
};

static PROFILE_VERIFIED: OnceLock<bool> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub struct TimeNormalizer {
    _verified_profile: (),
}

impl TimeNormalizer {
    /// Open the accepted normalizer only if its embedded rules match the pinned profile.
    pub fn posix_utc_v1() -> Result<Self, TimeError> {
        let verified = PROFILE_VERIFIED.get_or_init(|| {
            jiff_tzdb::VERSION == Some(TZDB_VERSION)
                && embedded_tzdb_digest().is_some_and(|digest| digest == TZDB_PROFILE_DIGEST)
        });
        if *verified {
            Ok(Self {
                _verified_profile: (),
            })
        } else {
            Err(TimeError::PinnedProfileUnavailable)
        }
    }

    pub fn normalize(
        &self,
        source: SourceDescriptor,
        role: TimestampRole,
        input: TimeInput<'_>,
        uncertainty: ClockUncertainty,
        assumptions: Vec<AuthorizedAssumption>,
    ) -> Result<TimestampEnvelope, TimeError> {
        if assumptions.len() > MAX_ASSUMPTIONS {
            return Err(TimeError::FieldTooLarge {
                field: "assumptions",
                actual: assumptions.len(),
                maximum: MAX_ASSUMPTIONS,
            });
        }
        match input {
            TimeInput::Missing => make_envelope(
                source,
                role,
                String::new(),
                interpretation(TimeScale::Unknown, None, None, None, None)?,
                Resolution::Missing(ResolutionReason::MissingValue),
                SourcePrecision::Unknown,
                uncertainty,
                assumptions,
                None,
            ),
            TimeInput::Rfc3339(original) => {
                preflight_original(original)?;
                self.normalize_rfc3339(source, role, original, uncertainty, assumptions)
            }
            TimeInput::Numeric {
                token,
                value,
                unit,
                scale,
            } => {
                preflight_original(token)?;
                self.normalize_numeric(
                    source,
                    role,
                    token,
                    value,
                    unit,
                    scale,
                    uncertainty,
                    assumptions,
                )
            }
            TimeInput::Local {
                text,
                zone,
                offset_seconds,
                fold,
            } => {
                preflight_original(text)?;
                self.normalize_local(
                    source,
                    role,
                    text,
                    zone,
                    offset_seconds,
                    fold,
                    uncertainty,
                    assumptions,
                )
            }
            TimeInput::DateOnly(original) => {
                preflight_original(original)?;
                let resolution = if valid_date_only(original) {
                    Resolution::Ambiguous(ResolutionReason::DateOnlyWithoutPolicy)
                } else {
                    Resolution::Invalid(ResolutionReason::InvalidSyntax)
                };
                make_envelope(
                    source,
                    role,
                    original.to_owned(),
                    interpretation(TimeScale::PosixUtc, None, None, None, None)?,
                    resolution,
                    SourcePrecision::Date,
                    uncertainty,
                    assumptions,
                    None,
                )
            }
        }
    }

    fn normalize_rfc3339(
        &self,
        source: SourceDescriptor,
        role: TimestampRole,
        original: &str,
        uncertainty: ClockUncertainty,
        assumptions: Vec<AuthorizedAssumption>,
    ) -> Result<TimestampEnvelope, TimeError> {
        let parsed = match parse_datetime(original, true) {
            Ok(parsed) => parsed,
            Err(failure) => {
                return make_envelope(
                    source,
                    role,
                    original.to_owned(),
                    interpretation(TimeScale::PosixUtc, None, failure.offset, None, None)?,
                    failure.resolution,
                    failure.precision,
                    uncertainty,
                    assumptions,
                    None,
                );
            }
        };
        let offset = parsed.offset.expect("RFC 3339 parser requires an offset");
        let resolution = timestamp_from_offset(parsed.datetime, offset)
            .map(|instant| Resolution::Resolved {
                instant,
                effective_offset_seconds: offset,
            })
            .unwrap_or(Resolution::Invalid(ResolutionReason::OutOfRange));
        make_envelope(
            source,
            role,
            original.to_owned(),
            interpretation(TimeScale::PosixUtc, None, Some(offset), None, None)?,
            resolution,
            parsed.precision,
            uncertainty,
            assumptions,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn normalize_numeric(
        &self,
        source: SourceDescriptor,
        role: TimestampRole,
        token: &str,
        value: i128,
        unit: Option<EpochUnit>,
        scale: TimeScale,
        uncertainty: ClockUncertainty,
        assumptions: Vec<AuthorizedAssumption>,
    ) -> Result<TimestampEnvelope, TimeError> {
        let resolution = if scale != TimeScale::PosixUtc {
            Resolution::Unsupported(ResolutionReason::UnsupportedTimeScale)
        } else if let Some(unit) = unit {
            numeric_instant(value, unit)
                .map(|instant| Resolution::Resolved {
                    instant,
                    effective_offset_seconds: 0,
                })
                .unwrap_or(Resolution::Invalid(ResolutionReason::OutOfRange))
        } else {
            Resolution::Invalid(ResolutionReason::UnitRequired)
        };
        make_envelope(
            source,
            role,
            token.to_owned(),
            interpretation(scale, unit, None, None, None)?,
            resolution,
            unit.map_or(SourcePrecision::Unknown, SourcePrecision::EpochUnit),
            uncertainty,
            assumptions,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn normalize_local(
        &self,
        source: SourceDescriptor,
        role: TimestampRole,
        original: &str,
        zone: Option<&str>,
        supplied_offset: Option<i32>,
        fold: Option<FoldChoice>,
        uncertainty: ClockUncertainty,
        assumptions: Vec<AuthorizedAssumption>,
    ) -> Result<TimestampEnvelope, TimeError> {
        if let Some(zone) = zone
            && zone.len() > MAX_ZONE_BYTES
        {
            return Err(TimeError::FieldTooLarge {
                field: "zone",
                actual: zone.len(),
                maximum: MAX_ZONE_BYTES,
            });
        }
        let zone_value = zone
            .map(|value| bounded(value.to_owned(), MAX_ZONE_BYTES, "zone"))
            .transpose()?;
        let base_interpretation = || {
            interpretation(
                TimeScale::PosixUtc,
                None,
                supplied_offset,
                zone_value.clone(),
                fold,
            )
        };
        if supplied_offset.is_some_and(|offset| !valid_offset(offset)) {
            return make_envelope(
                source,
                role,
                original.to_owned(),
                base_interpretation()?,
                Resolution::Invalid(ResolutionReason::InvalidSyntax),
                SourcePrecision::Unknown,
                uncertainty,
                assumptions,
                zone.map(|_| TimezoneProfile::PINNED),
            );
        }
        let parsed = match parse_datetime(original, false) {
            Ok(parsed) => parsed,
            Err(failure) => {
                return make_envelope(
                    source,
                    role,
                    original.to_owned(),
                    base_interpretation()?,
                    failure.resolution,
                    failure.precision,
                    uncertainty,
                    assumptions,
                    zone.map(|_| TimezoneProfile::PINNED),
                );
            }
        };
        let (resolution, timezone_profile) = match zone {
            None => match (supplied_offset, fold) {
                (Some(offset), None) => (
                    timestamp_from_offset(parsed.datetime, offset)
                        .map(|instant| Resolution::Resolved {
                            instant,
                            effective_offset_seconds: offset,
                        })
                        .unwrap_or(Resolution::Invalid(ResolutionReason::OutOfRange)),
                    None,
                ),
                (Some(_), Some(_)) | (None, Some(_)) => (
                    Resolution::Invalid(ResolutionReason::UnnecessaryFoldChoice),
                    None,
                ),
                (None, None) => (Resolution::Ambiguous(ResolutionReason::MissingZone), None),
            },
            Some(name) => (
                resolve_named_zone(parsed.datetime, name, supplied_offset, fold),
                Some(TimezoneProfile::PINNED),
            ),
        };
        make_envelope(
            source,
            role,
            original.to_owned(),
            base_interpretation()?,
            resolution,
            parsed.precision,
            uncertainty,
            assumptions,
            timezone_profile,
        )
    }

    /// Render an instant in an explicitly selected embedded zone.
    pub fn present_in_zone(
        &self,
        instant: UtcInstant,
        zone: &str,
    ) -> Result<LocalPresentation, TimeError> {
        if zone.len() > MAX_ZONE_BYTES {
            return Err(TimeError::FieldTooLarge {
                field: "zone",
                actual: zone.len(),
                maximum: MAX_ZONE_BYTES,
            });
        }
        let (canonical, bytes) = jiff_tzdb::get(zone).ok_or(TimeError::InvalidEnvelope)?;
        let timezone = TimeZone::tzif(canonical, bytes).map_err(|_| TimeError::InvalidEnvelope)?;
        let (datetime, offset) =
            local_datetime_for_instant(&timezone, instant).ok_or(TimeError::InvalidEnvelope)?;
        let text = format!(
            "{}{}[{}]",
            format_datetime(datetime),
            format_offset(offset),
            canonical
        );
        Ok(LocalPresentation::new(text, canonical.to_owned(), offset))
    }
}

pub fn format_utc(instant: UtcInstant) -> Result<String, TimeError> {
    let (year, month, day, hour, minute, second) = civil_from_epoch(instant.seconds());
    let mut result = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if instant.nanoseconds() != 0 {
        let mut fraction = format!("{:09}", instant.nanoseconds());
        while fraction.ends_with('0') {
            fraction.pop();
        }
        result.push('.');
        result.push_str(&fraction);
    }
    result.push('Z');
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn make_envelope(
    source: SourceDescriptor,
    role: TimestampRole,
    original: String,
    interpretation: TimeInterpretation,
    resolution: Resolution,
    precision: SourcePrecision,
    uncertainty: ClockUncertainty,
    assumptions: Vec<AuthorizedAssumption>,
    timezone_profile: Option<TimezoneProfile>,
) -> Result<TimestampEnvelope, TimeError> {
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

fn interpretation(
    scale: TimeScale,
    epoch_unit: Option<EpochUnit>,
    supplied_offset_seconds: Option<i32>,
    zone: Option<uste_types::BoundedString>,
    fold: Option<FoldChoice>,
) -> Result<TimeInterpretation, TimeError> {
    Ok(TimeInterpretation {
        scale,
        epoch_unit,
        supplied_offset_seconds,
        zone,
        fold,
    })
}

fn preflight_original(value: &str) -> Result<(), TimeError> {
    if value.len() > MAX_ORIGINAL_BYTES {
        return Err(TimeError::FieldTooLarge {
            field: "original",
            actual: value.len(),
            maximum: MAX_ORIGINAL_BYTES,
        });
    }
    Ok(())
}

fn valid_date_only(input: &str) -> bool {
    let bytes = input.as_bytes();
    if !input.is_ascii() || bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Some(year) = digits(bytes, 0, 4).and_then(|value| i16::try_from(value).ok()) else {
        return false;
    };
    if year < 1 {
        return false;
    }
    let Some(month) = digits(bytes, 5, 2).and_then(|value| i8::try_from(value).ok()) else {
        return false;
    };
    let Some(day) = digits(bytes, 8, 2).and_then(|value| i8::try_from(value).ok()) else {
        return false;
    };
    DateTime::new(year, month, day, 0, 0, 0, 0).is_ok()
}

#[derive(Clone, Copy)]
struct ParsedDateTime {
    datetime: DateTime,
    offset: Option<i32>,
    precision: SourcePrecision,
}

#[derive(Clone, Copy)]
struct ParseFailure {
    resolution: Resolution,
    precision: SourcePrecision,
    offset: Option<i32>,
}

fn parse_datetime(input: &str, require_offset: bool) -> Result<ParsedDateTime, ParseFailure> {
    let bytes = input.as_bytes();
    let invalid = || ParseFailure {
        resolution: Resolution::Invalid(ResolutionReason::InvalidSyntax),
        precision: SourcePrecision::Unknown,
        offset: None,
    };
    if !input.is_ascii()
        || bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(invalid());
    }
    let year = digits(bytes, 0, 4).ok_or_else(invalid)?;
    let month = digits(bytes, 5, 2).ok_or_else(invalid)?;
    let day = digits(bytes, 8, 2).ok_or_else(invalid)?;
    let hour = digits(bytes, 11, 2).ok_or_else(invalid)?;
    let minute = digits(bytes, 14, 2).ok_or_else(invalid)?;
    let second = digits(bytes, 17, 2).ok_or_else(invalid)?;
    if year == 0 {
        return Err(invalid());
    }
    let leap_second = second == 60;
    let mut cursor = 19;
    let mut nanos = 0_u32;
    let mut precision = SourcePrecision::Second;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        let count = cursor - start;
        if count == 0 || count > 9 {
            return Err(invalid());
        }
        let fraction = digits(bytes, start, count).ok_or_else(invalid)?;
        nanos = fraction
            .checked_mul(10_u32.pow(u32::try_from(9 - count).expect("fraction <= 9")))
            .ok_or_else(invalid)?;
        precision = SourcePrecision::FractionDigits(u8::try_from(count).expect("fraction <= 9"));
    }
    let mut unknown_offset = false;
    let offset = if require_offset {
        match bytes.get(cursor..) {
            Some(b"Z") => Some(0),
            Some(suffix)
                if suffix.len() == 6 && matches!(suffix[0], b'+' | b'-') && suffix[3] == b':' =>
            {
                let hours = digits(suffix, 1, 2).ok_or_else(invalid)?;
                let minutes = digits(suffix, 4, 2).ok_or_else(invalid)?;
                if hours > 23 || minutes > 59 {
                    return Err(invalid());
                }
                if suffix == b"-00:00" {
                    unknown_offset = true;
                    None
                } else {
                    let seconds =
                        i32::try_from(hours * 3_600 + minutes * 60).map_err(|_| invalid())?;
                    Some(if suffix[0] == b'-' { -seconds } else { seconds })
                }
            }
            _ => return Err(invalid()),
        }
    } else if cursor == bytes.len() {
        None
    } else {
        return Err(invalid());
    };
    let year = i16::try_from(year).map_err(|_| invalid())?;
    let month = i8::try_from(month).map_err(|_| invalid())?;
    let day = i8::try_from(day).map_err(|_| invalid())?;
    let hour = i8::try_from(hour).map_err(|_| invalid())?;
    let minute = i8::try_from(minute).map_err(|_| invalid())?;
    let second = i8::try_from(if leap_second { 59 } else { second }).map_err(|_| invalid())?;
    let nanos = i32::try_from(nanos).map_err(|_| invalid())?;
    let datetime =
        DateTime::new(year, month, day, hour, minute, second, nanos).map_err(|_| invalid())?;
    if leap_second {
        return Err(ParseFailure {
            resolution: Resolution::Unsupported(ResolutionReason::LeapSecond),
            precision,
            offset,
        });
    }
    if unknown_offset {
        return Err(ParseFailure {
            resolution: Resolution::Ambiguous(ResolutionReason::UnknownOffset),
            precision,
            offset: None,
        });
    }
    Ok(ParsedDateTime {
        datetime,
        offset,
        precision,
    })
}

fn digits(bytes: &[u8], start: usize, count: usize) -> Option<u32> {
    let end = start.checked_add(count)?;
    let slice = bytes.get(start..end)?;
    let mut value = 0_u32;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(*byte - b'0'))?;
    }
    Some(value)
}

fn timestamp_from_offset(datetime: DateTime, offset_seconds: i32) -> Option<UtcInstant> {
    instant_from_datetime_offset(datetime, offset_seconds)
}

fn resolve_named_zone(
    datetime: DateTime,
    zone: &str,
    supplied_offset: Option<i32>,
    fold: Option<FoldChoice>,
) -> Resolution {
    let Some((canonical, bytes)) = jiff_tzdb::get(zone) else {
        return Resolution::Invalid(ResolutionReason::UnknownZone);
    };
    let Ok(timezone) = TimeZone::tzif(canonical, bytes) else {
        return Resolution::Invalid(ResolutionReason::UnknownZone);
    };
    let ambiguous = timezone.to_ambiguous_timestamp(datetime);
    let effective_offset = match ambiguous.offset() {
        AmbiguousOffset::Gap { .. } => {
            return Resolution::Invalid(ResolutionReason::NonexistentLocalTime);
        }
        AmbiguousOffset::Fold { before, after } => match fold {
            None => return Resolution::Ambiguous(ResolutionReason::FoldChoiceRequired),
            Some(FoldChoice::Earlier) => before.seconds(),
            Some(FoldChoice::Later) => after.seconds(),
        },
        AmbiguousOffset::Unambiguous { offset } => {
            if fold.is_some() {
                return Resolution::Invalid(ResolutionReason::UnnecessaryFoldChoice);
            }
            offset.seconds()
        }
    };
    if supplied_offset.is_some_and(|offset| offset != effective_offset) {
        return Resolution::Invalid(ResolutionReason::OffsetZoneConflict);
    }
    instant_from_datetime_offset(datetime, effective_offset)
        .map(|instant| Resolution::Resolved {
            instant,
            effective_offset_seconds: effective_offset,
        })
        .unwrap_or(Resolution::Invalid(ResolutionReason::OutOfRange))
}

fn numeric_instant(value: i128, unit: EpochUnit) -> Option<UtcInstant> {
    let (divisor, nanos_per_remainder) = match unit {
        EpochUnit::Seconds => (1_i128, 0_i128),
        EpochUnit::Milliseconds => (1_000, 1_000_000),
        EpochUnit::Microseconds => (1_000_000, 1_000),
        EpochUnit::Nanoseconds => (1_000_000_000, 1),
    };
    let seconds = value.div_euclid(divisor);
    let nanos = if divisor == 1 {
        0
    } else {
        value.rem_euclid(divisor).checked_mul(nanos_per_remainder)?
    };
    UtcInstant::new(i64::try_from(seconds).ok()?, u32::try_from(nanos).ok()?).ok()
}

fn instant_from_datetime_offset(datetime: DateTime, offset_seconds: i32) -> Option<UtcInstant> {
    let days = days_from_civil(
        i64::from(datetime.year()),
        i64::from(datetime.month()),
        i64::from(datetime.day()),
    );
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(datetime.hour()) * 3_600)?
        .checked_add(i64::from(datetime.minute()) * 60)?
        .checked_add(i64::from(datetime.second()))?
        .checked_sub(i64::from(offset_seconds))?;
    UtcInstant::new(seconds, u32::try_from(datetime.subsec_nanosecond()).ok()?).ok()
}

fn local_datetime_for_instant(timezone: &TimeZone, instant: UtcInstant) -> Option<(DateTime, i32)> {
    let utc = datetime_from_epoch(instant.seconds(), instant.nanoseconds())?;
    let mut candidates = offsets(timezone.to_ambiguous_timestamp(utc).offset());
    for _ in 0..4 {
        let mut next = Vec::with_capacity(4);
        for candidate in candidates {
            let local_seconds = instant.seconds().checked_add(i64::from(candidate))?;
            let Some(local) = datetime_from_epoch(local_seconds, instant.nanoseconds()) else {
                continue;
            };
            for actual in offsets(timezone.to_ambiguous_timestamp(local).offset()) {
                if instant_from_datetime_offset(local, actual) == Some(instant) {
                    return Some((local, actual));
                }
                if !next.contains(&actual) {
                    next.push(actual);
                }
            }
        }
        if next.is_empty() {
            return None;
        }
        candidates = next;
    }
    None
}

fn offsets(ambiguous: AmbiguousOffset) -> Vec<i32> {
    match ambiguous {
        AmbiguousOffset::Unambiguous { offset } => vec![offset.seconds()],
        AmbiguousOffset::Gap { before, after } | AmbiguousOffset::Fold { before, after } => {
            if before == after {
                vec![before.seconds()]
            } else {
                vec![before.seconds(), after.seconds()]
            }
        }
    }
}

fn datetime_from_epoch(seconds: i64, nanoseconds: u32) -> Option<DateTime> {
    let (year, month, day, hour, minute, second) = civil_from_epoch(seconds);
    DateTime::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
        i8::try_from(hour).ok()?,
        i8::try_from(minute).ok()?,
        i8::try_from(second).ok()?,
        i32::try_from(nanoseconds).ok()?,
    )
    .ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_epoch(seconds: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let shifted_days = days + 719_468;
    let era = shifted_days.div_euclid(146_097);
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        year,
        month,
        day,
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60,
    )
}

fn format_datetime(datetime: DateTime) -> String {
    let mut result = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        datetime.year(),
        datetime.month(),
        datetime.day(),
        datetime.hour(),
        datetime.minute(),
        datetime.second()
    );
    let nanos = datetime.subsec_nanosecond();
    if nanos != 0 {
        let mut fraction = format!("{nanos:09}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        result.push('.');
        result.push_str(&fraction);
    }
    result
}

fn format_offset(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute = offset.unsigned_abs();
    let hours = absolute / 3_600;
    let minutes = (absolute % 3_600) / 60;
    let seconds = absolute % 60;
    if seconds == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{seconds:02}")
    }
}

fn embedded_tzdb_digest() -> Option<[u8; 32]> {
    let mut names: Vec<&str> = jiff_tzdb::available().collect();
    names.sort_unstable();
    let mut digest = Sha256::new();
    digest.update(b"uste-tzdb-profile-v1\0");
    hash_part(&mut digest, TZDB_VERSION.as_bytes())?;
    for name in names {
        let (_, data) = jiff_tzdb::get(name)?;
        hash_part(&mut digest, name.as_bytes())?;
        hash_part(&mut digest, data)?;
    }
    Some(digest.finalize().into())
}

fn hash_part(digest: &mut Sha256, value: &[u8]) -> Option<()> {
    digest.update(u64::try_from(value.len()).ok()?.to_be_bytes());
    digest.update(value);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::{civil_from_epoch, days_from_civil, embedded_tzdb_digest};
    use crate::TZDB_PROFILE_DIGEST;

    #[test]
    fn embedded_profile_digest_is_pinned() {
        assert_eq!(embedded_tzdb_digest(), Some(TZDB_PROFILE_DIGEST));
    }

    #[test]
    fn every_supported_gregorian_day_round_trips_epoch_arithmetic() {
        let first = days_from_civil(1, 1, 1);
        let last = days_from_civil(9_999, 12, 31);
        assert_eq!(first * 86_400, uste_types::MIN_EPOCH_SECONDS);
        assert_eq!(last * 86_400 + 86_399, uste_types::MAX_EPOCH_SECONDS);
        for day in first..=last {
            let (year, month, date, hour, minute, second) = civil_from_epoch(day * 86_400);
            assert_eq!((hour, minute, second), (0, 0, 0));
            assert_eq!(days_from_civil(year, month, date), day);
        }
    }
}
