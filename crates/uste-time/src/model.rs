use core::fmt;

use uste_types::{BoundedString, RecordRef, UtcInstant};

pub const ENVELOPE_PROFILE: &str = "uste-time-envelope-v1";
pub const NORMALIZATION_PROFILE: &str = "posix-utc-v1";
pub const PARSER_PROFILE: &str = "strict-rfc3339-v1";
pub const CALENDAR_PROFILE: &str = "proleptic-gregorian-v1";
pub const FORMAT_PROFILE: &str = "uste-local-presentation-v1";
pub const TZDB_VERSION: &str = "2026c";

/// SHA-256 over the domain, version and sorted canonical name/TZif entries of jiff-tzdb 0.1.8.
/// The value is verified before named-zone normalization is made available.
pub const TZDB_PROFILE_DIGEST: [u8; 32] = [
    0x8e, 0x18, 0xc4, 0xfd, 0x3a, 0xad, 0x2c, 0x58, 0xd4, 0x53, 0x40, 0xe3, 0x56, 0xa0, 0x22, 0x29,
    0xce, 0x68, 0x11, 0xc8, 0x34, 0x27, 0xc5, 0xb0, 0x86, 0xad, 0x44, 0x68, 0xb7, 0x58, 0x22, 0x09,
];

pub const MAX_ORIGINAL_BYTES: usize = 4_096;
pub const MAX_LOCATOR_BYTES: usize = 4_096;
pub const MAX_ZONE_BYTES: usize = 255;
pub const MAX_ASSUMPTION_BYTES: usize = 1_024;
pub const MAX_ASSUMPTIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampRole {
    SourceEvent,
    SourceCreated,
    SourceModified,
    SourcePublished,
    Received,
    CommitObservation,
    DerivationAvailable,
    SimulationAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochUnit {
    Seconds,
    Milliseconds,
    Microseconds,
    Nanoseconds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeScale {
    PosixUtc,
    UtcLeapAware,
    Tai,
    Gps,
    Smeared,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FoldChoice {
    Earlier,
    Later,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePrecision {
    Unknown,
    Date,
    Second,
    FractionDigits(u8),
    EpochUnit(EpochUnit),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockUncertainty {
    Unknown,
    BoundedNanoseconds { before: u64, after: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionStatus {
    Resolved,
    Missing,
    Ambiguous,
    Invalid,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionReason {
    Accepted,
    MissingValue,
    MissingZone,
    DateOnlyWithoutPolicy,
    UnitRequired,
    InvalidSyntax,
    OutOfRange,
    UnknownOffset,
    LeapSecond,
    UnsupportedTimeScale,
    UnknownZone,
    FoldChoiceRequired,
    UnnecessaryFoldChoice,
    NonexistentLocalTime,
    OffsetZoneConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimezoneProfile {
    pub version: &'static str,
    pub digest: [u8; 32],
}

impl TimezoneProfile {
    pub const PINNED: Self = Self {
        version: TZDB_VERSION,
        digest: TZDB_PROFILE_DIGEST,
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDescriptor {
    artifact: RecordRef,
    artifact_version: u64,
    locator: BoundedString,
}

impl SourceDescriptor {
    pub fn new(
        artifact: RecordRef,
        artifact_version: u64,
        locator: impl AsRef<str>,
    ) -> Result<Self, TimeError> {
        if artifact_version == 0 {
            return Err(TimeError::InvalidEnvelope);
        }
        let locator = bounded_borrowed(locator.as_ref(), MAX_LOCATOR_BYTES, "locator")?;
        Ok(Self {
            artifact,
            artifact_version,
            locator,
        })
    }

    #[must_use]
    pub const fn artifact(&self) -> RecordRef {
        self.artifact
    }

    #[must_use]
    pub const fn artifact_version(&self) -> u64 {
        self.artifact_version
    }

    #[must_use]
    pub fn locator(&self) -> &str {
        self.locator.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedAssumption {
    statement: BoundedString,
    authority: RecordRef,
}

impl AuthorizedAssumption {
    pub fn new(statement: impl AsRef<str>, authority: RecordRef) -> Result<Self, TimeError> {
        Ok(Self {
            statement: bounded_borrowed(statement.as_ref(), MAX_ASSUMPTION_BYTES, "assumption")?,
            authority,
        })
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        self.statement.as_str()
    }

    #[must_use]
    pub const fn authority(&self) -> RecordRef {
        self.authority
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeInterpretation {
    pub(crate) scale: TimeScale,
    pub(crate) epoch_unit: Option<EpochUnit>,
    pub(crate) supplied_offset_seconds: Option<i32>,
    pub(crate) zone: Option<BoundedString>,
    pub(crate) fold: Option<FoldChoice>,
}

impl TimeInterpretation {
    #[must_use]
    pub const fn scale(&self) -> TimeScale {
        self.scale
    }

    #[must_use]
    pub const fn epoch_unit(&self) -> Option<EpochUnit> {
        self.epoch_unit
    }

    #[must_use]
    pub const fn supplied_offset_seconds(&self) -> Option<i32> {
        self.supplied_offset_seconds
    }

    #[must_use]
    pub fn zone(&self) -> Option<&str> {
        self.zone.as_ref().map(BoundedString::as_str)
    }

    #[must_use]
    pub const fn fold(&self) -> Option<FoldChoice> {
        self.fold
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    Resolved {
        instant: UtcInstant,
        effective_offset_seconds: i32,
    },
    Missing(ResolutionReason),
    Ambiguous(ResolutionReason),
    Invalid(ResolutionReason),
    Unsupported(ResolutionReason),
}

impl Resolution {
    #[must_use]
    pub const fn status(self) -> ResolutionStatus {
        match self {
            Self::Resolved { .. } => ResolutionStatus::Resolved,
            Self::Missing(_) => ResolutionStatus::Missing,
            Self::Ambiguous(_) => ResolutionStatus::Ambiguous,
            Self::Invalid(_) => ResolutionStatus::Invalid,
            Self::Unsupported(_) => ResolutionStatus::Unsupported,
        }
    }

    #[must_use]
    pub const fn reason(self) -> ResolutionReason {
        match self {
            Self::Resolved { .. } => ResolutionReason::Accepted,
            Self::Missing(reason)
            | Self::Ambiguous(reason)
            | Self::Invalid(reason)
            | Self::Unsupported(reason) => reason,
        }
    }

    #[must_use]
    pub const fn instant(self) -> Option<UtcInstant> {
        match self {
            Self::Resolved { instant, .. } => Some(instant),
            _ => None,
        }
    }

    #[must_use]
    pub const fn effective_offset_seconds(self) -> Option<i32> {
        match self {
            Self::Resolved {
                effective_offset_seconds,
                ..
            } => Some(effective_offset_seconds),
            _ => None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TimestampEnvelope {
    pub(crate) source: SourceDescriptor,
    pub(crate) role: TimestampRole,
    pub(crate) original: BoundedString,
    pub(crate) interpretation: TimeInterpretation,
    pub(crate) resolution: Resolution,
    pub(crate) precision: SourcePrecision,
    pub(crate) uncertainty: ClockUncertainty,
    pub(crate) assumptions: Vec<AuthorizedAssumption>,
    pub(crate) timezone_profile: Option<TimezoneProfile>,
}

impl fmt::Debug for TimestampEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TimestampEnvelope")
            .field("source", &"[SOURCE]")
            .field("role", &self.role)
            .field("original", &"[SOURCE TOKEN]")
            .field("interpretation", &self.interpretation)
            .field("resolution", &self.resolution)
            .field("precision", &self.precision)
            .field("uncertainty", &self.uncertainty)
            .field("assumption_count", &self.assumptions.len())
            .field("timezone_profile", &self.timezone_profile)
            .finish()
    }
}

impl TimestampEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn checked(
        source: SourceDescriptor,
        role: TimestampRole,
        original: String,
        interpretation: TimeInterpretation,
        resolution: Resolution,
        precision: SourcePrecision,
        uncertainty: ClockUncertainty,
        assumptions: Vec<AuthorizedAssumption>,
        timezone_profile: Option<TimezoneProfile>,
    ) -> Result<Self, TimeError> {
        if assumptions.len() > MAX_ASSUMPTIONS
            || matches!(precision, SourcePrecision::FractionDigits(0 | 10..=u8::MAX))
            || matches!(
                (precision, interpretation.epoch_unit),
                (SourcePrecision::EpochUnit(left), Some(right)) if left != right
            )
            || matches!(precision, SourcePrecision::EpochUnit(_))
                != interpretation.epoch_unit.is_some()
            || interpretation
                .supplied_offset_seconds
                .is_some_and(|offset| !valid_offset(offset))
                && resolution != Resolution::Invalid(ResolutionReason::InvalidSyntax)
            || interpretation
                .zone
                .as_ref()
                .is_some_and(|zone| zone.as_str().len() > MAX_ZONE_BYTES)
            || timezone_profile.is_some_and(|profile| profile != TimezoneProfile::PINNED)
            || timezone_profile.is_some() != interpretation.zone.is_some()
            || resolution
                .effective_offset_seconds()
                .is_some_and(|offset| !(-93_599..=93_599).contains(&offset))
            || matches!(resolution, Resolution::Resolved { .. })
                && interpretation.scale != TimeScale::PosixUtc
            || matches!(resolution, Resolution::Resolved { .. })
                && matches!(precision, SourcePrecision::Unknown | SourcePrecision::Date)
            || matches!(resolution, Resolution::Resolved { .. })
                && interpretation
                    .supplied_offset_seconds
                    .is_some_and(|supplied| resolution.effective_offset_seconds() != Some(supplied))
            || matches!(precision, SourcePrecision::Date)
                && !matches!(
                    resolution,
                    Resolution::Ambiguous(ResolutionReason::DateOnlyWithoutPolicy)
                        | Resolution::Invalid(ResolutionReason::InvalidSyntax)
                )
            || matches!(resolution, Resolution::Missing(_)) && precision != SourcePrecision::Unknown
            || interpretation.epoch_unit.is_some()
                && (interpretation.zone.is_some()
                    || interpretation.fold.is_some()
                    || interpretation.supplied_offset_seconds.is_some()
                    || matches!(
                        resolution,
                        Resolution::Resolved {
                            effective_offset_seconds,
                            ..
                        } if effective_offset_seconds != 0
                    ))
            || interpretation.epoch_unit.is_none()
                && matches!(resolution, Resolution::Resolved { .. })
                && interpretation.zone.is_none()
                && interpretation.supplied_offset_seconds.is_none()
            || matches!(resolution, Resolution::Resolved { .. })
                && interpretation.fold.is_some()
                && interpretation.zone.is_none()
            || !semantic_shape_matches(&interpretation, resolution, precision, &original)
            || !resolution_reason_matches(resolution)
        {
            return Err(TimeError::InvalidEnvelope);
        }
        Ok(Self {
            source,
            role,
            original: bounded(original, MAX_ORIGINAL_BYTES, "original")?,
            interpretation,
            resolution,
            precision,
            uncertainty,
            assumptions,
            timezone_profile,
        })
    }

    #[must_use]
    pub const fn source(&self) -> &SourceDescriptor {
        &self.source
    }

    #[must_use]
    pub const fn role(&self) -> TimestampRole {
        self.role
    }

    #[must_use]
    pub fn original(&self) -> &str {
        self.original.as_str()
    }

    #[must_use]
    pub const fn interpretation(&self) -> &TimeInterpretation {
        &self.interpretation
    }

    #[must_use]
    pub const fn resolution(&self) -> Resolution {
        self.resolution
    }

    #[must_use]
    pub const fn status(&self) -> ResolutionStatus {
        self.resolution.status()
    }

    #[must_use]
    pub const fn precision(&self) -> SourcePrecision {
        self.precision
    }

    #[must_use]
    pub const fn uncertainty(&self) -> ClockUncertainty {
        self.uncertainty
    }

    #[must_use]
    pub fn assumptions(&self) -> &[AuthorizedAssumption] {
        &self.assumptions
    }

    #[must_use]
    pub const fn timezone_profile(&self) -> Option<TimezoneProfile> {
        self.timezone_profile
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeInput<'a> {
    Missing,
    Rfc3339(&'a str),
    Numeric {
        token: &'a str,
        value: i128,
        unit: Option<EpochUnit>,
        scale: TimeScale,
    },
    Local {
        text: &'a str,
        zone: Option<&'a str>,
        offset_seconds: Option<i32>,
        fold: Option<FoldChoice>,
    },
    DateOnly(&'a str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPresentation {
    text: String,
    zone: String,
    offset_seconds: i32,
    timezone_profile: TimezoneProfile,
}

impl LocalPresentation {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn zone(&self) -> &str {
        &self.zone
    }

    #[must_use]
    pub const fn offset_seconds(&self) -> i32 {
        self.offset_seconds
    }

    #[must_use]
    pub const fn timezone_profile(&self) -> TimezoneProfile {
        self.timezone_profile
    }

    #[must_use]
    pub const fn format_profile(&self) -> &'static str {
        FORMAT_PROFILE
    }

    pub(crate) fn new(text: String, zone: String, offset_seconds: i32) -> Self {
        Self {
            text,
            zone,
            offset_seconds,
            timezone_profile: TimezoneProfile::PINNED,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimeError {
    FieldTooLarge {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    InvalidEnvelope,
    InvalidEncoding,
    UnsupportedProfile,
    PinnedProfileUnavailable,
    ResourceLimit,
}

impl fmt::Display for TimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldTooLarge {
                field,
                actual,
                maximum,
            } => write!(formatter, "{field} length {actual} exceeds {maximum}"),
            Self::InvalidEnvelope => formatter.write_str("timestamp envelope is inconsistent"),
            Self::InvalidEncoding => formatter.write_str("timestamp envelope encoding is invalid"),
            Self::UnsupportedProfile => formatter.write_str("timestamp profile is unsupported"),
            Self::PinnedProfileUnavailable => {
                formatter.write_str("pinned timezone profile is unavailable or changed")
            }
            Self::ResourceLimit => {
                formatter.write_str("bounded time operation exhausted resources")
            }
        }
    }
}

impl std::error::Error for TimeError {}

pub(crate) fn bounded(
    value: String,
    maximum: usize,
    field: &'static str,
) -> Result<BoundedString, TimeError> {
    if value.len() > maximum {
        return Err(TimeError::FieldTooLarge {
            field,
            actual: value.len(),
            maximum,
        });
    }
    BoundedString::new(value).map_err(|_| TimeError::ResourceLimit)
}

fn bounded_borrowed(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<BoundedString, TimeError> {
    if value.len() > maximum {
        return Err(TimeError::FieldTooLarge {
            field,
            actual: value.len(),
            maximum,
        });
    }
    BoundedString::new(value.to_owned()).map_err(|_| TimeError::ResourceLimit)
}

pub(crate) const fn valid_offset(offset: i32) -> bool {
    offset >= -86_340 && offset <= 86_340 && offset % 60 == 0
}

fn resolution_reason_matches(resolution: Resolution) -> bool {
    matches!(
        resolution,
        Resolution::Resolved { .. }
            | Resolution::Missing(ResolutionReason::MissingValue)
            | Resolution::Ambiguous(
                ResolutionReason::MissingZone
                    | ResolutionReason::DateOnlyWithoutPolicy
                    | ResolutionReason::UnknownOffset
                    | ResolutionReason::FoldChoiceRequired
            )
            | Resolution::Invalid(
                ResolutionReason::UnitRequired
                    | ResolutionReason::InvalidSyntax
                    | ResolutionReason::OutOfRange
                    | ResolutionReason::UnknownZone
                    | ResolutionReason::UnnecessaryFoldChoice
                    | ResolutionReason::NonexistentLocalTime
                    | ResolutionReason::OffsetZoneConflict
            )
            | Resolution::Unsupported(
                ResolutionReason::LeapSecond | ResolutionReason::UnsupportedTimeScale
            )
    )
}

fn semantic_shape_matches(
    interpretation: &TimeInterpretation,
    resolution: Resolution,
    precision: SourcePrecision,
    original: &str,
) -> bool {
    let empty_interpretation = interpretation.epoch_unit.is_none()
        && interpretation.supplied_offset_seconds.is_none()
        && interpretation.zone.is_none()
        && interpretation.fold.is_none();
    if resolution == Resolution::Missing(ResolutionReason::MissingValue) {
        return original.is_empty()
            && interpretation.scale == TimeScale::Unknown
            && empty_interpretation
            && precision == SourcePrecision::Unknown;
    }
    if precision == SourcePrecision::Date {
        return interpretation.scale == TimeScale::PosixUtc
            && empty_interpretation
            && matches!(
                resolution,
                Resolution::Ambiguous(ResolutionReason::DateOnlyWithoutPolicy)
                    | Resolution::Invalid(ResolutionReason::InvalidSyntax)
            );
    }
    if interpretation.scale != TimeScale::PosixUtc {
        return interpretation.supplied_offset_seconds.is_none()
            && interpretation.zone.is_none()
            && interpretation.fold.is_none()
            && resolution == Resolution::Unsupported(ResolutionReason::UnsupportedTimeScale);
    }
    if resolution == Resolution::Unsupported(ResolutionReason::UnsupportedTimeScale) {
        return false;
    }
    if interpretation.epoch_unit.is_some() {
        return matches!(
            resolution,
            Resolution::Resolved {
                effective_offset_seconds: 0,
                ..
            } | Resolution::Invalid(ResolutionReason::OutOfRange)
        );
    }
    let wall_precision = matches!(
        precision,
        SourcePrecision::Second | SourcePrecision::FractionDigits(_)
    );
    match resolution {
        Resolution::Invalid(ResolutionReason::UnitRequired) => {
            empty_interpretation && precision == SourcePrecision::Unknown
        }
        Resolution::Unsupported(ResolutionReason::LeapSecond) => matches!(
            precision,
            SourcePrecision::Second | SourcePrecision::FractionDigits(_)
        ),
        Resolution::Ambiguous(ResolutionReason::UnknownOffset) => {
            empty_interpretation && wall_precision
        }
        Resolution::Ambiguous(ResolutionReason::MissingZone) => {
            empty_interpretation && wall_precision
        }
        Resolution::Ambiguous(ResolutionReason::FoldChoiceRequired) => {
            interpretation.zone.is_some() && interpretation.fold.is_none() && wall_precision
        }
        Resolution::Invalid(ResolutionReason::UnknownZone)
        | Resolution::Invalid(ResolutionReason::NonexistentLocalTime) => {
            interpretation.zone.is_some() && wall_precision
        }
        Resolution::Invalid(ResolutionReason::UnnecessaryFoldChoice) => {
            interpretation.fold.is_some() && wall_precision
        }
        Resolution::Invalid(ResolutionReason::OffsetZoneConflict) => {
            interpretation.zone.is_some()
                && interpretation.supplied_offset_seconds.is_some()
                && wall_precision
        }
        Resolution::Invalid(ResolutionReason::InvalidSyntax) => {
            precision == SourcePrecision::Unknown
        }
        Resolution::Invalid(ResolutionReason::OutOfRange) => {
            (interpretation.zone.is_some() || interpretation.supplied_offset_seconds.is_some())
                && wall_precision
        }
        Resolution::Resolved { .. } => wall_precision,
        _ => false,
    }
}
