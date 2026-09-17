use core::fmt;

use uste_time::{TimestampEnvelope, TimestampRole};
use uste_types::{
    BoundedString, RecordRef, SourceEventRef,
    spatial::{
        CoordinateProfile, CoordinateSystem, GeographicBox, GeographicPoint, LocalBox2, LocalBox3,
        SpatialPosition, SpatialPrimitiveError, SpatialVersion, VersionedRecordRef,
    },
};

pub const SPATIAL_PROFILE: &str = "uste-spatial-record-v1";
pub const MAX_SESSION_BYTES: usize = 255;
pub const MAX_OBSERVATIONS_PER_RESULT: usize = 100_000;
pub const MAX_SPATIAL_BATCH_RECORDS: usize = 10_000;
pub const MAX_SPATIAL_CATALOG_ENTRIES: usize = 1_000_000;
/// R1 in-memory correctness-catalog limit over canonical record bytes plus checkpoint framing.
/// This is not the R3 native-index capacity target.
pub const MAX_SPATIAL_CATALOG_LOGICAL_BYTES: usize = 64 * 1024 * 1024;
pub type FrameKind = CoordinateSystem;

pub(crate) fn accepts_geometry(kind: CoordinateSystem, geometry: Geometry) -> bool {
    matches!(
        (kind, geometry),
        (
            CoordinateSystem::GeographicWgs84,
            Geometry::GeographicPoint(_) | Geometry::GeographicBox(_)
        ) | (
            CoordinateSystem::LocalCartesian2,
            Geometry::LocalPoint2(_) | Geometry::LocalBox2(_)
        ) | (
            CoordinateSystem::LocalCartesian3,
            Geometry::LocalPoint3(_) | Geometry::LocalBox3(_)
        )
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldDefinition {
    id: RecordRef,
    version: SpatialVersion,
    root_frame: VersionedRecordRef,
}

impl WorldDefinition {
    pub fn new(
        id: RecordRef,
        version: SpatialVersion,
        root_frame: VersionedRecordRef,
    ) -> Result<Self, SpatialError> {
        same_scope(id, root_frame.record)?;
        Ok(Self {
            id,
            version,
            root_frame,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RecordRef {
        self.id
    }

    #[must_use]
    pub const fn version(&self) -> SpatialVersion {
        self.version
    }

    #[must_use]
    pub const fn root_frame(&self) -> VersionedRecordRef {
        self.root_frame
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameParent {
    /// Exact parent-frame version used by this immutable frame version.
    pub frame: VersionedRecordRef,
    /// Opaque exact transform binding. T-50 validates its target and numeric meaning.
    pub transform: VersionedRecordRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameDefinition {
    id: RecordRef,
    version: SpatialVersion,
    world: RecordRef,
    profile: CoordinateProfile,
    parent: Option<FrameParent>,
}

impl FrameDefinition {
    pub fn new(
        id: RecordRef,
        version: SpatialVersion,
        world: RecordRef,
        kind: CoordinateSystem,
        parent: Option<FrameParent>,
    ) -> Result<Self, SpatialError> {
        same_scope(id, world)?;
        if let Some(parent) = parent {
            same_scope(id, parent.frame.record)?;
            same_scope(id, parent.transform.record)?;
            if parent.frame.record == id {
                return Err(SpatialError::FrameCycle);
            }
        }
        Ok(Self {
            id,
            version,
            world,
            profile: CoordinateProfile::new(kind),
            parent,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RecordRef {
        self.id
    }

    #[must_use]
    pub const fn version(&self) -> SpatialVersion {
        self.version
    }

    #[must_use]
    pub const fn world(&self) -> RecordRef {
        self.world
    }

    #[must_use]
    pub const fn kind(&self) -> CoordinateSystem {
        self.profile.system()
    }

    #[must_use]
    pub const fn profile(&self) -> CoordinateProfile {
        self.profile
    }

    #[must_use]
    pub const fn parent(&self) -> Option<FrameParent> {
        self.parent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Geometry {
    LocalPoint2(crate::LocalPoint2),
    LocalPoint3(crate::LocalPoint3),
    GeographicPoint(GeographicPoint),
    LocalBox2(LocalBox2),
    LocalBox3(LocalBox3),
    GeographicBox(GeographicBox),
}

impl Geometry {
    #[must_use]
    pub const fn point(self) -> Option<SpatialPosition> {
        match self {
            Self::LocalPoint2(value) => Some(SpatialPosition::Local2(value)),
            Self::LocalPoint3(value) => Some(SpatialPosition::Local3(value)),
            Self::GeographicPoint(value) => Some(SpatialPosition::Geographic(value)),
            Self::LocalBox2(_) | Self::LocalBox3(_) | Self::GeographicBox(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionUncertainty {
    Unknown,
    Radial(crate::NonNegativeNanometres),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryVersion {
    id: RecordRef,
    version: SpatialVersion,
    world: RecordRef,
    frame: VersionedRecordRef,
    geometry: Geometry,
    precision: PositionUncertainty,
    predecessor: Option<VersionedRecordRef>,
}

impl GeometryVersion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RecordRef,
        version: SpatialVersion,
        world: RecordRef,
        frame: VersionedRecordRef,
        geometry: Geometry,
        precision: PositionUncertainty,
        predecessor: Option<VersionedRecordRef>,
    ) -> Result<Self, SpatialError> {
        same_scope(id, world)?;
        same_scope(id, frame.record)?;
        if let Some(predecessor) = predecessor
            && (predecessor.record != id || predecessor.version >= version)
        {
            return Err(SpatialError::InvalidPredecessor);
        }
        Ok(Self {
            id,
            version,
            world,
            frame,
            geometry,
            precision,
            predecessor,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RecordRef {
        self.id
    }
    #[must_use]
    pub const fn version(&self) -> SpatialVersion {
        self.version
    }
    #[must_use]
    pub const fn world(&self) -> RecordRef {
        self.world
    }
    #[must_use]
    pub const fn frame(&self) -> VersionedRecordRef {
        self.frame
    }
    #[must_use]
    pub const fn geometry(&self) -> Geometry {
        self.geometry
    }
    #[must_use]
    pub const fn precision(&self) -> PositionUncertainty {
        self.precision
    }
    #[must_use]
    pub const fn predecessor(&self) -> Option<VersionedRecordRef> {
        self.predecessor
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObservationKey {
    source: RecordRef,
    session: BoundedString,
    event: SourceEventRef,
}

impl ObservationKey {
    pub fn new(
        source: RecordRef,
        session: impl AsRef<str>,
        event: SourceEventRef,
    ) -> Result<Self, SpatialError> {
        same_source_scope(source, event)?;
        let session = session.as_ref();
        if session.is_empty() || session.len() > MAX_SESSION_BYTES {
            return Err(SpatialError::InvalidSession);
        }
        Ok(Self {
            source,
            session: BoundedString::new(session.to_owned())
                .map_err(|_| SpatialError::ResourceLimit)?,
            event,
        })
    }

    #[must_use]
    pub const fn source(&self) -> RecordRef {
        self.source
    }
    #[must_use]
    pub fn session(&self) -> &str {
        self.session.as_str()
    }
    #[must_use]
    pub const fn event(&self) -> SourceEventRef {
        self.event
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PositionObservation {
    id: RecordRef,
    entity: RecordRef,
    world: RecordRef,
    frame: VersionedRecordRef,
    key: ObservationKey,
    evidence: RecordRef,
    source_time: TimestampEnvelope,
    position: SpatialPosition,
    uncertainty: PositionUncertainty,
    correction_of: Option<RecordRef>,
}

impl PositionObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RecordRef,
        entity: RecordRef,
        world: RecordRef,
        frame: VersionedRecordRef,
        key: ObservationKey,
        evidence: RecordRef,
        source_time: TimestampEnvelope,
        position: SpatialPosition,
        uncertainty: PositionUncertainty,
        correction_of: Option<RecordRef>,
    ) -> Result<Self, SpatialError> {
        for reference in [entity, world, frame.record, key.source(), evidence] {
            same_scope(id, reference)?;
        }
        same_source_scope(id, key.event())?;
        if source_time.role() != TimestampRole::SourceEvent
            || source_time.source().artifact() != evidence
        {
            return Err(SpatialError::InvalidObservation);
        }
        if let Some(previous) = correction_of {
            same_scope(id, previous)?;
            if previous == id {
                return Err(SpatialError::InvalidCorrection);
            }
        }
        Ok(Self {
            id,
            entity,
            world,
            frame,
            key,
            evidence,
            source_time,
            position,
            uncertainty,
            correction_of,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RecordRef {
        self.id
    }
    #[must_use]
    pub const fn entity(&self) -> RecordRef {
        self.entity
    }
    #[must_use]
    pub const fn world(&self) -> RecordRef {
        self.world
    }
    #[must_use]
    pub const fn frame(&self) -> VersionedRecordRef {
        self.frame
    }
    #[must_use]
    pub const fn key(&self) -> &ObservationKey {
        &self.key
    }
    #[must_use]
    pub const fn evidence(&self) -> RecordRef {
        self.evidence
    }
    #[must_use]
    pub const fn source_time(&self) -> &TimestampEnvelope {
        &self.source_time
    }
    #[must_use]
    pub const fn position(&self) -> SpatialPosition {
        self.position
    }
    #[must_use]
    pub const fn uncertainty(&self) -> PositionUncertainty {
        self.uncertainty
    }
    #[must_use]
    pub const fn correction_of(&self) -> Option<RecordRef> {
        self.correction_of
    }
}

/// Schema-level result distinction. Algorithms that produce estimates/simulations arrive later.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PositionKnowledge {
    Unknown,
    Observed { observation: RecordRef },
    Estimated { trajectory: RecordRef },
    Simulated { branch: RecordRef },
    Conflicting { observations: ObservationReferences },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationReferences(Vec<RecordRef>);

impl ObservationReferences {
    pub fn new(mut observations: Vec<RecordRef>) -> Result<Self, SpatialError> {
        if observations.len() > MAX_OBSERVATIONS_PER_RESULT {
            return Err(SpatialError::ResourceLimit);
        }
        observations.sort_unstable();
        observations.dedup();
        if observations.len() < 2 {
            return Err(SpatialError::InvalidObservation);
        }
        Ok(Self(observations))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[RecordRef] {
        &self.0
    }
}

impl PositionKnowledge {
    /// Unknown is a distinct state; it cannot carry a coordinate or fabricate an origin.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpatialRecord {
    World(WorldDefinition),
    Frame(FrameDefinition),
    Geometry(GeometryVersion),
    Observation(Box<PositionObservation>),
}

impl SpatialRecord {
    #[must_use]
    pub const fn id(&self) -> RecordRef {
        match self {
            Self::World(value) => value.id(),
            Self::Frame(value) => value.id(),
            Self::Geometry(value) => value.id(),
            Self::Observation(value) => value.id(),
        }
    }
}

/// Borrowed view of one canonical spatial record in deterministic catalog order.
#[derive(Clone, Copy)]
pub enum SpatialRecordRef<'a> {
    World(&'a WorldDefinition),
    Frame(&'a FrameDefinition),
    Geometry(&'a GeometryVersion),
    Observation(&'a PositionObservation),
}

impl<'a> From<&'a SpatialRecord> for SpatialRecordRef<'a> {
    fn from(value: &'a SpatialRecord) -> Self {
        match value {
            SpatialRecord::World(value) => Self::World(value),
            SpatialRecord::Frame(value) => Self::Frame(value),
            SpatialRecord::Geometry(value) => Self::Geometry(value),
            SpatialRecord::Observation(value) => Self::Observation(value),
        }
    }
}

impl SpatialRecordRef<'_> {
    #[must_use]
    pub const fn id(self) -> RecordRef {
        match self {
            Self::World(value) => value.id(),
            Self::Frame(value) => value.id(),
            Self::Geometry(value) => value.id(),
            Self::Observation(value) => value.id(),
        }
    }

    #[must_use]
    pub(crate) const fn order_key(self) -> (u8, RecordRef, u64) {
        match self {
            Self::World(value) => (0, value.id(), value.version().get()),
            Self::Frame(value) => (1, value.id(), value.version().get()),
            Self::Geometry(value) => (2, value.id(), value.version().get()),
            Self::Observation(value) => (3, value.id(), 0),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpatialError {
    InvalidVersion,
    VersionExhausted,
    VersionConflict,
    ScopeMismatch,
    MissingWorld,
    MissingFrame,
    MissingFrameVersion,
    MissingGeometryVersion,
    MissingObservation,
    InvalidRootFrame,
    InvalidPredecessor,
    InvalidSession,
    InvalidObservation,
    InvalidCorrection,
    IncompatibleFrame,
    FrameCycle,
    ResourceLimit,
    DuplicateRecord,
    SourceEventConflict,
    InvalidEncoding,
    UnsupportedProfile,
}

impl From<SpatialPrimitiveError> for SpatialError {
    fn from(error: SpatialPrimitiveError) -> Self {
        match error {
            SpatialPrimitiveError::InvalidVersion => Self::InvalidVersion,
            SpatialPrimitiveError::VersionExhausted => Self::VersionExhausted,
            SpatialPrimitiveError::InvalidLatitude
            | SpatialPrimitiveError::InvalidLongitude
            | SpatialPrimitiveError::InvalidBounds => Self::InvalidEncoding,
        }
    }
}

impl fmt::Display for SpatialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SpatialError {}

pub(crate) fn same_scope(left: RecordRef, right: RecordRef) -> Result<(), SpatialError> {
    if left.database() == right.database() && left.namespace() == right.namespace() {
        Ok(())
    } else {
        Err(SpatialError::ScopeMismatch)
    }
}

fn same_source_scope(left: RecordRef, right: SourceEventRef) -> Result<(), SpatialError> {
    if left.database() == right.database() && left.namespace() == right.namespace() {
        Ok(())
    } else {
        Err(SpatialError::ScopeMismatch)
    }
}
