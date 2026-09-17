//! Typed, capability-free spatial/world schemas and version-reference validation.
//!
//! This R1 kernel stores fixed-point truth and validates reference history. It deliberately does
//! not perform frame-transform binding validation/evaluation, external graph reference closure,
//! distance predicates, interpolation or spatial indexing; T-49/T-50 and later layers own them.

#![forbid(unsafe_code)]

mod catalog;
mod codec;
mod model;
mod state;

pub use catalog::{ObservationOutcome, SpatialCatalog};
pub use codec::{decode_record, encode_record, record_from_value, record_to_value};
pub use model::{
    FrameDefinition, FrameKind, FrameParent, Geometry, GeometryVersion,
    MAX_OBSERVATIONS_PER_RESULT, MAX_SESSION_BYTES, MAX_SPATIAL_BATCH_RECORDS,
    MAX_SPATIAL_CATALOG_ENTRIES, MAX_SPATIAL_CATALOG_LOGICAL_BYTES, ObservationKey,
    ObservationReferences, PositionKnowledge, PositionObservation, PositionUncertainty,
    SPATIAL_PROFILE, SpatialError, SpatialRecord, SpatialRecordRef, WorldDefinition,
};
pub use state::{
    SpatialSnapshot, SpatialState, SpatialTransaction, decode_transaction, encode_transaction,
};
pub use uste_types::spatial::{
    AngularUnit, AxisOrder, CoordinateProfile, CoordinateSystem, GeographicBox, GeographicPoint,
    Handedness, LatitudeNanodegrees, LengthUnit, LocalBox2, LocalBox3, LocalPoint2, LocalPoint3,
    LongitudeNanodegrees, MAX_FRAME_DEPTH, Nanometres, NonNegativeNanometres, SPACE_PROFILE,
    SpatialPosition, SpatialPrimitiveError, SpatialVersion, VersionedRecordRef, VerticalReference,
};
