//! Byte-compatible `synthetic-v1` graph-stream generation.

const HEADER: &[u8] = b"USTE-SYNTHETIC-V1\0";
const DATASET_CONTEXT: &str = "USTE synthetic-v1 dataset";
const RECORD_DOMAIN: &[u8] = b"USTE synthetic-v1 record";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticGraphRecord {
    pub index: u64,
    pub identity: [u8; 32],
    pub shaped: u64,
}

#[must_use]
pub fn graph_record(count: u64, seed: &[u8; 32], index: u64) -> SyntheticGraphRecord {
    assert!(index < count, "synthetic record index must be in range");
    let mut hasher = blake3::Hasher::new_keyed(seed);
    hasher.update(RECORD_DOMAIN);
    hasher.update(b"graph");
    hasher.update(&index.to_le_bytes());
    let identity = *hasher.finalize().as_bytes();
    SyntheticGraphRecord {
        index,
        identity,
        shaped: index.wrapping_mul(6_364_136_223_846_793_005) % count.max(1),
    }
}

#[must_use]
pub fn graph_stream_digest(count: u64, seed: &[u8; 32]) -> [u8; 32] {
    let mut digest = blake3::Hasher::new_derive_key(DATASET_CONTEXT);
    digest.update(HEADER);
    digest.update(&[5]);
    digest.update(b"graph");
    digest.update(&count.to_le_bytes());
    for index in 0..count {
        let record = graph_record(count, seed, index);
        digest.update(&record.index.to_le_bytes());
        digest.update(&record.identity);
        digest.update(&record.shaped.to_le_bytes());
    }
    *digest.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::graph_stream_digest;
    use crate::ACCEPTED_SEED;

    #[test]
    fn generator_graph_golden_is_byte_compatible() {
        assert_eq!(
            hex(&graph_stream_digest(3, &ACCEPTED_SEED)),
            "03e40364c7454f76790af26062ab4094483bb5df6dfc0d0feac4f0b4c2c1f499"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
