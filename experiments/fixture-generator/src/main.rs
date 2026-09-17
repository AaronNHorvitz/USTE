#![forbid(unsafe_code)]

use std::{env, process::ExitCode};

const KINDS: &[&str] = &["graph", "events", "blobs", "content", "points", "observations", "bodies", "mixed"];

fn decode_seed(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("seed must contain exactly 64 hexadecimal characters".into());
    }
    let mut seed = [0_u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "seed contains a non-hexadecimal character")?;
    }
    Ok(seed)
}

fn fixture_digest(kind: &str, count: u64, seed: [u8; 32]) -> Result<blake3::Hash, String> {
    if !KINDS.contains(&kind) {
        return Err(format!("unsupported fixture kind: {kind}"));
    }
    let mut dataset = blake3::Hasher::new_derive_key("USTE synthetic-v1 dataset");
    dataset.update(kind.as_bytes());
    dataset.update(&count.to_le_bytes());
    for index in 0..count {
        let mut record = blake3::Hasher::new_keyed(&seed);
        record.update(b"USTE synthetic-v1 record");
        record.update(kind.as_bytes());
        record.update(&index.to_le_bytes());
        // Domain-shaped deterministic values make accidental generator changes visible while
        // allowing benchmark drivers to materialize only the fields they need.
        let identity = record.finalize();
        dataset.update(&index.to_le_bytes());
        dataset.update(identity.as_bytes());
        match kind {
            "graph" => dataset.update(&(index.wrapping_mul(6364136223846793005) % count.max(1)).to_le_bytes()),
            "points" => dataset.update(&i64::from_le_bytes(identity.as_bytes()[..8].try_into().unwrap()).to_le_bytes()),
            "observations" => dataset.update(&(index % 100_000).to_le_bytes()),
            "bodies" => dataset.update(&(1 + index % 1_000).to_le_bytes()),
            "blobs" | "content" => dataset.update(&(4096 + index % 65_536).to_le_bytes()),
            "events" | "mixed" => dataset.update(&(index / 10_000).to_le_bytes()),
            _ => unreachable!(),
        };
    }
    Ok(dataset.finalize())
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let kind = args.next().ok_or("usage: uste-fixture-generator KIND COUNT SEED_HEX")?;
    let count = args
        .next()
        .ok_or("missing COUNT")?
        .parse::<u64>()
        .map_err(|_| "COUNT must be an unsigned integer")?;
    let seed = decode_seed(&args.next().ok_or("missing SEED_HEX")?)?;
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    println!("{}", fixture_digest(&kind, count, seed)?);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fixture-generator: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: &str = "8f41d0a52b40f13f4a77bc3beae2026a8bc42ad48d12ce53d92e29f612111001";

    #[test]
    fn seed_codec_is_strict() {
        assert!(decode_seed(SEED).is_ok());
        assert!(decode_seed("00").is_err());
        assert!(decode_seed(&"g".repeat(64)).is_err());
    }

    #[test]
    fn domains_and_counts_change_the_digest() {
        let seed = decode_seed(SEED).unwrap();
        assert_ne!(fixture_digest("graph", 10, seed), fixture_digest("points", 10, seed));
        assert_ne!(fixture_digest("graph", 10, seed), fixture_digest("graph", 11, seed));
        assert!(fixture_digest("unknown", 10, seed).is_err());
    }

    #[test]
    fn small_graph_golden_digest() {
        let seed = decode_seed(SEED).unwrap();
        assert_eq!(
            fixture_digest("graph", 3, seed).unwrap().to_hex().as_str(),
            "eb2cf7582dcdd97ccf55925e9c4b5026fbf6dc05dcd2d4f773485e6961568203"
        );
    }
}
