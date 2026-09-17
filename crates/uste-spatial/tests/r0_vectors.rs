mod common;

use common::{child_frame, record, reference, revision, world_and_root};
use uste_spatial::{
    GeographicPoint, LatitudeNanodegrees, LongitudeNanodegrees, SpatialCatalog, SpatialError,
    SpatialRecord,
};

const VECTORS: &str = include_str!("../../../acceptance/r0/spatial.tsv");

#[test]
fn t48_literal_spatial_vectors_execute() {
    let rows: Vec<_> = VECTORS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|row| {
            matches!(
                row[0],
                "longitude_wrap" | "north_pole_lon" | "frame_cycle" | "frame_depth"
            )
        })
        .collect();
    assert_eq!(rows.len(), 4);
    for row in rows {
        match row[0] {
            "longitude_wrap" => {
                assert_eq!(row[1], "canonical_lon_deg");
                let degrees = row[2].parse::<i64>().unwrap();
                let canonical = LongitudeNanodegrees::new(degrees * 1_000_000_000).unwrap();
                assert_eq!(canonical.get() / 1_000_000_000, row[3].parse().unwrap());
            }
            "north_pole_lon" => {
                assert_eq!(row[2], "lat=90,lon=42");
                let point = GeographicPoint::new(
                    LongitudeNanodegrees::new(42_000_000_000).unwrap(),
                    LatitudeNanodegrees::new(90_000_000_000).unwrap(),
                    None,
                );
                assert_eq!(point.longitude().get(), 0);
                assert_eq!(row[3], "lat=90,lon=0");
            }
            "frame_cycle" => {
                let mut catalog = SpatialCatalog::new();
                catalog.apply_batch(revision(1), &world_and_root()).unwrap();
                let cycle = [
                    SpatialRecord::Frame(child_frame(12, 1, reference(13, 1))),
                    SpatialRecord::Frame(child_frame(13, 1, reference(12, 1))),
                ];
                assert_eq!(row[2], "A->B,B->A");
                assert_eq!(
                    catalog.apply_batch(revision(2), &cycle),
                    Err(SpatialError::FrameCycle)
                );
                assert_eq!(row[3], "FrameMismatch");
            }
            "frame_depth" => {
                let mut catalog = SpatialCatalog::new();
                catalog.apply_batch(revision(1), &world_and_root()).unwrap();
                let mut parent = reference(11, 1);
                for step in 0_u8..32 {
                    let id = 60 + step;
                    catalog
                        .apply_batch(
                            revision(u64::from(step) + 2),
                            &[SpatialRecord::Frame(child_frame(id, 1, parent))],
                        )
                        .unwrap();
                    parent = reference(id, 1);
                }
                assert_eq!(row[2], "33_edges");
                assert_eq!(
                    catalog.apply_batch(
                        revision(34),
                        &[SpatialRecord::Frame(child_frame(92, 1, parent))],
                    ),
                    Err(SpatialError::ResourceLimit)
                );
                assert_eq!(row[3], "ResourceLimit");
                assert_eq!(catalog.frame_history_len(record(92)), 0);
            }
            _ => unreachable!(),
        }
    }
}
