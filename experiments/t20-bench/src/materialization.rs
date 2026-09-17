//! Pinned `bm01-materialization-v1` topology and typed identifier mapping.

use crate::synthetic::{graph_record, graph_stream_digest};

pub const ACCEPTED_SEED: [u8; 32] = [
    0x8f, 0x41, 0xd0, 0xa5, 0x2b, 0x40, 0xf1, 0x3f, 0x4a, 0x77, 0xbc, 0x3b, 0xea, 0xe2, 0x02, 0x6a,
    0x8b, 0xc4, 0x2a, 0xd4, 0x8d, 0x12, 0xce, 0x53, 0xd9, 0x2e, 0x29, 0xf6, 0x12, 0x11, 0x10, 0x01,
];

pub const QUALIFYING_ENTITIES: u64 = 100_000;
pub const QUALIFYING_RELATIONSHIPS: u64 = 1_000_000;
const RELATIONSHIPS_PER_ENTITY: u64 = 10;
const ID_DOMAIN: &[u8] = b"USTE BM-01 materialization-v1 typed-id\0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EntityId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelationshipId(pub [u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Topology {
    Uniform,
    DistributedHub,
    RingCycle,
}

impl Topology {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Uniform => 1,
            Self::DistributedHub => 2,
            Self::RingCycle => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Edge {
    pub ordinal: u64,
    pub id: RelationshipId,
    pub topology: Topology,
    pub source: u64,
    pub destination: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bm01Profile {
    entities: u64,
    relationships: u64,
    uniform: u64,
    distributed_hub: u64,
    ring_cycle: u64,
    hubs: u64,
}

impl Bm01Profile {
    pub fn new(entities: u64) -> Result<Self, &'static str> {
        if !(2..=QUALIFYING_ENTITIES).contains(&entities) {
            return Err("entity count must be between 2 and 100000");
        }
        let relationships = entities
            .checked_mul(RELATIONSHIPS_PER_ENTITY)
            .ok_or("relationship count overflow")?;
        let uniform = entities.checked_mul(8).ok_or("uniform count overflow")?;
        let distributed_hub = entities;
        let ring_cycle = entities;
        let hubs = (entities / 1_000).clamp(1, 100);
        Ok(Self {
            entities,
            relationships,
            uniform,
            distributed_hub,
            ring_cycle,
            hubs,
        })
    }

    #[must_use]
    pub fn qualifying() -> Self {
        Self::new(QUALIFYING_ENTITIES).expect("qualifying count is valid")
    }

    #[must_use]
    pub const fn is_qualifying(self) -> bool {
        self.entities == QUALIFYING_ENTITIES && self.relationships == QUALIFYING_RELATIONSHIPS
    }

    #[must_use]
    pub const fn entities(self) -> u64 {
        self.entities
    }

    #[must_use]
    pub const fn relationships(self) -> u64 {
        self.relationships
    }

    #[must_use]
    pub const fn uniform(self) -> u64 {
        self.uniform
    }

    #[must_use]
    pub const fn distributed_hub(self) -> u64 {
        self.distributed_hub
    }

    #[must_use]
    pub const fn ring_cycle(self) -> u64 {
        self.ring_cycle
    }

    #[must_use]
    pub const fn hubs(self) -> u64 {
        self.hubs
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Materializer {
    profile: Bm01Profile,
}

impl Materializer {
    #[must_use]
    pub const fn new(profile: Bm01Profile) -> Self {
        Self { profile }
    }

    #[must_use]
    pub const fn profile(self) -> Bm01Profile {
        self.profile
    }

    #[must_use]
    pub fn entity_id(self, ordinal: u64) -> EntityId {
        assert!(ordinal < self.profile.entities);
        EntityId(typed_id(0xe1, ordinal))
    }

    #[must_use]
    pub fn relationship_id(self, ordinal: u64) -> RelationshipId {
        assert!(ordinal < self.profile.relationships);
        RelationshipId(typed_id(0xa1, ordinal))
    }

    #[must_use]
    pub fn edge(self, ordinal: u64) -> Edge {
        assert!(ordinal < self.profile.relationships);
        let record = graph_record(self.profile.relationships, &ACCEPTED_SEED, ordinal);
        let (topology, source, destination) = if ordinal < self.profile.uniform {
            let source = word(&record.identity, 0) % self.profile.entities;
            let candidate = word(&record.identity, 8) % (self.profile.entities - 1);
            let destination = if candidate >= source {
                candidate + 1
            } else {
                candidate
            };
            (Topology::Uniform, source, destination)
        } else if ordinal < self.profile.uniform + self.profile.distributed_hub {
            let local = ordinal - self.profile.uniform;
            let hub = local % self.profile.hubs;
            let candidate = word(&record.identity, 16) % (self.profile.entities - 1);
            let leaf = if candidate >= hub {
                candidate + 1
            } else {
                candidate
            };
            // Round-robin assigns one edge to every hub before changing direction. At the
            // qualifying size each hub therefore has exactly 500 outward and 500 inward edges.
            if (local / self.profile.hubs).is_multiple_of(2) {
                (Topology::DistributedHub, hub, leaf)
            } else {
                (Topology::DistributedHub, leaf, hub)
            }
        } else {
            let local = ordinal - self.profile.uniform - self.profile.distributed_hub;
            let source = local % self.profile.entities;
            let destination = (source + 1) % self.profile.entities;
            (Topology::RingCycle, source, destination)
        };
        Edge {
            ordinal,
            id: self.relationship_id(ordinal),
            topology,
            source,
            destination,
        }
    }

    #[must_use]
    pub fn synthetic_entity_digest(self) -> [u8; 32] {
        graph_stream_digest(self.profile.entities, &ACCEPTED_SEED)
    }

    #[must_use]
    pub fn synthetic_relationship_digest(self) -> [u8; 32] {
        graph_stream_digest(self.profile.relationships, &ACCEPTED_SEED)
    }

    #[must_use]
    pub fn digests(self) -> MaterializationDigests {
        let mut materialization = blake3::Hasher::new_derive_key("USTE BM-01 materialization-v1");
        let mut topology = blake3::Hasher::new_derive_key("USTE BM-01 topology-v1");
        for digest in [&mut materialization, &mut topology] {
            digest.update(&ACCEPTED_SEED);
            digest.update(&self.profile.entities.to_be_bytes());
            digest.update(&self.profile.relationships.to_be_bytes());
            digest.update(&self.profile.uniform.to_be_bytes());
            digest.update(&self.profile.distributed_hub.to_be_bytes());
            digest.update(&self.profile.ring_cycle.to_be_bytes());
            digest.update(&self.profile.hubs.to_be_bytes());
        }
        for ordinal in 0..self.profile.entities {
            materialization.update(b"E");
            materialization.update(&ordinal.to_be_bytes());
            materialization.update(&self.entity_id(ordinal).0);
        }
        for ordinal in 0..self.profile.relationships {
            let edge = self.edge(ordinal);
            topology.update(&[edge.topology.code()]);
            topology.update(&edge.ordinal.to_be_bytes());
            topology.update(&edge.source.to_be_bytes());
            topology.update(&edge.destination.to_be_bytes());

            materialization.update(b"R");
            materialization.update(&[edge.topology.code()]);
            materialization.update(&edge.ordinal.to_be_bytes());
            materialization.update(&edge.id.0);
            materialization.update(&self.entity_id(edge.source).0);
            materialization.update(&self.entity_id(edge.destination).0);
        }
        MaterializationDigests {
            materialization: *materialization.finalize().as_bytes(),
            topology: *topology.finalize().as_bytes(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterializationDigests {
    pub materialization: [u8; 32],
    pub topology: [u8; 32],
}

fn typed_id(tag: u8, ordinal: u64) -> [u8; 16] {
    let mut prefix = blake3::Hasher::new_keyed(&ACCEPTED_SEED);
    prefix.update(ID_DOMAIN);
    prefix.update(&[tag]);
    let prefix = prefix.finalize();
    let mut id = [0_u8; 16];
    id[0] = tag;
    id[1..8].copy_from_slice(&prefix.as_bytes()[..7]);
    id[8..].copy_from_slice(&ordinal.to_be_bytes());
    id
}

fn word(bytes: &[u8; 32], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed slice"))
}

#[cfg(test)]
mod tests {
    use super::{Bm01Profile, Materializer, Topology};

    #[test]
    fn qualifying_partition_is_exact() {
        let profile = Bm01Profile::qualifying();
        assert!(profile.is_qualifying());
        assert_eq!(profile.entities(), 100_000);
        assert_eq!(profile.relationships(), 1_000_000);
        assert_eq!(profile.uniform(), 800_000);
        assert_eq!(profile.distributed_hub(), 100_000);
        assert_eq!(profile.ring_cycle(), 100_000);
        assert_eq!(profile.hubs(), 100);
    }

    #[test]
    fn scaled_topology_has_no_self_loops_and_one_full_ring() {
        let profile = Bm01Profile::new(20).unwrap();
        let materializer = Materializer::new(profile);
        let mut counts = [0_u64; 3];
        for ordinal in 0..profile.relationships() {
            let edge = materializer.edge(ordinal);
            assert_ne!(edge.source, edge.destination);
            match edge.topology {
                Topology::Uniform => counts[0] += 1,
                Topology::DistributedHub => counts[1] += 1,
                Topology::RingCycle => {
                    counts[2] += 1;
                    assert_eq!(edge.destination, (edge.source + 1) % profile.entities());
                }
            }
        }
        assert_eq!(counts, [160, 20, 20]);
    }

    #[test]
    fn qualifying_hubs_have_balanced_stable_directions() {
        let profile = Bm01Profile::qualifying();
        let materializer = Materializer::new(profile);
        let mut directions = vec![(0_u64, 0_u64); profile.hubs() as usize];
        for ordinal in profile.uniform()..profile.uniform() + profile.distributed_hub() {
            let edge = materializer.edge(ordinal);
            let local = ordinal - profile.uniform();
            let hub = local % profile.hubs();
            if edge.source == hub {
                directions[hub as usize].0 += 1;
            } else {
                assert_eq!(edge.destination, hub);
                directions[hub as usize].1 += 1;
            }
        }
        assert!(
            directions
                .into_iter()
                .all(|(outgoing, incoming)| (outgoing, incoming) == (500, 500))
        );
    }

    #[test]
    fn typed_ids_are_stable_disjoint_and_ordinal_preserving() {
        let materializer = Materializer::new(Bm01Profile::new(20).unwrap());
        let entity = materializer.entity_id(7).0;
        let relationship = materializer.relationship_id(7).0;
        assert_ne!(entity, relationship);
        assert_eq!(&entity[8..], &7_u64.to_be_bytes());
        assert_eq!(entity, materializer.entity_id(7).0);
    }
}
