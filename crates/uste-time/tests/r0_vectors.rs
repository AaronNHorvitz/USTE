use uste_time::{
    ClockUncertainty, ResolutionStatus, SourceDescriptor, TimeInput, TimeNormalizer, TimestampRole,
    format_utc,
};
use uste_types::{DatabaseId, NamespaceId, RecordId, RecordRef, UtcInstant};

const VECTORS: &str = include_str!("../../../acceptance/r0/time.tsv");

fn source() -> SourceDescriptor {
    SourceDescriptor::new(
        RecordRef::new(
            DatabaseId::from_bytes([0x10; 16]),
            NamespaceId::from_bytes([0x20; 16]),
            RecordId::from_bytes([0x30; 16]),
        ),
        1,
        "acceptance/r0/time.tsv",
    )
    .unwrap()
}

#[test]
fn every_r0_point_normalization_vector_runs_against_production() {
    let normalizer = TimeNormalizer::posix_utc_v1().unwrap();
    let rows: Vec<Vec<&str>> = VECTORS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    let mut executed = 0;
    for row in rows.iter().filter(|row| row[1] != "interval") {
        match row[1] {
            "decode_pair" => {
                let (seconds, nanos) = row[2].split_once(',').unwrap();
                let instant =
                    UtcInstant::new(seconds.parse().unwrap(), nanos.parse().unwrap()).unwrap();
                assert_eq!(format_utc(instant).unwrap(), row[3], "{}", row[0]);
            }
            "normalize" => {
                let input = match row[0] {
                    "naive" => TimeInput::Local {
                        text: row[2],
                        zone: None,
                        offset_seconds: None,
                        fold: None,
                    },
                    "numeric_no_unit" => TimeInput::Numeric {
                        token: row[2],
                        value: row[2].parse().unwrap(),
                        unit: None,
                        scale: uste_time::TimeScale::PosixUtc,
                    },
                    "fold_without_choice" | "gap" => {
                        let (text, zone) = row[2].split_once('[').unwrap();
                        TimeInput::Local {
                            text,
                            zone: Some(zone.strip_suffix(']').unwrap()),
                            offset_seconds: None,
                            fold: None,
                        }
                    }
                    _ => TimeInput::Rfc3339(row[2]),
                };
                let envelope = normalizer
                    .normalize(
                        source(),
                        TimestampRole::SourceEvent,
                        input,
                        ClockUncertainty::Unknown,
                        Vec::new(),
                    )
                    .unwrap();
                match row[3] {
                    "UnsupportedTimeScale" => {
                        assert_eq!(envelope.status(), ResolutionStatus::Unsupported)
                    }
                    "AmbiguousTime" => {
                        assert_eq!(envelope.status(), ResolutionStatus::Ambiguous)
                    }
                    "InvalidTime" => assert_eq!(envelope.status(), ResolutionStatus::Invalid),
                    expected => assert_eq!(
                        format_utc(envelope.resolution().instant().unwrap()).unwrap(),
                        expected,
                        "{}",
                        row[0]
                    ),
                }
            }
            operation => panic!("unknown operation {operation}"),
        }
        executed += 1;
    }
    assert_eq!(executed, 10);
}
