use uste_types::{
    DecodeError, decode_value,
    spatial::{
        AxisOrder, CoordinateProfile, CoordinateSystem, GeographicBox, GeographicPoint, Handedness,
        LatitudeNanodegrees, LengthUnit, LocalBox2, LocalPoint2, LongitudeNanodegrees, Nanometres,
        SpatialPrimitiveError, SpatialVersion,
    },
};

#[test]
fn longitude_latitude_and_poles_are_canonical() {
    let west = LongitudeNanodegrees::new(-180_000_000_000).unwrap();
    let east_alias = LongitudeNanodegrees::new(180_000_000_000).unwrap();
    assert_eq!(west, east_alias);
    assert_eq!(east_alias.get(), -180_000_000_000);
    assert_eq!(
        LongitudeNanodegrees::new(180_000_000_001),
        Err(SpatialPrimitiveError::InvalidLongitude)
    );

    let north = LatitudeNanodegrees::new(90_000_000_000).unwrap();
    assert_eq!(
        LatitudeNanodegrees::new(90_000_000_001),
        Err(SpatialPrimitiveError::InvalidLatitude)
    );
    let pole = GeographicPoint::new(
        LongitudeNanodegrees::new(42_000_000_000).unwrap(),
        north,
        None,
    );
    assert_eq!(pole.longitude().get(), 0);
    assert_eq!(pole.height(), None);
    let zero_height = GeographicPoint::new(east_alias, north, Some(Nanometres::new(0)));
    assert_ne!(pole, zero_height);
}

#[test]
fn box_and_coordinate_profiles_close_invalid_unit_combinations() {
    let zero = Nanometres::new(0);
    let ten = Nanometres::new(10);
    let degenerate = LocalBox2::new(
        LocalPoint2 { x: zero, y: zero },
        LocalPoint2 { x: zero, y: zero },
    );
    assert!(degenerate.is_ok());
    assert_eq!(
        LocalBox2::new(
            LocalPoint2 { x: ten, y: zero },
            LocalPoint2 { x: zero, y: ten },
        ),
        Err(SpatialPrimitiveError::InvalidBounds)
    );

    let ten_degrees = LongitudeNanodegrees::new(10_000_000_000).unwrap();
    let zero_degrees = LatitudeNanodegrees::new(0).unwrap();
    let meridian = GeographicBox::new(ten_degrees, ten_degrees, zero_degrees, zero_degrees);
    assert!(
        meridian.is_ok(),
        "equal longitudes are zero width, not the world"
    );
    let wrapped = GeographicBox::new(
        LongitudeNanodegrees::new(170_000_000_000).unwrap(),
        LongitudeNanodegrees::new(-170_000_000_000).unwrap(),
        LatitudeNanodegrees::new(-1).unwrap(),
        LatitudeNanodegrees::new(1).unwrap(),
    );
    assert!(wrapped.is_ok());

    let local = CoordinateProfile::new(CoordinateSystem::LocalCartesian2);
    assert_eq!(local.axis_order(), AxisOrder::Xy);
    assert_eq!(local.handedness(), Some(Handedness::Right));
    assert_eq!(local.length_unit(), LengthUnit::Nanometre);
    assert_eq!(local.angular_unit(), None);
}

#[test]
fn generic_wire_has_no_float_or_nonfinite_value_tag() {
    let bytes = [b'U', b'S', b'T', b'E', 1, 1, 0, 1, 0x0b];
    assert_eq!(
        decode_value(&bytes),
        Err(DecodeError::InvalidValueTag(0x0b))
    );
}

#[test]
fn spatial_versions_are_nonzero_and_never_wrap() {
    assert_eq!(
        SpatialVersion::new(0),
        Err(SpatialPrimitiveError::InvalidVersion)
    );
    assert_eq!(
        SpatialVersion::new(u64::MAX).unwrap().checked_next(),
        Err(SpatialPrimitiveError::VersionExhausted)
    );
}
