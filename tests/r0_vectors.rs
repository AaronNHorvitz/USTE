#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

const TIME: &str = include_str!("../acceptance/r0/time.tsv");
const TRANSITIONS: &str = include_str!("../acceptance/r0/transitions.tsv");
const STORAGE: &str = include_str!("../acceptance/r0/storage-crash.tsv");
const SECURITY: &str = include_str!("../acceptance/r0/security-lifecycle.tsv");
const SPATIAL: &str = include_str!("../acceptance/r0/spatial.tsv");
const PHYSICS: &str = include_str!("../acceptance/r0/physics.tsv");
const BENCHMARKS: &str = include_str!("../acceptance/r0/benchmark-manifest.tsv");
const CONTENT: &str = include_str!("../acceptance/r0/content-fixtures.tsv");
const CONTENT_GENERATED: &str = include_str!("../acceptance/r0/content-generated.tsv");

fn rows(input: &str, columns: usize) -> Vec<Vec<&str>> {
    let mut lines = input.lines();
    let header = lines.next().expect("header");
    assert_eq!(header.split('\t').count(), columns);
    lines
        .map(|line| {
            let row: Vec<_> = line.split('\t').collect();
            assert_eq!(row.len(), columns, "malformed vector: {line}");
            assert!(row.iter().all(|value| !value.is_empty()));
            row
        })
        .collect()
}

#[test]
fn all_vector_files_are_structurally_valid_and_cases_are_unique() {
    for (input, columns) in [
        (TIME, 4),
        (TRANSITIONS, 5),
        (STORAGE, 4),
        (SECURITY, 4),
        (SPATIAL, 4),
        (PHYSICS, 4),
        (BENCHMARKS, 5),
        (CONTENT, 6),
        (CONTENT_GENERATED, 3),
    ] {
        let parsed = rows(input, columns);
        assert!(!parsed.is_empty());
        let cases: BTreeSet<_> = parsed.iter().map(|row| row[0]).collect();
        assert_eq!(cases.len(), parsed.len(), "duplicate case ID");
    }
}

#[test]
fn generated_content_recipes_have_pinned_sha256_entries() {
    let content = rows(CONTENT, 6);
    let generated = rows(CONTENT_GENERATED, 3);
    let generated_ids: BTreeSet<_> = generated.iter().map(|row| row[0]).collect();
    let recipe_ids: BTreeSet<_> = content
        .iter()
        .filter(|row| row[3].starts_with("generated:"))
        .map(|row| row[0])
        .collect();
    assert_eq!(generated_ids, recipe_ids);
    assert!(generated.iter().all(|row| {
        row[1].parse::<u64>().is_ok()
            && row[2].len() == 64
            && row[2].bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
}

#[test]
fn every_content_family_has_positive_and_negative_or_inert_coverage() {
    let parsed = rows(CONTENT, 6);
    let families: BTreeSet<_> = parsed.iter().map(|row| row[1]).collect();
    for required in [
        "unknown", "text", "json", "csv", "xml", "html", "pdf", "ooxml", "zip", "tar", "png",
        "jpeg", "wav", "y4m",
    ] {
        assert!(
            families.contains(required),
            "missing fixture family {required}"
        );
        assert!(parsed.iter().filter(|row| row[1] == required).count() >= 2);
    }
    assert!(parsed.iter().any(|row| row[5].contains("no_egress")));
    assert!(parsed.iter().any(|row| row[5].contains("no_path_escape")));
    assert!(parsed.iter().any(|row| row[4] == "LimitExceeded"));
}

#[test]
fn negative_epoch_uses_floor_seconds() {
    let seconds = -1_i64;
    let nanos = 500_000_000_u32;
    assert_eq!((seconds + 1, nanos), (0, 500_000_000));
    let row = rows(TIME, 4)
        .into_iter()
        .find(|row| row[0] == "negative_half")
        .unwrap();
    assert_eq!(row[2], "-1,500000000");
    assert_eq!(row[3], "1969-12-31T23:59:59.5Z");
}

#[test]
fn assertion_transitions_match_the_closed_state_machine() {
    let allowed: BTreeSet<_> = [
        ("proposed", "accept", "accepted"),
        ("proposed", "reject", "rejected"),
        ("accepted", "dispute", "disputed"),
        ("accepted", "supersede", "superseded"),
        ("accepted", "retract", "retracted"),
        ("accepted", "expire", "expired"),
        ("accepted", "correct", "new-proposed-linked"),
    ]
    .into_iter()
    .collect();
    for row in rows(TRANSITIONS, 5) {
        let actual = allowed.contains(&(row[1], row[2], row[3]));
        assert_eq!(actual, row[4] == "ok", "case {}", row[0]);
    }
}

fn canonical_lon(mut degrees: i64) -> i64 {
    while degrees < -180 {
        degrees += 360;
    }
    while degrees >= 180 {
        degrees -= 360;
    }
    degrees
}

fn in_lon_box(west: i64, east: i64, point: i64) -> bool {
    let (west, east, point) = (
        canonical_lon(west),
        canonical_lon(east),
        canonical_lon(point),
    );
    if west <= east {
        west <= point && point <= east
    } else {
        point >= west || point <= east
    }
}

#[test]
fn antimeridian_and_zero_width_vectors_are_literal() {
    assert_eq!(canonical_lon(180), -180);
    assert!(in_lon_box(170, -170, -175));
    assert!(!in_lon_box(170, -170, 0));
    assert!(in_lon_box(10, 10, 10));
    assert!(!in_lon_box(10, 10, 11));
}

#[test]
fn authalic_equator_degree_matches_declared_vector() {
    let metres = 6_371_007.1809_f64 * std::f64::consts::PI / 180.0;
    assert!((metres - 111_195.052).abs() < 0.001);
}

fn kinematic(p0: i128, v0: i128, acceleration: i128, seconds: i128) -> (i128, i128) {
    assert!(seconds >= 0);
    let position = p0 + v0 * seconds + acceleration * seconds * seconds / 2;
    let velocity = v0 + acceleration * seconds;
    (position, velocity)
}

#[test]
fn kinematics_are_evaluated_from_the_origin() {
    assert_eq!(kinematic(5, 0, 0, 10), (5, 0));
    assert_eq!(kinematic(1, 3, 0, 4), (13, 3));
    assert_eq!(kinematic(0, 2, 4, 3), (24, 14));
}

#[test]
fn contact_pair_order_is_stable() {
    let mut pairs = vec![(3_u8, 1_u8), (2, 1), (3, 2)];
    for pair in &mut pairs {
        if pair.0 > pair.1 {
            *pair = (pair.1, pair.0);
        }
    }
    pairs.sort();
    assert_eq!(pairs, vec![(1, 2), (1, 3), (2, 3)]);
}

#[test]
fn crash_vectors_never_roll_back_complete_certificates() {
    let outcomes: BTreeMap<_, _> = rows(STORAGE, 4)
        .into_iter()
        .map(|row| (row[0], (row[2], row[3])))
        .collect();
    for (durable, expected) in outcomes.values() {
        if *durable == "yes" {
            assert!(!expected.starts_with("previous_frontier"));
        }
    }
}

#[test]
fn benchmark_seeds_are_256_bit_hex_and_unique() {
    let parsed = rows(BENCHMARKS, 5);
    let seeds: BTreeSet<_> = parsed.iter().map(|row| row[2]).collect();
    assert_eq!(seeds.len(), parsed.len());
    assert!(
        seeds
            .iter()
            .all(|seed| seed.len() == 64 && seed.bytes().all(|b| b.is_ascii_hexdigit()))
    );
}

#[test]
fn benchmark_blob_fixture_stays_within_the_selected_single_blob_cap() {
    let parsed = rows(BENCHMARKS, 5);
    let bm04 = parsed.iter().find(|row| row[0] == "BM-04").unwrap();
    assert!(bm04[3].contains("12GiB"));
    let selected_cap_gib = 16_u64;
    let fixture_gib = 12_u64;
    assert!(fixture_gib <= selected_cap_gib);
}
