//! Capability-free fixed-point spatial primitives selected by `space-v1`.

use core::{fmt, num::NonZeroU64};

use crate::RecordRef;

pub const SPACE_PROFILE: &str = "space-v1";
pub const MAX_FRAME_DEPTH: usize = 32;

const HALF_TURN_NANODEGREES: i64 = 180_000_000_000;
const QUARTER_TURN_NANODEGREES: i64 = 90_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpatialPrimitiveError {
    InvalidVersion,
    VersionExhausted,
    InvalidLatitude,
    InvalidLongitude,
    InvalidBounds,
}

impl fmt::Display for SpatialPrimitiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SpatialPrimitiveError {}

/// Nonzero immutable schema/reference version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SpatialVersion(NonZeroU64);

impl SpatialVersion {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub const fn new(value: u64) -> Result<Self, SpatialPrimitiveError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(SpatialPrimitiveError::InvalidVersion),
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub const fn checked_next(self) -> Result<Self, SpatialPrimitiveError> {
        match self.get().checked_add(1) {
            Some(value) => Self::new(value),
            None => Err(SpatialPrimitiveError::VersionExhausted),
        }
    }
}

/// A durable record reference pinned to one immutable spatial schema version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VersionedRecordRef {
    pub record: RecordRef,
    pub version: SpatialVersion,
}

impl VersionedRecordRef {
    #[must_use]
    pub const fn new(record: RecordRef, version: SpatialVersion) -> Self {
        Self { record, version }
    }
}

/// Signed local Cartesian distance in exact SI nanometres.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Nanometres(i128);

impl Nanometres {
    #[must_use]
    pub const fn new(value: i128) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> i128 {
        self.0
    }
}

/// Nonnegative radial distance/uncertainty in exact SI nanometres.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NonNegativeNanometres(u128);

impl NonNegativeNanometres {
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Canonical longitude in nanodegrees, in `[-180°, 180°)`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LongitudeNanodegrees(i64);

impl LongitudeNanodegrees {
    pub const fn new(value: i64) -> Result<Self, SpatialPrimitiveError> {
        if value < -HALF_TURN_NANODEGREES || value > HALF_TURN_NANODEGREES {
            return Err(SpatialPrimitiveError::InvalidLongitude);
        }
        if value == HALF_TURN_NANODEGREES {
            Ok(Self(-HALF_TURN_NANODEGREES))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Latitude in nanodegrees, in `[-90°, 90°]`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LatitudeNanodegrees(i64);

impl LatitudeNanodegrees {
    pub const fn new(value: i64) -> Result<Self, SpatialPrimitiveError> {
        if value < -QUARTER_TURN_NANODEGREES || value > QUARTER_TURN_NANODEGREES {
            Err(SpatialPrimitiveError::InvalidLatitude)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_pole(self) -> bool {
        self.0 == -QUARTER_TURN_NANODEGREES || self.0 == QUARTER_TURN_NANODEGREES
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPoint2 {
    pub x: Nanometres,
    pub y: Nanometres,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalPoint3 {
    pub x: Nanometres,
    pub y: Nanometres,
    pub z: Nanometres,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeographicPoint {
    longitude: LongitudeNanodegrees,
    latitude: LatitudeNanodegrees,
    height: Option<Nanometres>,
}

impl GeographicPoint {
    #[must_use]
    pub const fn new(
        longitude: LongitudeNanodegrees,
        latitude: LatitudeNanodegrees,
        height: Option<Nanometres>,
    ) -> Self {
        let longitude = if latitude.is_pole() {
            LongitudeNanodegrees(0)
        } else {
            longitude
        };
        Self {
            longitude,
            latitude,
            height,
        }
    }

    #[must_use]
    pub const fn longitude(self) -> LongitudeNanodegrees {
        self.longitude
    }
    #[must_use]
    pub const fn latitude(self) -> LatitudeNanodegrees {
        self.latitude
    }
    /// `None` is unknown height, never an implicit ellipsoid height of zero.
    #[must_use]
    pub const fn height(self) -> Option<Nanometres> {
        self.height
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalBox2 {
    min: LocalPoint2,
    max: LocalPoint2,
}

impl LocalBox2 {
    pub const fn new(min: LocalPoint2, max: LocalPoint2) -> Result<Self, SpatialPrimitiveError> {
        if min.x.get() > max.x.get() || min.y.get() > max.y.get() {
            Err(SpatialPrimitiveError::InvalidBounds)
        } else {
            Ok(Self { min, max })
        }
    }

    #[must_use]
    pub const fn min(self) -> LocalPoint2 {
        self.min
    }

    #[must_use]
    pub const fn max(self) -> LocalPoint2 {
        self.max
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalBox3 {
    min: LocalPoint3,
    max: LocalPoint3,
}

impl LocalBox3 {
    pub const fn new(min: LocalPoint3, max: LocalPoint3) -> Result<Self, SpatialPrimitiveError> {
        if min.x.get() > max.x.get() || min.y.get() > max.y.get() || min.z.get() > max.z.get() {
            Err(SpatialPrimitiveError::InvalidBounds)
        } else {
            Ok(Self { min, max })
        }
    }

    #[must_use]
    pub const fn min(self) -> LocalPoint3 {
        self.min
    }

    #[must_use]
    pub const fn max(self) -> LocalPoint3 {
        self.max
    }
}

/// Closed geographic bounds. `west > east` means antimeridian wrap; equality is zero width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeographicBox {
    west: LongitudeNanodegrees,
    east: LongitudeNanodegrees,
    south: LatitudeNanodegrees,
    north: LatitudeNanodegrees,
}

impl GeographicBox {
    pub const fn new(
        west: LongitudeNanodegrees,
        east: LongitudeNanodegrees,
        south: LatitudeNanodegrees,
        north: LatitudeNanodegrees,
    ) -> Result<Self, SpatialPrimitiveError> {
        if south.get() > north.get() {
            Err(SpatialPrimitiveError::InvalidBounds)
        } else {
            Ok(Self {
                west,
                east,
                south,
                north,
            })
        }
    }

    #[must_use]
    pub const fn west(self) -> LongitudeNanodegrees {
        self.west
    }
    #[must_use]
    pub const fn east(self) -> LongitudeNanodegrees {
        self.east
    }
    #[must_use]
    pub const fn south(self) -> LatitudeNanodegrees {
        self.south
    }
    #[must_use]
    pub const fn north(self) -> LatitudeNanodegrees {
        self.north
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpatialPosition {
    Local2(LocalPoint2),
    Local3(LocalPoint3),
    Geographic(GeographicPoint),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateSystem {
    GeographicWgs84,
    LocalCartesian2,
    LocalCartesian3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisOrder {
    LongitudeLatitude,
    Xy,
    Xyz,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handedness {
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LengthUnit {
    Nanometre,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AngularUnit {
    Nanodegree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalReference {
    Wgs84Ellipsoid,
}

/// Closed coordinate profile prevents invalid axis/unit/handedness combinations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoordinateProfile {
    system: CoordinateSystem,
}

impl CoordinateProfile {
    #[must_use]
    pub const fn new(system: CoordinateSystem) -> Self {
        Self { system }
    }

    #[must_use]
    pub const fn system(self) -> CoordinateSystem {
        self.system
    }

    #[must_use]
    pub const fn axis_order(self) -> AxisOrder {
        match self.system {
            CoordinateSystem::GeographicWgs84 => AxisOrder::LongitudeLatitude,
            CoordinateSystem::LocalCartesian2 => AxisOrder::Xy,
            CoordinateSystem::LocalCartesian3 => AxisOrder::Xyz,
        }
    }

    #[must_use]
    pub const fn handedness(self) -> Option<Handedness> {
        match self.system {
            CoordinateSystem::GeographicWgs84 => None,
            CoordinateSystem::LocalCartesian2 | CoordinateSystem::LocalCartesian3 => {
                Some(Handedness::Right)
            }
        }
    }

    #[must_use]
    pub const fn length_unit(self) -> LengthUnit {
        LengthUnit::Nanometre
    }

    #[must_use]
    pub const fn angular_unit(self) -> Option<AngularUnit> {
        match self.system {
            CoordinateSystem::GeographicWgs84 => Some(AngularUnit::Nanodegree),
            CoordinateSystem::LocalCartesian2 | CoordinateSystem::LocalCartesian3 => None,
        }
    }

    #[must_use]
    pub const fn vertical_reference(self) -> Option<VerticalReference> {
        match self.system {
            CoordinateSystem::GeographicWgs84 => Some(VerticalReference::Wgs84Ellipsoid),
            CoordinateSystem::LocalCartesian2 | CoordinateSystem::LocalCartesian3 => None,
        }
    }

    #[must_use]
    pub const fn accepts(self, position: SpatialPosition) -> bool {
        matches!(
            (self.system, position),
            (
                CoordinateSystem::GeographicWgs84,
                SpatialPosition::Geographic(_)
            ) | (
                CoordinateSystem::LocalCartesian2,
                SpatialPosition::Local2(_)
            ) | (
                CoordinateSystem::LocalCartesian3,
                SpatialPosition::Local3(_)
            )
        )
    }
}
