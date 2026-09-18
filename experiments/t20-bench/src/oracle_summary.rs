//! Content-free, separately generated BM-01 query expectations.

use std::fmt::Write;

use crate::{
    Bm01Profile, Oracle, OracleError, OracleLimits, OracleOutput, QuerySpec, engine_mapping_digest,
    measured_queries, query_digest,
};

pub const ORACLE_SUMMARY_PROFILE: &str = "bm01-oracle-summary-v1";
pub const RESULT_SIZE_PROFILE: &str = "bm01-result-v1";
pub const MAX_ORACLE_SUMMARY_BYTES: usize = 256 * 1024;
pub const QUALIFYING_ORACLE_SUMMARY_DIGEST: [u8; 32] = [
    94, 156, 184, 18, 0, 178, 1, 106, 180, 112, 65, 144, 33, 86, 30, 10, 48, 78, 30, 177, 214, 97,
    14, 6, 51, 179, 89, 37, 178, 125, 244, 2,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OracleExpectedOutcome {
    Output {
        visits: u64,
        relationships: u64,
        entities: u64,
        logical_result_bytes: u64,
        output_digest: [u8; 32],
    },
    VisitLimit,
    ResultLimit,
}

impl OracleExpectedOutcome {
    const fn code(self) -> u8 {
        match self {
            Self::Output { .. } => 1,
            Self::VisitLimit => 2,
            Self::ResultLimit => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OracleExpectation {
    pub query: QuerySpec,
    pub outcome: OracleExpectedOutcome,
}

#[derive(Clone, Copy)]
struct OutputFields {
    pub visits: u64,
    pub relationships: u64,
    pub entities: u64,
    pub logical_result_bytes: u64,
    pub output_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleSummary {
    profile: Bm01Profile,
    expectations: Vec<OracleExpectation>,
    digest: [u8; 32],
}

impl OracleSummary {
    pub fn build(profile: Bm01Profile) -> Result<Self, String> {
        let oracle = Oracle::build(profile).map_err(debug)?;
        let mut expectations = Vec::with_capacity(measured_queries(profile).len());
        for query in measured_queries(profile) {
            let outcome = match oracle.expand(query, OracleLimits::default()) {
                Ok(output) => expectation(&output)?,
                Err(OracleError::VisitLimit) => OracleExpectedOutcome::VisitLimit,
                Err(OracleError::ResultLimit) => OracleExpectedOutcome::ResultLimit,
                Err(error) => return Err(debug(error)),
            };
            expectations.push(OracleExpectation { query, outcome });
        }
        let digest = summary_digest(profile, &expectations);
        Ok(Self {
            profile,
            expectations,
            digest,
        })
    }

    #[must_use]
    pub fn profile(&self) -> Bm01Profile {
        self.profile
    }

    #[must_use]
    pub fn expectations(&self) -> &[OracleExpectation] {
        &self.expectations
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[must_use]
    pub fn to_tsv(&self) -> String {
        let mut output = String::new();
        writeln!(&mut output, "schema\t{ORACLE_SUMMARY_PROFILE}").unwrap();
        writeln!(&mut output, "result_size_profile\t{RESULT_SIZE_PROFILE}").unwrap();
        writeln!(&mut output, "entities\t{}", self.profile.entities()).unwrap();
        writeln!(
            &mut output,
            "relationships\t{}",
            self.profile.relationships()
        )
        .unwrap();
        writeln!(
            &mut output,
            "engine_mapping_digest\t{}",
            hex(&engine_mapping_digest(self.profile))
        )
        .unwrap();
        writeln!(
            &mut output,
            "measured_query_digest\t{}",
            hex(&query_digest(self.profile, crate::QuerySet::Measured))
        )
        .unwrap();
        writeln!(&mut output, "queries\t{}", self.expectations.len()).unwrap();
        for (index, expected) in self.expectations.iter().enumerate() {
            let (status, fields) = match expected.outcome {
                OracleExpectedOutcome::Output {
                    visits,
                    relationships,
                    entities,
                    logical_result_bytes,
                    output_digest,
                } => (
                    "ok",
                    OutputFields {
                        visits,
                        relationships,
                        entities,
                        logical_result_bytes,
                        output_digest,
                    },
                ),
                OracleExpectedOutcome::VisitLimit => ("visit-limit", OutputFields::zero()),
                OracleExpectedOutcome::ResultLimit => ("result-limit", OutputFields::zero()),
            };
            writeln!(
                &mut output,
                "query\t{index}\t{}\t{}\t{}\t{}\t{status}\t{}\t{}\t{}\t{}\t{}",
                expected.query.class.code(),
                expected.query.direction.code(),
                expected.query.depth,
                expected.query.ordinal,
                fields.visits,
                fields.relationships,
                fields.entities,
                fields.logical_result_bytes,
                hex(&fields.output_digest),
            )
            .unwrap();
        }
        writeln!(&mut output, "summary_digest\t{}", hex(&self.digest)).unwrap();
        output
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        if input.len() > MAX_ORACLE_SUMMARY_BYTES || input.contains('\r') {
            return Err("invalid oracle summary envelope".into());
        }
        let mut lines = input.lines();
        expect_pair(&mut lines, "schema", ORACLE_SUMMARY_PROFILE)?;
        expect_pair(&mut lines, "result_size_profile", RESULT_SIZE_PROFILE)?;
        let entities = parse_pair_u64(&mut lines, "entities")?;
        let relationships = parse_pair_u64(&mut lines, "relationships")?;
        let profile = Bm01Profile::new(entities).map_err(str::to_owned)?;
        if profile.relationships() != relationships {
            return Err("oracle summary relationship count mismatch".into());
        }
        let mapping = parse_pair_digest(&mut lines, "engine_mapping_digest")?;
        if mapping != engine_mapping_digest(profile) {
            return Err("oracle summary engine mapping mismatch".into());
        }
        let queries_digest = parse_pair_digest(&mut lines, "measured_query_digest")?;
        if queries_digest != query_digest(profile, crate::QuerySet::Measured) {
            return Err("oracle summary query corpus mismatch".into());
        }
        let count = parse_pair_usize(&mut lines, "queries")?;
        let queries = measured_queries(profile);
        if count != queries.len() {
            return Err("oracle summary query count mismatch".into());
        }
        let mut expectations = Vec::with_capacity(count);
        for (index, query) in queries.into_iter().enumerate() {
            let line = lines
                .next()
                .ok_or_else(|| "oracle summary missing query".to_owned())?;
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 12
                || fields[0] != "query"
                || parse::<usize>(fields[1])? != index
                || parse::<u8>(fields[2])? != query.class.code()
                || parse::<u8>(fields[3])? != query.direction.code()
                || parse::<u8>(fields[4])? != query.depth
                || parse::<u64>(fields[5])? != query.ordinal
            {
                return Err("oracle summary query identity mismatch".into());
            }
            let output = OutputFields {
                visits: parse(fields[7])?,
                relationships: parse(fields[8])?,
                entities: parse(fields[9])?,
                logical_result_bytes: parse(fields[10])?,
                output_digest: parse_hex(fields[11])?,
            };
            let outcome = match fields[6] {
                "ok" => OracleExpectedOutcome::Output {
                    visits: output.visits,
                    relationships: output.relationships,
                    entities: output.entities,
                    logical_result_bytes: output.logical_result_bytes,
                    output_digest: output.output_digest,
                },
                "visit-limit" if output.is_zero() => OracleExpectedOutcome::VisitLimit,
                "result-limit" if output.is_zero() => OracleExpectedOutcome::ResultLimit,
                _ => return Err("oracle summary outcome mismatch".into()),
            };
            expectations.push(OracleExpectation { query, outcome });
        }
        let digest = parse_pair_digest(&mut lines, "summary_digest")?;
        if lines.next().is_some() || digest != summary_digest(profile, &expectations) {
            return Err("oracle summary digest mismatch".into());
        }
        Ok(Self {
            profile,
            expectations,
            digest,
        })
    }
}

fn expectation(output: &OracleOutput) -> Result<OracleExpectedOutcome, String> {
    Ok(OracleExpectedOutcome::Output {
        visits: u64::try_from(output.visits).map_err(|_| "oracle visit count overflow")?,
        relationships: u64::try_from(output.relationships.len())
            .map_err(|_| "oracle relationship count overflow")?,
        entities: u64::try_from(output.reachable_entities.len())
            .map_err(|_| "oracle entity count overflow")?,
        logical_result_bytes: logical_result_bytes(output)?,
        output_digest: output.digest(),
    })
}

pub(crate) fn logical_result_bytes(output: &OracleOutput) -> Result<u64, String> {
    let ordinals = output
        .relationships
        .len()
        .checked_add(output.reachable_entities.len())
        .ok_or("logical result size overflow")?;
    let ordinal_bytes = u64::try_from(ordinals)
        .map_err(|_| "logical result size overflow")?
        .checked_mul(8)
        .ok_or("logical result size overflow")?;
    // bm01-result-v1: visits, relationship count and entity count as u64, followed by the two
    // ordered u64 ordinal arrays. The comparison digest is metadata, not response payload.
    24_u64
        .checked_add(ordinal_bytes)
        .ok_or_else(|| "logical result size overflow".into())
}

fn summary_digest(profile: Bm01Profile, entries: &[OracleExpectation]) -> [u8; 32] {
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 oracle-summary-v1");
    digest.update(&engine_mapping_digest(profile));
    digest.update(&query_digest(profile, crate::QuerySet::Measured));
    digest.update(&(entries.len() as u64).to_be_bytes());
    for entry in entries {
        digest.update(&[
            entry.query.class.code(),
            entry.query.direction.code(),
            entry.query.depth,
        ]);
        digest.update(&entry.query.ordinal.to_be_bytes());
        digest.update(&[entry.outcome.code()]);
        let fields = match entry.outcome {
            OracleExpectedOutcome::Output {
                visits,
                relationships,
                entities,
                logical_result_bytes,
                output_digest,
            } => OutputFields {
                visits,
                relationships,
                entities,
                logical_result_bytes,
                output_digest,
            },
            OracleExpectedOutcome::VisitLimit | OracleExpectedOutcome::ResultLimit => {
                OutputFields::zero()
            }
        };
        digest.update(&fields.visits.to_be_bytes());
        digest.update(&fields.relationships.to_be_bytes());
        digest.update(&fields.entities.to_be_bytes());
        digest.update(&fields.logical_result_bytes.to_be_bytes());
        digest.update(&fields.output_digest);
    }
    *digest.finalize().as_bytes()
}

fn expect_pair<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected_key: &str,
    expected_value: &str,
) -> Result<(), String> {
    let (key, value) = pair(lines)?;
    if key != expected_key || value != expected_value {
        return Err("oracle summary header mismatch".into());
    }
    Ok(())
}

fn parse_pair_u64<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected_key: &str,
) -> Result<u64, String> {
    let (key, value) = pair(lines)?;
    if key != expected_key {
        return Err("oracle summary header mismatch".into());
    }
    parse(value)
}

fn parse_pair_usize<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected_key: &str,
) -> Result<usize, String> {
    let value = parse_pair_u64(lines, expected_key)?;
    usize::try_from(value).map_err(|_| "oracle summary integer overflow".into())
}

fn parse_pair_digest<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected_key: &str,
) -> Result<[u8; 32], String> {
    let (key, value) = pair(lines)?;
    if key != expected_key {
        return Err("oracle summary header mismatch".into());
    }
    parse_hex(value)
}

fn pair<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Result<(&'a str, &'a str), String> {
    let line = lines
        .next()
        .ok_or_else(|| "oracle summary truncated".to_owned())?;
    let mut fields = line.split('\t');
    let key = fields
        .next()
        .ok_or_else(|| "oracle summary malformed pair".to_owned())?;
    let value = fields
        .next()
        .ok_or_else(|| "oracle summary malformed pair".to_owned())?;
    if fields.next().is_some() {
        return Err("oracle summary malformed pair".into());
    }
    Ok((key, value))
}

fn parse<T: core::str::FromStr>(value: &str) -> Result<T, String> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("oracle summary noncanonical integer".into());
    }
    value
        .parse()
        .map_err(|_| "oracle summary invalid integer".into())
}

fn parse_hex(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("oracle summary invalid digest".into());
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
        _ => Err("oracle summary invalid digest".into()),
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

impl OutputFields {
    const fn zero() -> Self {
        Self {
            visits: 0,
            relationships: 0,
            entities: 0,
            logical_result_bytes: 0,
            output_digest: [0; 32],
        }
    }

    fn is_zero(self) -> bool {
        self.visits == 0
            && self.relationships == 0
            && self.entities == 0
            && self.logical_result_bytes == 0
            && self.output_digest == [0; 32]
    }
}

#[cfg(test)]
mod tests {
    use super::{OracleExpectedOutcome, OracleSummary, QUALIFYING_ORACLE_SUMMARY_DIGEST, hex};
    use crate::Bm01Profile;

    #[test]
    fn scaled_summary_round_trips_and_rejects_mutation() {
        let summary = OracleSummary::build(Bm01Profile::new(20).unwrap()).unwrap();
        let encoded = summary.to_tsv();
        assert_eq!(OracleSummary::parse(&encoded).unwrap(), summary);
        assert_eq!(summary.expectations().len(), 384);
        assert!(!encoded.contains("root"));

        let mutated = encoded.replacen("\t384\n", "\t383\n", 1);
        assert!(OracleSummary::parse(&mutated).is_err());
        assert!(
            OracleSummary::parse(&encoded.replacen("entities\t20", "entities\t+20", 1)).is_err()
        );
        assert!(OracleSummary::parse(&(encoded + "extra\tfield\n")).is_err());
    }

    #[test]
    #[ignore = "exact-profile oracle generation is exercised as a release-profile acceptance command"]
    fn qualifying_summary_outcomes_and_digest_are_golden() {
        let summary = OracleSummary::build(Bm01Profile::qualifying()).unwrap();
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
        assert_eq!((successful, visit_limits, result_limits), (299, 0, 85));
        assert_eq!(summary.digest(), QUALIFYING_ORACLE_SUMMARY_DIGEST);
        assert_eq!(
            hex(&QUALIFYING_ORACLE_SUMMARY_DIGEST),
            "5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402"
        );
        assert_eq!(OracleSummary::parse(&summary.to_tsv()).unwrap(), summary);
    }
}
