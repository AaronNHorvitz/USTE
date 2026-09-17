//! Independent adjacency-array breadth-first expansion oracle.

use std::collections::BTreeSet;

use crate::{Bm01Profile, Direction, Materializer, QuerySpec};

pub const MAX_VISITS: usize = 1_000_000;
pub const MAX_RESULTS: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OracleLimits {
    pub maximum_visits: usize,
    pub maximum_results: usize,
}

impl Default for OracleLimits {
    fn default() -> Self {
        Self {
            maximum_visits: MAX_VISITS,
            maximum_results: MAX_RESULTS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleError {
    InvalidRoot,
    InvalidDepth,
    VisitLimit,
    ResultLimit,
    ResourceLimit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleOutput {
    pub visits: usize,
    pub relationships: Vec<u64>,
    pub reachable_entities: Vec<u64>,
}

impl OracleOutput {
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 oracle-output-v1");
        digest.update(&(self.visits as u64).to_be_bytes());
        digest.update(&(self.relationships.len() as u64).to_be_bytes());
        for ordinal in &self.relationships {
            digest.update(&ordinal.to_be_bytes());
        }
        digest.update(&(self.reachable_entities.len() as u64).to_be_bytes());
        for ordinal in &self.reachable_entities {
            digest.update(&ordinal.to_be_bytes());
        }
        *digest.finalize().as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Arc {
    relationship: u64,
    neighbor: u64,
}

#[derive(Debug)]
pub struct Oracle {
    profile: Bm01Profile,
    outgoing: Vec<Vec<Arc>>,
    incoming: Vec<Vec<Arc>>,
}

impl Oracle {
    pub fn build(profile: Bm01Profile) -> Result<Self, OracleError> {
        let entities =
            usize::try_from(profile.entities()).map_err(|_| OracleError::ResourceLimit)?;
        let mut outgoing = vec![Vec::new(); entities];
        let mut incoming = vec![Vec::new(); entities];
        let materializer = Materializer::new(profile);
        for ordinal in 0..profile.relationships() {
            let edge = materializer.edge(ordinal);
            let source = usize::try_from(edge.source).map_err(|_| OracleError::ResourceLimit)?;
            let destination =
                usize::try_from(edge.destination).map_err(|_| OracleError::ResourceLimit)?;
            outgoing[source].push(Arc {
                relationship: ordinal,
                neighbor: edge.destination,
            });
            incoming[destination].push(Arc {
                relationship: ordinal,
                neighbor: edge.source,
            });
        }
        for adjacency in outgoing.iter_mut().chain(&mut incoming) {
            adjacency.sort_unstable();
        }
        Ok(Self {
            profile,
            outgoing,
            incoming,
        })
    }

    pub fn expand(
        &self,
        query: QuerySpec,
        limits: OracleLimits,
    ) -> Result<OracleOutput, OracleError> {
        if query.root >= self.profile.entities() {
            return Err(OracleError::InvalidRoot);
        }
        if !(1..=4).contains(&query.depth) {
            return Err(OracleError::InvalidDepth);
        }
        if limits.maximum_visits > MAX_VISITS || limits.maximum_results > MAX_RESULTS {
            return Err(OracleError::ResourceLimit);
        }

        let mut visits = 0_usize;
        let mut relationships = BTreeSet::new();
        let mut reachable_entities = BTreeSet::new();
        let mut seen_entities = BTreeSet::from([query.root]);
        let mut frontier = BTreeSet::from([query.root]);

        for _ in 0..query.depth {
            let mut next = BTreeSet::new();
            for entity in frontier {
                let entity = usize::try_from(entity).map_err(|_| OracleError::ResourceLimit)?;
                let mut candidates = Vec::new();
                if matches!(query.direction, Direction::Outgoing | Direction::Either) {
                    candidates.extend_from_slice(&self.outgoing[entity]);
                }
                if matches!(query.direction, Direction::Incoming | Direction::Either) {
                    candidates.extend_from_slice(&self.incoming[entity]);
                }
                candidates.sort_unstable();
                candidates.dedup();
                for candidate in candidates {
                    visits = visits.checked_add(1).ok_or(OracleError::ResourceLimit)?;
                    if visits > limits.maximum_visits {
                        return Err(OracleError::VisitLimit);
                    }
                    if relationships.insert(candidate.relationship)
                        && relationships.len() > limits.maximum_results
                    {
                        return Err(OracleError::ResultLimit);
                    }
                    reachable_entities.insert(candidate.neighbor);
                    if seen_entities.insert(candidate.neighbor) {
                        next.insert(candidate.neighbor);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        reachable_entities.remove(&query.root);
        Ok(OracleOutput {
            visits,
            relationships: relationships.into_iter().collect(),
            reachable_entities: reachable_entities.into_iter().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Oracle, OracleError, OracleLimits};
    use crate::{Bm01Profile, Direction, Materializer, QueryClass, QuerySet, QuerySpec};

    #[test]
    fn adjacency_arrays_match_an_edge_scan_reference() {
        let profile = Bm01Profile::new(20).unwrap();
        let oracle = Oracle::build(profile).unwrap();
        for direction in [Direction::Outgoing, Direction::Incoming, Direction::Either] {
            let query = QuerySpec {
                set: QuerySet::Measured,
                class: QueryClass::Uniform,
                direction,
                depth: 4,
                ordinal: 0,
                root: 7,
            };
            let actual = oracle.expand(query, OracleLimits::default()).unwrap();
            let expected = edge_scan_reference(profile, query);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn global_limits_fail_closed() {
        let profile = Bm01Profile::new(20).unwrap();
        let oracle = Oracle::build(profile).unwrap();
        let query = QuerySpec {
            set: QuerySet::Measured,
            class: QueryClass::Hub,
            direction: Direction::Either,
            depth: 1,
            ordinal: 0,
            root: 0,
        };
        assert_eq!(
            oracle.expand(
                query,
                OracleLimits {
                    maximum_visits: 0,
                    maximum_results: 100,
                }
            ),
            Err(OracleError::VisitLimit)
        );
        assert_eq!(
            oracle.expand(
                query,
                OracleLimits {
                    maximum_visits: 100,
                    maximum_results: 0,
                }
            ),
            Err(OracleError::ResultLimit)
        );
    }

    fn edge_scan_reference(profile: Bm01Profile, query: QuerySpec) -> super::OracleOutput {
        let materializer = Materializer::new(profile);
        let mut visits = 0;
        let mut relationships = BTreeSet::new();
        let mut reachable = BTreeSet::new();
        let mut seen = BTreeSet::from([query.root]);
        let mut frontier = BTreeSet::from([query.root]);
        for _ in 0..query.depth {
            let mut next = BTreeSet::new();
            for entity in frontier {
                for ordinal in 0..profile.relationships() {
                    let edge = materializer.edge(ordinal);
                    let neighbor = match query.direction {
                        Direction::Outgoing if edge.source == entity => Some(edge.destination),
                        Direction::Incoming if edge.destination == entity => Some(edge.source),
                        Direction::Either if edge.source == entity => Some(edge.destination),
                        Direction::Either if edge.destination == entity => Some(edge.source),
                        _ => None,
                    };
                    if let Some(neighbor) = neighbor {
                        visits += 1;
                        relationships.insert(ordinal);
                        reachable.insert(neighbor);
                        if seen.insert(neighbor) {
                            next.insert(neighbor);
                        }
                    }
                }
            }
            frontier = next;
        }
        reachable.remove(&query.root);
        super::OracleOutput {
            visits,
            relationships: relationships.into_iter().collect(),
            reachable_entities: reachable.into_iter().collect(),
        }
    }
}
