use std::collections::BTreeMap;

use uste_time::{envelope_from_value, envelope_to_value};
use uste_types::{
    BoundedBytes, BoundedString, CanonicalMap, DatabaseId, NamespaceId, RecordRef, SourceEventId,
    SourceEventRef, Value, decode_value, encode_value,
    spatial::{
        CoordinateSystem, GeographicBox, GeographicPoint, LatitudeNanodegrees, LocalBox2,
        LocalBox3, LocalPoint2, LocalPoint3, LongitudeNanodegrees, Nanometres,
        NonNegativeNanometres, SpatialPosition, SpatialVersion, VersionedRecordRef,
    },
};

use crate::model::{
    FrameDefinition, FrameParent, Geometry, GeometryVersion, ObservationKey, PositionObservation,
    PositionUncertainty, SPATIAL_PROFILE, SpatialError, SpatialRecord, SpatialRecordRef,
    WorldDefinition,
};

const SCHEMA_VERSION: u128 = 1;

pub fn encode_record(record: &SpatialRecord) -> Result<Vec<u8>, SpatialError> {
    encode_record_ref(record.into())
}

pub(crate) fn encode_record_ref(record: SpatialRecordRef<'_>) -> Result<Vec<u8>, SpatialError> {
    encode_value(&record_ref_to_value(record)?).map_err(|error| match error {
        uste_types::EncodeError::ResourceLimit => SpatialError::ResourceLimit,
        _ => SpatialError::InvalidEncoding,
    })
}

pub fn decode_record(input: &[u8]) -> Result<SpatialRecord, SpatialError> {
    record_from_value(decode_value(input).map_err(|_| SpatialError::InvalidEncoding)?)
}

pub fn record_to_value(record: &SpatialRecord) -> Result<Value, SpatialError> {
    record_ref_to_value(record.into())
}

fn record_ref_to_value(record: SpatialRecordRef<'_>) -> Result<Value, SpatialError> {
    let (kind, body) = match record {
        SpatialRecordRef::World(value) => ("world", encode_world(value)?),
        SpatialRecordRef::Frame(value) => ("frame", encode_frame(value)?),
        SpatialRecordRef::Geometry(value) => ("geometry", encode_geometry_version(value)?),
        SpatialRecordRef::Observation(value) => ("observation", encode_observation(value)?),
    };
    map([
        ("body", body),
        ("kind", text(kind)?),
        ("profile", text(SPATIAL_PROFILE)?),
        ("schema_version", Value::Unsigned(SCHEMA_VERSION)),
    ])
}

pub fn record_from_value(value: Value) -> Result<SpatialRecord, SpatialError> {
    let mut fields = Fields::new(value)?;
    let body = fields.take("body")?;
    let kind = take_text(fields.take("kind")?)?;
    expect_text(fields.take("profile")?, SPATIAL_PROFILE)?;
    if take_u128(fields.take("schema_version")?)? != SCHEMA_VERSION {
        return Err(SpatialError::UnsupportedProfile);
    }
    fields.finish()?;
    match kind.as_str() {
        "world" => decode_world(body).map(SpatialRecord::World),
        "frame" => decode_frame(body).map(SpatialRecord::Frame),
        "geometry" => decode_geometry_version(body).map(SpatialRecord::Geometry),
        "observation" => decode_observation(body)
            .map(Box::new)
            .map(SpatialRecord::Observation),
        _ => Err(SpatialError::UnsupportedProfile),
    }
}

fn encode_world(value: &WorldDefinition) -> Result<Value, SpatialError> {
    map([
        ("id", Value::RecordRef(value.id())),
        ("root_frame", encode_versioned(value.root_frame())?),
        ("version", encode_version(value.version())),
    ])
}

fn decode_world(value: Value) -> Result<WorldDefinition, SpatialError> {
    let mut fields = Fields::new(value)?;
    let id = take_record(fields.take("id")?)?;
    let root = decode_versioned(fields.take("root_frame")?)?;
    let version = decode_version(fields.take("version")?)?;
    fields.finish()?;
    WorldDefinition::new(id, version, root)
}

fn encode_frame(value: &FrameDefinition) -> Result<Value, SpatialError> {
    map([
        ("coordinate_system", text(coordinate_name(value.kind()))?),
        ("id", Value::RecordRef(value.id())),
        (
            "parent",
            value.parent().map_or(Ok(Value::Null), encode_parent)?,
        ),
        ("version", encode_version(value.version())),
        ("world", Value::RecordRef(value.world())),
    ])
}

fn decode_frame(value: Value) -> Result<FrameDefinition, SpatialError> {
    let mut fields = Fields::new(value)?;
    let kind = decode_coordinate(&take_text(fields.take("coordinate_system")?)?)?;
    let id = take_record(fields.take("id")?)?;
    let parent = match fields.take("parent")? {
        Value::Null => None,
        value => Some(decode_parent(value)?),
    };
    let version = decode_version(fields.take("version")?)?;
    let world = take_record(fields.take("world")?)?;
    fields.finish()?;
    FrameDefinition::new(id, version, world, kind, parent)
}

fn encode_parent(value: FrameParent) -> Result<Value, SpatialError> {
    map([
        ("frame", encode_versioned(value.frame)?),
        ("transform", encode_versioned(value.transform)?),
    ])
}

fn decode_parent(value: Value) -> Result<FrameParent, SpatialError> {
    let mut fields = Fields::new(value)?;
    let frame = decode_versioned(fields.take("frame")?)?;
    let transform = decode_versioned(fields.take("transform")?)?;
    fields.finish()?;
    Ok(FrameParent { frame, transform })
}

fn encode_geometry_version(value: &GeometryVersion) -> Result<Value, SpatialError> {
    map([
        ("frame", encode_versioned(value.frame())?),
        ("geometry", encode_geometry(value.geometry())?),
        ("id", Value::RecordRef(value.id())),
        (
            "predecessor",
            value
                .predecessor()
                .map_or(Ok(Value::Null), encode_versioned)?,
        ),
        ("precision", encode_uncertainty(value.precision())?),
        ("version", encode_version(value.version())),
        ("world", Value::RecordRef(value.world())),
    ])
}

fn decode_geometry_version(value: Value) -> Result<GeometryVersion, SpatialError> {
    let mut fields = Fields::new(value)?;
    let frame = decode_versioned(fields.take("frame")?)?;
    let geometry = decode_geometry(fields.take("geometry")?)?;
    let id = take_record(fields.take("id")?)?;
    let predecessor = match fields.take("predecessor")? {
        Value::Null => None,
        value => Some(decode_versioned(value)?),
    };
    let precision = decode_uncertainty(fields.take("precision")?)?;
    let version = decode_version(fields.take("version")?)?;
    let world = take_record(fields.take("world")?)?;
    fields.finish()?;
    GeometryVersion::new(id, version, world, frame, geometry, precision, predecessor)
}

fn encode_observation(value: &PositionObservation) -> Result<Value, SpatialError> {
    map([
        (
            "correction_of",
            value.correction_of().map_or(Value::Null, Value::RecordRef),
        ),
        ("entity", Value::RecordRef(value.entity())),
        ("evidence", Value::RecordRef(value.evidence())),
        ("frame", encode_versioned(value.frame())?),
        ("id", Value::RecordRef(value.id())),
        ("key", encode_key(value.key())?),
        ("position", encode_position(value.position())?),
        (
            "source_time",
            envelope_to_value(value.source_time()).map_err(|_| SpatialError::InvalidObservation)?,
        ),
        ("uncertainty", encode_uncertainty(value.uncertainty())?),
        ("world", Value::RecordRef(value.world())),
    ])
}

fn decode_observation(value: Value) -> Result<PositionObservation, SpatialError> {
    let mut fields = Fields::new(value)?;
    let correction = take_optional_record(fields.take("correction_of")?)?;
    let entity = take_record(fields.take("entity")?)?;
    let evidence = take_record(fields.take("evidence")?)?;
    let frame = decode_versioned(fields.take("frame")?)?;
    let id = take_record(fields.take("id")?)?;
    let key = decode_key(fields.take("key")?)?;
    let position = decode_position(fields.take("position")?)?;
    let source_time = envelope_from_value(fields.take("source_time")?)
        .map_err(|_| SpatialError::InvalidObservation)?;
    let uncertainty = decode_uncertainty(fields.take("uncertainty")?)?;
    let world = take_record(fields.take("world")?)?;
    fields.finish()?;
    PositionObservation::new(
        id,
        entity,
        world,
        frame,
        key,
        evidence,
        source_time,
        position,
        uncertainty,
        correction,
    )
}

fn encode_key(value: &ObservationKey) -> Result<Value, SpatialError> {
    map([
        ("event", encode_source_event(value.event())?),
        ("session", text(value.session())?),
        ("source", Value::RecordRef(value.source())),
    ])
}

fn decode_key(value: Value) -> Result<ObservationKey, SpatialError> {
    let mut fields = Fields::new(value)?;
    let event = decode_source_event(fields.take("event")?)?;
    let session = take_text(fields.take("session")?)?;
    let source = take_record(fields.take("source")?)?;
    fields.finish()?;
    ObservationKey::new(source, session, event)
}

fn encode_source_event(value: SourceEventRef) -> Result<Value, SpatialError> {
    map([
        ("database", bytes(value.database().as_bytes())?),
        ("event", bytes(value.source_event().as_bytes())?),
        ("namespace", bytes(value.namespace().as_bytes())?),
    ])
}

fn decode_source_event(value: Value) -> Result<SourceEventRef, SpatialError> {
    let mut fields = Fields::new(value)?;
    let database = DatabaseId::from_bytes(take_id(fields.take("database")?)?);
    let event = SourceEventId::from_bytes(take_id(fields.take("event")?)?);
    let namespace = NamespaceId::from_bytes(take_id(fields.take("namespace")?)?);
    fields.finish()?;
    Ok(SourceEventRef::new(database, namespace, event))
}

fn encode_geometry(value: Geometry) -> Result<Value, SpatialError> {
    match value {
        Geometry::LocalPoint2(value) => encode_local2("local_point_2", value),
        Geometry::LocalPoint3(value) => encode_local3("local_point_3", value),
        Geometry::GeographicPoint(value) => encode_geo_point("geographic_point", value),
        Geometry::LocalBox2(value) => map([
            ("kind", text("local_box_2")?),
            ("max", encode_local2("point", value.max())?),
            ("min", encode_local2("point", value.min())?),
        ]),
        Geometry::LocalBox3(value) => map([
            ("kind", text("local_box_3")?),
            ("max", encode_local3("point", value.max())?),
            ("min", encode_local3("point", value.min())?),
        ]),
        Geometry::GeographicBox(value) => map([
            ("east", Value::Signed(i128::from(value.east().get()))),
            ("kind", text("geographic_box")?),
            ("north", Value::Signed(i128::from(value.north().get()))),
            ("south", Value::Signed(i128::from(value.south().get()))),
            ("west", Value::Signed(i128::from(value.west().get()))),
        ]),
    }
}

fn decode_geometry(value: Value) -> Result<Geometry, SpatialError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    match kind.as_str() {
        "local_point_2" => decode_local2_fields(fields).map(Geometry::LocalPoint2),
        "local_point_3" => decode_local3_fields(fields).map(Geometry::LocalPoint3),
        "geographic_point" => decode_geo_fields(fields).map(Geometry::GeographicPoint),
        "local_box_2" => {
            let max = decode_local2(fields.take("max")?)?;
            let min = decode_local2(fields.take("min")?)?;
            fields.finish()?;
            LocalBox2::new(min, max)
                .map(Geometry::LocalBox2)
                .map_err(Into::into)
        }
        "local_box_3" => {
            let max = decode_local3(fields.take("max")?)?;
            let min = decode_local3(fields.take("min")?)?;
            fields.finish()?;
            LocalBox3::new(min, max)
                .map(Geometry::LocalBox3)
                .map_err(Into::into)
        }
        "geographic_box" => {
            let east = longitude(fields.take("east")?)?;
            let north = latitude(fields.take("north")?)?;
            let south = latitude(fields.take("south")?)?;
            let west = longitude(fields.take("west")?)?;
            fields.finish()?;
            GeographicBox::new(west, east, south, north)
                .map(Geometry::GeographicBox)
                .map_err(Into::into)
        }
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn encode_position(value: SpatialPosition) -> Result<Value, SpatialError> {
    match value {
        SpatialPosition::Local2(value) => encode_local2("local_2", value),
        SpatialPosition::Local3(value) => encode_local3("local_3", value),
        SpatialPosition::Geographic(value) => encode_geo_point("geographic", value),
    }
}

fn decode_position(value: Value) -> Result<SpatialPosition, SpatialError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    match kind.as_str() {
        "local_2" => decode_local2_fields(fields).map(SpatialPosition::Local2),
        "local_3" => decode_local3_fields(fields).map(SpatialPosition::Local3),
        "geographic" => decode_geo_fields(fields).map(SpatialPosition::Geographic),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn encode_local2(kind: &str, value: LocalPoint2) -> Result<Value, SpatialError> {
    map([
        ("kind", text(kind)?),
        ("x", Value::Signed(value.x.get())),
        ("y", Value::Signed(value.y.get())),
    ])
}

fn decode_local2(value: Value) -> Result<LocalPoint2, SpatialError> {
    let mut fields = Fields::new(value)?;
    expect_text(fields.take("kind")?, "point")?;
    decode_local2_fields(fields)
}

fn decode_local2_fields(mut fields: Fields) -> Result<LocalPoint2, SpatialError> {
    let x = Nanometres::new(take_i128(fields.take("x")?)?);
    let y = Nanometres::new(take_i128(fields.take("y")?)?);
    fields.finish()?;
    Ok(LocalPoint2 { x, y })
}

fn encode_local3(kind: &str, value: LocalPoint3) -> Result<Value, SpatialError> {
    map([
        ("kind", text(kind)?),
        ("x", Value::Signed(value.x.get())),
        ("y", Value::Signed(value.y.get())),
        ("z", Value::Signed(value.z.get())),
    ])
}

fn decode_local3(value: Value) -> Result<LocalPoint3, SpatialError> {
    let mut fields = Fields::new(value)?;
    expect_text(fields.take("kind")?, "point")?;
    decode_local3_fields(fields)
}

fn decode_local3_fields(mut fields: Fields) -> Result<LocalPoint3, SpatialError> {
    let x = Nanometres::new(take_i128(fields.take("x")?)?);
    let y = Nanometres::new(take_i128(fields.take("y")?)?);
    let z = Nanometres::new(take_i128(fields.take("z")?)?);
    fields.finish()?;
    Ok(LocalPoint3 { x, y, z })
}

fn encode_geo_point(kind: &str, value: GeographicPoint) -> Result<Value, SpatialError> {
    map([
        (
            "height",
            value
                .height()
                .map_or(Value::Null, |height| Value::Signed(height.get())),
        ),
        ("kind", text(kind)?),
        (
            "latitude",
            Value::Signed(i128::from(value.latitude().get())),
        ),
        (
            "longitude",
            Value::Signed(i128::from(value.longitude().get())),
        ),
    ])
}

fn decode_geo_fields(mut fields: Fields) -> Result<GeographicPoint, SpatialError> {
    let height = match fields.take("height")? {
        Value::Null => None,
        value => Some(Nanometres::new(take_i128(value)?)),
    };
    let latitude = latitude(fields.take("latitude")?)?;
    let longitude = longitude(fields.take("longitude")?)?;
    fields.finish()?;
    let point = GeographicPoint::new(longitude, latitude, height);
    if point.longitude() != longitude {
        return Err(SpatialError::InvalidEncoding);
    }
    Ok(point)
}

fn encode_uncertainty(value: PositionUncertainty) -> Result<Value, SpatialError> {
    match value {
        PositionUncertainty::Unknown => map([
            ("kind", text("unknown")?),
            ("radial_nanometres", Value::Null),
        ]),
        PositionUncertainty::Radial(value) => map([
            ("kind", text("radial")?),
            ("radial_nanometres", Value::Unsigned(value.get())),
        ]),
    }
}

fn decode_uncertainty(value: Value) -> Result<PositionUncertainty, SpatialError> {
    let mut fields = Fields::new(value)?;
    let kind = take_text(fields.take("kind")?)?;
    let radial = fields.take("radial_nanometres")?;
    fields.finish()?;
    match (kind.as_str(), radial) {
        ("unknown", Value::Null) => Ok(PositionUncertainty::Unknown),
        ("radial", Value::Unsigned(value)) => Ok(PositionUncertainty::Radial(
            NonNegativeNanometres::new(value),
        )),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn encode_versioned(value: VersionedRecordRef) -> Result<Value, SpatialError> {
    map([
        ("record", Value::RecordRef(value.record)),
        ("version", encode_version(value.version)),
    ])
}

fn decode_versioned(value: Value) -> Result<VersionedRecordRef, SpatialError> {
    let mut fields = Fields::new(value)?;
    let record = take_record(fields.take("record")?)?;
    let version = decode_version(fields.take("version")?)?;
    fields.finish()?;
    Ok(VersionedRecordRef::new(record, version))
}

fn encode_version(value: SpatialVersion) -> Value {
    Value::Unsigned(u128::from(value.get()))
}

fn decode_version(value: Value) -> Result<SpatialVersion, SpatialError> {
    SpatialVersion::new(take_u64(value)?).map_err(Into::into)
}

fn coordinate_name(value: CoordinateSystem) -> &'static str {
    match value {
        CoordinateSystem::GeographicWgs84 => "wgs84_lon_lat",
        CoordinateSystem::LocalCartesian2 => "local_cartesian_2",
        CoordinateSystem::LocalCartesian3 => "local_cartesian_3",
    }
}

fn decode_coordinate(value: &str) -> Result<CoordinateSystem, SpatialError> {
    match value {
        "wgs84_lon_lat" => Ok(CoordinateSystem::GeographicWgs84),
        "local_cartesian_2" => Ok(CoordinateSystem::LocalCartesian2),
        "local_cartesian_3" => Ok(CoordinateSystem::LocalCartesian3),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn longitude(value: Value) -> Result<LongitudeNanodegrees, SpatialError> {
    let value = i64::try_from(take_i128(value)?).map_err(|_| SpatialError::InvalidEncoding)?;
    let canonical = LongitudeNanodegrees::new(value).map_err(SpatialError::from)?;
    if canonical.get() != value {
        return Err(SpatialError::InvalidEncoding);
    }
    Ok(canonical)
}

fn latitude(value: Value) -> Result<LatitudeNanodegrees, SpatialError> {
    let value = i64::try_from(take_i128(value)?).map_err(|_| SpatialError::InvalidEncoding)?;
    LatitudeNanodegrees::new(value).map_err(Into::into)
}

fn take_record(value: Value) -> Result<RecordRef, SpatialError> {
    match value {
        Value::RecordRef(value) => Ok(value),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn take_optional_record(value: Value) -> Result<Option<RecordRef>, SpatialError> {
    match value {
        Value::Null => Ok(None),
        Value::RecordRef(value) => Ok(Some(value)),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn take_text(value: Value) -> Result<String, SpatialError> {
    match value {
        Value::String(value) => Ok(value.into_string()),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn take_u128(value: Value) -> Result<u128, SpatialError> {
    match value {
        Value::Unsigned(value) => Ok(value),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn take_u64(value: Value) -> Result<u64, SpatialError> {
    u64::try_from(take_u128(value)?).map_err(|_| SpatialError::InvalidEncoding)
}

fn take_i128(value: Value) -> Result<i128, SpatialError> {
    match value {
        Value::Signed(value) => Ok(value),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn take_id(value: Value) -> Result<[u8; 16], SpatialError> {
    match value {
        Value::Bytes(value) => value
            .as_slice()
            .try_into()
            .map_err(|_| SpatialError::InvalidEncoding),
        _ => Err(SpatialError::InvalidEncoding),
    }
}

fn expect_text(value: Value, expected: &str) -> Result<(), SpatialError> {
    if take_text(value)? == expected {
        Ok(())
    } else {
        Err(SpatialError::UnsupportedProfile)
    }
}

fn text(value: &str) -> Result<Value, SpatialError> {
    BoundedString::new(value.to_owned())
        .map(Value::String)
        .map_err(|_| SpatialError::ResourceLimit)
}

fn bytes(value: &[u8]) -> Result<Value, SpatialError> {
    BoundedBytes::new(value.to_vec())
        .map(Value::Bytes)
        .map_err(|_| SpatialError::ResourceLimit)
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Result<Value, SpatialError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(N)
        .map_err(|_| SpatialError::ResourceLimit)?;
    for (key, value) in entries {
        values.push((
            BoundedString::new(key.to_owned()).map_err(|_| SpatialError::ResourceLimit)?,
            value,
        ));
    }
    CanonicalMap::new(values)
        .map(Value::Map)
        .map_err(|_| SpatialError::ResourceLimit)
}

struct Fields(BTreeMap<String, Value>);

impl Fields {
    fn new(value: Value) -> Result<Self, SpatialError> {
        let Value::Map(map) = value else {
            return Err(SpatialError::InvalidEncoding);
        };
        let mut fields = BTreeMap::new();
        for (key, value) in map.into_vec() {
            fields.insert(key.into_string(), value);
        }
        Ok(Self(fields))
    }

    fn take(&mut self, name: &str) -> Result<Value, SpatialError> {
        self.0.remove(name).ok_or(SpatialError::InvalidEncoding)
    }

    fn finish(self) -> Result<(), SpatialError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(SpatialError::InvalidEncoding)
        }
    }
}
