//! Content-free JSON manifest construction.

use std::fmt::Write;

use crate::{
    Bm01Profile, Materializer, QuerySet, materialization_revision_count, measured_queries,
    query_digest, warmup_queries,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bm01Manifest {
    profile: Bm01Profile,
    synthetic_entities: [u8; 32],
    synthetic_relationships: [u8; 32],
    materialization: [u8; 32],
    topology: [u8; 32],
    measured_queries: [u8; 32],
    warmup_queries: [u8; 32],
}

impl Bm01Manifest {
    #[must_use]
    pub fn build(profile: Bm01Profile) -> Self {
        let materializer = Materializer::new(profile);
        let digests = materializer.digests();
        Self {
            profile,
            synthetic_entities: materializer.synthetic_entity_digest(),
            synthetic_relationships: materializer.synthetic_relationship_digest(),
            materialization: digests.materialization,
            topology: digests.topology,
            measured_queries: query_digest(profile, QuerySet::Measured),
            warmup_queries: query_digest(profile, QuerySet::Warmup),
        }
    }

    #[must_use]
    pub fn profile(&self) -> Bm01Profile {
        self.profile
    }

    #[must_use]
    pub fn synthetic_entity_digest(&self) -> [u8; 32] {
        self.synthetic_entities
    }

    #[must_use]
    pub fn synthetic_relationship_digest(&self) -> [u8; 32] {
        self.synthetic_relationships
    }

    #[must_use]
    pub fn materialization_digest(&self) -> [u8; 32] {
        self.materialization
    }

    #[must_use]
    pub fn topology_digest(&self) -> [u8; 32] {
        self.topology
    }

    #[must_use]
    pub fn measured_query_digest(&self) -> [u8; 32] {
        self.measured_queries
    }

    #[must_use]
    pub fn warmup_query_digest(&self) -> [u8; 32] {
        self.warmup_queries
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let qualification = if self.profile.is_qualifying() {
            "qualifying-fixture-size"
        } else {
            "nonqualifying-small-scale"
        };
        let mut json = String::new();
        writeln!(&mut json, "{{").unwrap();
        writeln!(&mut json, "  \"schema\": \"uste-bm01-manifest-v1\",").unwrap();
        writeln!(
            &mut json,
            "  \"materialization_profile\": \"bm01-materialization-v1\","
        )
        .unwrap();
        writeln!(
            &mut json,
            "  \"engine_mapping_profile\": \"bm01-uste-graph-v1\","
        )
        .unwrap();
        writeln!(&mut json, "  \"qualification\": \"{qualification}\",").unwrap();
        writeln!(&mut json, "  \"engine_benchmark\": false,").unwrap();
        writeln!(
            &mut json,
            "  \"durable_revisions\": {},",
            materialization_revision_count(self.profile)
        )
        .unwrap();
        writeln!(&mut json, "  \"seed\": \"{}\",", hex(&crate::ACCEPTED_SEED)).unwrap();
        writeln!(&mut json, "  \"counts\": {{").unwrap();
        writeln!(&mut json, "    \"entities\": {},", self.profile.entities()).unwrap();
        writeln!(
            &mut json,
            "    \"relationships\": {},",
            self.profile.relationships()
        )
        .unwrap();
        writeln!(&mut json, "    \"uniform\": {},", self.profile.uniform()).unwrap();
        writeln!(
            &mut json,
            "    \"distributed_hub\": {},",
            self.profile.distributed_hub()
        )
        .unwrap();
        writeln!(
            &mut json,
            "    \"ring_cycle\": {},",
            self.profile.ring_cycle()
        )
        .unwrap();
        writeln!(&mut json, "    \"hubs\": {}", self.profile.hubs()).unwrap();
        writeln!(&mut json, "  }},").unwrap();
        writeln!(&mut json, "  \"queries\": {{").unwrap();
        writeln!(&mut json, "    \"depths\": [1, 2, 3, 4],").unwrap();
        writeln!(
            &mut json,
            "    \"measured\": {},",
            measured_queries(self.profile).len()
        )
        .unwrap();
        writeln!(
            &mut json,
            "    \"warmup\": {}",
            warmup_queries(self.profile).len()
        )
        .unwrap();
        writeln!(&mut json, "  }},").unwrap();
        writeln!(
            &mut json,
            "  \"limits\": {{\"visits\": 1000000, \"results\": 100000}},"
        )
        .unwrap();
        writeln!(&mut json, "  \"digests\": {{").unwrap();
        writeln!(
            &mut json,
            "    \"synthetic_entities\": \"{}\",",
            hex(&self.synthetic_entities)
        )
        .unwrap();
        writeln!(
            &mut json,
            "    \"synthetic_relationships\": \"{}\",",
            hex(&self.synthetic_relationships)
        )
        .unwrap();
        writeln!(
            &mut json,
            "    \"materialization\": \"{}\",",
            hex(&self.materialization)
        )
        .unwrap();
        writeln!(&mut json, "    \"topology\": \"{}\",", hex(&self.topology)).unwrap();
        writeln!(
            &mut json,
            "    \"measured_queries\": \"{}\",",
            hex(&self.measured_queries)
        )
        .unwrap();
        writeln!(
            &mut json,
            "    \"warmup_queries\": \"{}\"",
            hex(&self.warmup_queries)
        )
        .unwrap();
        writeln!(&mut json, "  }}").unwrap();
        writeln!(&mut json, "}}").unwrap();
        json
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").unwrap();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::Bm01Manifest;
    use crate::Bm01Profile;

    const ACCEPTANCE: &str = include_str!("../../../acceptance/r1/bm01-materialization-v1.tsv");

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn qualifying_manifest_digests_are_golden() {
        let manifest = Bm01Manifest::build(Bm01Profile::qualifying());
        assert_eq!(
            hex(manifest.synthetic_entity_digest()),
            "f2d0051a2f2930990ccb4f272e6b02208167c9c7c17a92ba12bc8b118cb5ebae"
        );
        assert_eq!(
            hex(manifest.synthetic_relationship_digest()),
            "0bfef18608c1b86507af704f9f0fcd5addf4cb76978590426016f395b8707cc1"
        );
        assert_eq!(
            hex(manifest.materialization_digest()),
            "ac2899311100d05f5cbf9772ce2ba0fa20fcb7af1aa200d61ef606b597b5890d"
        );
        assert_eq!(
            hex(manifest.topology_digest()),
            "237ae96c6872fa5b2e016686e78ed90e380c72799c16e2efb5800eaecd5f27d6"
        );
        assert_eq!(
            hex(manifest.measured_query_digest()),
            "5e12be0c9c8016d5b1da1dbceddbccc351c8729013925ebad81b96a100a6ce27"
        );
        assert_eq!(
            hex(manifest.warmup_query_digest()),
            "f350fba4568e146bdd0f61542a1b3c4579fa5f4dee53ccad65b738b1a75bf226"
        );
        assert_eq!(ACCEPTANCE.lines().count(), 21);
        for expected in [
            "profile\tseed\tbm01-materialization-v1\t8f41d0a52b40f13f4a77bc3beae2026a8bc42ad48d12ce53d92e29f612111001",
            "profile\tentities\tqualifying\t100000",
            "profile\trelationships\tqualifying\t1000000",
            "profile\tengine_mapping\tqualifying\tbm01-uste-graph-v1",
            "profile\tdurable_revisions\tmaximum-10000-operations\t212",
            "topology\tuniform_relationships\tqualifying\t800000",
            "topology\tdistributed_hub_relationships\tqualifying\t100000",
            "topology\tring_cycle_relationships\tqualifying\t100000",
            "topology\thubs\tqualifying\t100",
            "queries\tmeasured\t3-classes-x-4-depths-x-32\t384",
            "queries\twarmup\t3-classes-x-4-depths-x-8\t96",
            "limits\tvisits\tglobal-per-query\t1000000",
            "limits\tunique_relationship_results\tglobal-per-query\t100000",
            "digest\tsynthetic_entities\tsynthetic-v1-graph-count-100000\tf2d0051a2f2930990ccb4f272e6b02208167c9c7c17a92ba12bc8b118cb5ebae",
            "digest\tsynthetic_relationships\tsynthetic-v1-graph-count-1000000\t0bfef18608c1b86507af704f9f0fcd5addf4cb76978590426016f395b8707cc1",
            "digest\tmaterialization\tbm01-materialization-v1\tac2899311100d05f5cbf9772ce2ba0fa20fcb7af1aa200d61ef606b597b5890d",
            "digest\ttopology\tbm01-topology-v1\t237ae96c6872fa5b2e016686e78ed90e380c72799c16e2efb5800eaecd5f27d6",
            "digest\tmeasured_queries\tbm01-query-corpus-v1\t5e12be0c9c8016d5b1da1dbceddbccc351c8729013925ebad81b96a100a6ce27",
            "digest\twarmup_queries\tbm01-query-corpus-v1\tf350fba4568e146bdd0f61542a1b3c4579fa5f4dee53ccad65b738b1a75bf226",
            "claim\tengine_benchmark\tfixture-only\tfalse",
        ] {
            assert!(ACCEPTANCE.lines().any(|line| line == expected));
        }
    }

    #[test]
    fn scaled_manifest_is_explicitly_nonqualifying_and_content_free() {
        let manifest = Bm01Manifest::build(Bm01Profile::new(20).unwrap());
        let json = manifest.to_json();
        assert!(json.contains("\"qualification\": \"nonqualifying-small-scale\""));
        assert!(json.contains("\"engine_benchmark\": false"));
        assert!(!json.contains("\"root\""));
        assert!(!json.contains("entity_id"));
    }
}
