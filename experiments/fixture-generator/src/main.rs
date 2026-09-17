#![forbid(unsafe_code)]

use std::{env, io::Write, process::ExitCode};

const KINDS: &[&str] = &[
    "graph",
    "events",
    "blobs",
    "content",
    "points",
    "observations",
    "bodies",
    "mixed",
];

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

fn visit_fixture(
    kind: &str,
    count: u64,
    seed: [u8; 32],
    mut emit: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if !KINDS.contains(&kind) {
        return Err(format!("unsupported fixture kind: {kind}"));
    }
    emit(b"USTE-SYNTHETIC-V1\0")?;
    emit(&[kind.len() as u8])?;
    emit(kind.as_bytes())?;
    emit(&count.to_le_bytes())?;
    for index in 0..count {
        let mut record = blake3::Hasher::new_keyed(&seed);
        record.update(b"USTE synthetic-v1 record");
        record.update(kind.as_bytes());
        record.update(&index.to_le_bytes());
        // Domain-shaped deterministic values make accidental generator changes visible while
        // allowing benchmark drivers to materialize only the fields they need.
        let identity = record.finalize();
        emit(&index.to_le_bytes())?;
        emit(identity.as_bytes())?;
        let shaped = match kind {
            "graph" => index.wrapping_mul(6364136223846793005) % count.max(1),
            "points" => u64::from_le_bytes(identity.as_bytes()[..8].try_into().unwrap()),
            "observations" => index % 100_000,
            "bodies" => 1 + index % 1_000,
            "blobs" | "content" => 4096 + index % 65_536,
            "events" | "mixed" => index / 10_000,
            _ => unreachable!(),
        };
        emit(&shaped.to_le_bytes())?;
    }
    Ok(())
}

fn fixture_digest(kind: &str, count: u64, seed: [u8; 32]) -> Result<blake3::Hash, String> {
    let mut dataset = blake3::Hasher::new_derive_key("USTE synthetic-v1 dataset");
    visit_fixture(kind, count, seed, |bytes| {
        dataset.update(bytes);
        Ok(())
    })?;
    Ok(dataset.finalize())
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let action = args
        .next()
        .ok_or("usage: uste-fixture-generator digest|emit KIND COUNT SEED_HEX")?;
    let kind = args.next().ok_or("missing KIND")?;
    let count = args
        .next()
        .ok_or("missing COUNT")?
        .parse::<u64>()
        .map_err(|_| "COUNT must be an unsigned integer")?;
    let seed = decode_seed(&args.next().ok_or("missing SEED_HEX")?)?;
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    match action.as_str() {
        "digest" => println!("{}", fixture_digest(&kind, count, seed)?),
        "emit" => {
            let stdout = std::io::stdout();
            let mut output = stdout.lock();
            visit_fixture(&kind, count, seed, |bytes| {
                output.write_all(bytes).map_err(|error| error.to_string())
            })?;
            output.flush().map_err(|error| error.to_string())?;
        }
        _ => return Err(format!("unsupported action: {action}")),
    }
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
        assert_ne!(
            fixture_digest("graph", 10, seed),
            fixture_digest("points", 10, seed)
        );
        assert_ne!(
            fixture_digest("graph", 10, seed),
            fixture_digest("graph", 11, seed)
        );
        assert!(fixture_digest("unknown", 10, seed).is_err());
    }

    #[test]
    fn small_graph_golden_digest() {
        let seed = decode_seed(SEED).unwrap();
        assert_eq!(
            fixture_digest("graph", 3, seed).unwrap().to_hex().as_str(),
            "03e40364c7454f76790af26062ab4094483bb5df6dfc0d0feac4f0b4c2c1f499"
        );
    }

    #[test]
    fn materialized_stream_hash_equals_digest() {
        let seed = decode_seed(SEED).unwrap();
        let mut bytes = Vec::new();
        visit_fixture("points", 5, seed, |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
        let mut hash = blake3::Hasher::new_derive_key("USTE synthetic-v1 dataset");
        hash.update(&bytes);
        assert_eq!(hash.finalize(), fixture_digest("points", 5, seed).unwrap());
        assert_eq!(&bytes[..18], b"USTE-SYNTHETIC-V1\0");
    }
}
