use uste_graph::{IntervalBound, ValidTime};
use uste_types::UtcInstant;

const VECTORS: &str = include_str!("../../../acceptance/r0/time.tsv");

#[test]
fn r0_unknown_and_unbounded_intervals_remain_distinct() {
    let rows: Vec<Vec<&str>> = VECTORS
        .lines()
        .skip(1)
        .filter(|line| line.split('\t').nth(1) == Some("interval"))
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ["unknown", "interval", "unknown", "Unknown"]);
    let unknown = ValidTime::Unknown;
    assert_eq!(unknown.contains(UtcInstant::new(0, 0).unwrap()), None);

    assert_eq!(rows[1][0], "unbounded");
    assert_eq!(rows[1][3], "Valid");
    let end = UtcInstant::new(1_789_603_200, 0).unwrap();
    let interval = ValidTime::HalfOpen {
        start: IntervalBound::Unbounded,
        end: IntervalBound::Bounded(end),
    };
    assert_eq!(
        interval.contains(UtcInstant::new(-1, 0).unwrap()),
        Some(true)
    );
    assert_eq!(interval.contains(end), Some(false));
}
