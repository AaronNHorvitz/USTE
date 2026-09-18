//! Bounded warm-up and measured BM-01 expectations generated outside the sampler process.

use std::fmt::Write as _;

use crate::{Bm01Profile, Oracle, OracleSummary, QuerySet};

pub const ORACLE_BUNDLE_PROFILE: &str = "bm01-oracle-bundle-v1";
pub const MAX_ORACLE_BUNDLE_BYTES: usize = 512 * 1024;
pub const QUALIFYING_ORACLE_BUNDLE_DIGEST: [u8; 32] = [
    213, 40, 105, 242, 77, 99, 84, 118, 248, 99, 116, 129, 62, 117, 75, 227, 100, 208, 210, 223,
    71, 5, 68, 183, 18, 33, 194, 75, 150, 20, 95, 174,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleBundle {
    warmup: OracleSummary,
    measured: OracleSummary,
    digest: [u8; 32],
}

impl OracleBundle {
    pub fn build(profile: Bm01Profile) -> Result<Self, String> {
        let oracle = Oracle::build(profile).map_err(debug)?;
        let warmup = OracleSummary::build_with_oracle(profile, QuerySet::Warmup, &oracle)?;
        let measured = OracleSummary::build_with_oracle(profile, QuerySet::Measured, &oracle)?;
        Self::from_sections(warmup, measured)
    }

    fn from_sections(warmup: OracleSummary, measured: OracleSummary) -> Result<Self, String> {
        if warmup.query_set() != QuerySet::Warmup
            || measured.query_set() != QuerySet::Measured
            || warmup.profile() != measured.profile()
        {
            return Err("oracle bundle section mismatch".into());
        }
        let digest = bundle_digest(&warmup, &measured);
        Ok(Self {
            warmup,
            measured,
            digest,
        })
    }

    #[must_use]
    pub fn profile(&self) -> Bm01Profile {
        self.measured.profile()
    }

    #[must_use]
    pub fn warmup(&self) -> &OracleSummary {
        &self.warmup
    }

    #[must_use]
    pub fn measured(&self) -> &OracleSummary {
        &self.measured
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[must_use]
    pub fn to_tsv(&self) -> String {
        let warmup = self.warmup.to_tsv();
        let measured = self.measured.to_tsv();
        let mut output = String::with_capacity(
            warmup
                .len()
                .saturating_add(measured.len())
                .saturating_add(256),
        );
        writeln!(&mut output, "schema\t{ORACLE_BUNDLE_PROFILE}").unwrap();
        writeln!(&mut output, "warmup_bytes\t{}", warmup.len()).unwrap();
        output.push_str(&warmup);
        writeln!(&mut output, "measured_bytes\t{}", measured.len()).unwrap();
        output.push_str(&measured);
        writeln!(&mut output, "bundle_digest\t{}", hex(&self.digest)).unwrap();
        output
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        if input.len() > MAX_ORACLE_BUNDLE_BYTES || input.contains('\r') {
            return Err("invalid oracle bundle envelope".into());
        }
        let mut offset = 0_usize;
        expect_line(input, &mut offset, "schema", ORACLE_BUNDLE_PROFILE)?;
        let warmup_bytes = parse_size_line(input, &mut offset, "warmup_bytes")?;
        let warmup_encoded = take_section(input, &mut offset, warmup_bytes)?;
        let warmup = OracleSummary::parse(warmup_encoded)?;
        expect_query_set(&warmup, QuerySet::Warmup)?;
        if warmup.to_tsv() != warmup_encoded {
            return Err("oracle bundle noncanonical warmup section".into());
        }

        let measured_bytes = parse_size_line(input, &mut offset, "measured_bytes")?;
        let measured_encoded = take_section(input, &mut offset, measured_bytes)?;
        let measured = OracleSummary::parse(measured_encoded)?;
        expect_query_set(&measured, QuerySet::Measured)?;
        if measured.to_tsv() != measured_encoded {
            return Err("oracle bundle noncanonical measured section".into());
        }

        let claimed_digest = parse_digest_line(input, &mut offset, "bundle_digest")?;
        if offset != input.len() {
            return Err("oracle bundle trailing bytes".into());
        }
        let bundle = Self::from_sections(warmup, measured)?;
        if claimed_digest != bundle.digest {
            return Err("oracle bundle digest mismatch".into());
        }
        Ok(bundle)
    }
}

fn bundle_digest(warmup: &OracleSummary, measured: &OracleSummary) -> [u8; 32] {
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 oracle-bundle-v1");
    digest.update(&warmup.profile().entities().to_be_bytes());
    digest.update(&warmup.profile().relationships().to_be_bytes());
    digest.update(&warmup.digest());
    digest.update(&measured.digest());
    digest.update(&(warmup.expectations().len() as u64).to_be_bytes());
    digest.update(&(measured.expectations().len() as u64).to_be_bytes());
    *digest.finalize().as_bytes()
}

fn expect_query_set(summary: &OracleSummary, expected: QuerySet) -> Result<(), String> {
    if summary.query_set() != expected {
        return Err("oracle bundle query set mismatch".into());
    }
    Ok(())
}

fn take_section<'a>(input: &'a str, offset: &mut usize, length: usize) -> Result<&'a str, String> {
    if length == 0 || length > crate::MAX_ORACLE_SUMMARY_BYTES {
        return Err("oracle bundle section size invalid".into());
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| "oracle bundle section size overflow".to_owned())?;
    let section = input
        .get(*offset..end)
        .ok_or_else(|| "oracle bundle section truncated".to_owned())?;
    *offset = end;
    Ok(section)
}

fn expect_line(
    input: &str,
    offset: &mut usize,
    expected_key: &str,
    expected_value: &str,
) -> Result<(), String> {
    let (key, value) = line_pair(input, offset)?;
    if key != expected_key || value != expected_value {
        return Err("oracle bundle header mismatch".into());
    }
    Ok(())
}

fn parse_size_line(input: &str, offset: &mut usize, expected_key: &str) -> Result<usize, String> {
    let (key, value) = line_pair(input, offset)?;
    if key != expected_key {
        return Err("oracle bundle header mismatch".into());
    }
    parse_usize(value)
}

fn parse_digest_line(
    input: &str,
    offset: &mut usize,
    expected_key: &str,
) -> Result<[u8; 32], String> {
    let (key, value) = line_pair(input, offset)?;
    if key != expected_key {
        return Err("oracle bundle header mismatch".into());
    }
    parse_hex(value)
}

fn line_pair<'a>(input: &'a str, offset: &mut usize) -> Result<(&'a str, &'a str), String> {
    let remainder = input
        .get(*offset..)
        .ok_or_else(|| "oracle bundle offset invalid".to_owned())?;
    let newline = remainder
        .find('\n')
        .ok_or_else(|| "oracle bundle line truncated".to_owned())?;
    let line = &remainder[..newline];
    *offset = offset
        .checked_add(newline + 1)
        .ok_or_else(|| "oracle bundle offset overflow".to_owned())?;
    let mut fields = line.split('\t');
    let key = fields
        .next()
        .ok_or_else(|| "oracle bundle line malformed".to_owned())?;
    let value = fields
        .next()
        .ok_or_else(|| "oracle bundle line malformed".to_owned())?;
    if fields.next().is_some() {
        return Err("oracle bundle line malformed".into());
    }
    Ok((key, value))
}

fn parse_usize(value: &str) -> Result<usize, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("oracle bundle noncanonical integer".into());
    }
    value
        .parse()
        .map_err(|_| "oracle bundle invalid integer".into())
}

fn parse_hex(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("oracle bundle invalid digest".into());
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(output)
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("oracle bundle invalid digest".into()),
    }
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn debug(error: impl core::fmt::Debug) -> String {
    format!("{error:?}")
}

#[cfg(test)]
mod tests {
    use super::{OracleBundle, QUALIFYING_ORACLE_BUNDLE_DIGEST, hex};
    use crate::{
        Bm01Profile, OracleExpectedOutcome, QUALIFYING_ORACLE_SUMMARY_DIGEST,
        QUALIFYING_WARMUP_SUMMARY_DIGEST, QuerySet,
    };

    #[test]
    fn scaled_bundle_round_trips_and_rejects_section_mutation() {
        let bundle = OracleBundle::build(Bm01Profile::new(20).unwrap()).unwrap();
        assert_eq!(bundle.warmup().query_set(), QuerySet::Warmup);
        assert_eq!(bundle.warmup().expectations().len(), 96);
        assert_eq!(bundle.measured().query_set(), QuerySet::Measured);
        assert_eq!(bundle.measured().expectations().len(), 384);
        let encoded = bundle.to_tsv();
        assert_eq!(OracleBundle::parse(&encoded).unwrap(), bundle);
        assert!(!encoded.contains("root"));
        assert!(
            OracleBundle::parse(&encoded.replacen("warmup_bytes\t", "warmup_bytes\t+", 1)).is_err()
        );
        assert!(OracleBundle::parse(&(encoded + "extra\tfield\n")).is_err());
    }

    #[test]
    #[ignore = "exact-profile oracle generation is exercised as a release-profile acceptance command"]
    fn qualifying_bundle_outcomes_and_digests_are_golden() {
        let bundle = OracleBundle::build(Bm01Profile::qualifying()).unwrap();
        assert_eq!(outcomes(bundle.warmup()), (74, 0, 22));
        assert_eq!(outcomes(bundle.measured()), (299, 0, 85));
        assert_eq!(bundle.warmup().digest(), QUALIFYING_WARMUP_SUMMARY_DIGEST);
        assert_eq!(bundle.measured().digest(), QUALIFYING_ORACLE_SUMMARY_DIGEST);
        assert_eq!(bundle.digest(), QUALIFYING_ORACLE_BUNDLE_DIGEST);
        assert_eq!(
            hex(&QUALIFYING_ORACLE_BUNDLE_DIGEST),
            "d52869f24d635476f86374813e754be364d0d2df470544b71221c24b96145fae"
        );
        assert_eq!(OracleBundle::parse(&bundle.to_tsv()).unwrap(), bundle);
    }

    fn outcomes(summary: &crate::OracleSummary) -> (usize, usize, usize) {
        let mut successful = 0;
        let mut visit_limits = 0;
        let mut result_limits = 0;
        for expectation in summary.expectations() {
            match expectation.outcome {
                OracleExpectedOutcome::Output { .. } => successful += 1,
                OracleExpectedOutcome::VisitLimit => visit_limits += 1,
                OracleExpectedOutcome::ResultLimit => result_limits += 1,
            }
        }
        (successful, visit_limits, result_limits)
    }
}
