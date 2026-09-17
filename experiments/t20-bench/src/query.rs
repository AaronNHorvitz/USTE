//! Pinned measured and warm-up query corpora.

use crate::materialization::{ACCEPTED_SEED, Bm01Profile, Materializer};

pub const MEASURED_PER_CLASS_DEPTH: u64 = 32;
pub const WARMUP_PER_CLASS_DEPTH: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Direction {
    Outgoing,
    Incoming,
    Either,
}

impl Direction {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Outgoing => 1,
            Self::Incoming => 2,
            Self::Either => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QueryClass {
    Uniform,
    Hub,
    Cycle,
}

impl QueryClass {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Uniform => 1,
            Self::Hub => 2,
            Self::Cycle => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuerySet {
    Measured,
    Warmup,
}

impl QuerySet {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Measured => 1,
            Self::Warmup => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuerySpec {
    pub set: QuerySet,
    pub class: QueryClass,
    pub direction: Direction,
    pub depth: u8,
    pub ordinal: u64,
    pub root: u64,
}

#[must_use]
pub fn measured_queries(profile: Bm01Profile) -> Vec<QuerySpec> {
    queries(profile, QuerySet::Measured, MEASURED_PER_CLASS_DEPTH)
}

#[must_use]
pub fn warmup_queries(profile: Bm01Profile) -> Vec<QuerySpec> {
    queries(profile, QuerySet::Warmup, WARMUP_PER_CLASS_DEPTH)
}

#[must_use]
pub fn query_digest(profile: Bm01Profile, set: QuerySet) -> [u8; 32] {
    let materializer = Materializer::new(profile);
    let corpus = match set {
        QuerySet::Measured => measured_queries(profile),
        QuerySet::Warmup => warmup_queries(profile),
    };
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 query-corpus-v1");
    digest.update(&ACCEPTED_SEED);
    digest.update(&[set.code()]);
    digest.update(&profile.entities().to_be_bytes());
    digest.update(&(corpus.len() as u64).to_be_bytes());
    for query in corpus {
        digest.update(&[query.class.code(), query.direction.code(), query.depth]);
        digest.update(&query.ordinal.to_be_bytes());
        digest.update(&materializer.entity_id(query.root).0);
    }
    *digest.finalize().as_bytes()
}

fn queries(profile: Bm01Profile, set: QuerySet, per_class_depth: u64) -> Vec<QuerySpec> {
    let mut result = Vec::with_capacity((4 * 3 * per_class_depth) as usize);
    for depth in 1..=4 {
        for class in [QueryClass::Uniform, QueryClass::Hub, QueryClass::Cycle] {
            let base = query_base(profile, class, depth);
            let set_offset = match set {
                QuerySet::Measured => 0,
                QuerySet::Warmup => MEASURED_PER_CLASS_DEPTH,
            };
            for ordinal in 0..per_class_depth {
                let root = match class {
                    QueryClass::Uniform => {
                        let eligible = profile.entities() - profile.hubs();
                        profile.hubs() + (base + set_offset + ordinal) % eligible
                    }
                    QueryClass::Hub => (base + set_offset + ordinal) % profile.hubs(),
                    QueryClass::Cycle => (base + set_offset + ordinal) % profile.entities(),
                };
                let direction = match class {
                    QueryClass::Uniform if ordinal.is_multiple_of(2) => Direction::Outgoing,
                    QueryClass::Uniform => Direction::Incoming,
                    QueryClass::Hub => Direction::Either,
                    QueryClass::Cycle => Direction::Outgoing,
                };
                result.push(QuerySpec {
                    set,
                    class,
                    direction,
                    depth,
                    ordinal,
                    root,
                });
            }
        }
    }
    result
}

fn query_base(profile: Bm01Profile, class: QueryClass, depth: u8) -> u64 {
    let mut hasher = blake3::Hasher::new_keyed(&ACCEPTED_SEED);
    hasher.update(b"USTE BM-01 query-root-v1\0");
    hasher.update(&profile.entities().to_be_bytes());
    hasher.update(&[class.code(), depth]);
    u64::from_le_bytes(
        hasher.finalize().as_bytes()[..8]
            .try_into()
            .expect("fixed slice"),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{QueryClass, measured_queries, warmup_queries};
    use crate::Bm01Profile;

    #[test]
    fn qualifying_query_sets_have_pinned_shape_and_disjoint_roots_per_stratum() {
        let profile = Bm01Profile::qualifying();
        let measured = measured_queries(profile);
        let warmup = warmup_queries(profile);
        assert_eq!(measured.len(), 384);
        assert_eq!(warmup.len(), 96);
        for depth in 1..=4 {
            for class in [QueryClass::Uniform, QueryClass::Hub, QueryClass::Cycle] {
                let measured_roots: BTreeSet<_> = measured
                    .iter()
                    .filter(|query| query.depth == depth && query.class == class)
                    .map(|query| query.root)
                    .collect();
                let warmup_roots: BTreeSet<_> = warmup
                    .iter()
                    .filter(|query| query.depth == depth && query.class == class)
                    .map(|query| query.root)
                    .collect();
                assert!(measured_roots.is_disjoint(&warmup_roots));
            }
        }
    }
}
