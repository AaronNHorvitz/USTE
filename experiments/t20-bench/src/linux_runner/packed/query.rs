use super::crypto_work::CryptoWork;
use super::query_cache::QueryCacheMode;
use super::*;
use crate::engine::execute_query_with;

pub fn query_correctness(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Pages,
        false,
    )
}

pub fn query_correctness_with_lookup(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Positive,
        false,
    )
}

pub fn query_correctness_with_ranges(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Range,
        false,
    )
}

pub fn query_correctness_wide(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Pages,
        true,
    )
}

pub fn query_correctness_wide_with_lookup(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Positive,
        true,
    )
}

pub fn query_correctness_wide_with_ranges(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    query_mode(
        root,
        password_file,
        oracle_file,
        profile,
        QueryCacheMode::Range,
        true,
    )
}

fn query_mode(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
    mode: QueryCacheMode,
    wide: bool,
) -> Result<String, LinuxRunnerError> {
    disk::validate_native_profile(profile)?;
    let summary = read_oracle_summary(oracle_file)?;
    validate_measured_summary(&summary, profile)?;
    let mut session = prepare(root, password_file, profile, "open")?;
    let reader = mode.reader_with_size(
        &session.coordinator,
        &session.policy,
        session
            .limits
            .read()
            .map_err(|_| error("USTE_BM01_LIMITS"))?,
        wide,
    )?;
    let setup = session.filesystem.snapshot()?;
    let setup_crypto = CryptoWork::from(
        session
            .coordinator
            .vault_decrypt_report()
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))?,
    );
    let started = Instant::now();
    let mut successes = 0_u64;
    let mut visit_limits = 0_u64;
    let mut result_limits = 0_u64;
    let mut visits = 0_u64;
    let mut result_bytes = 0_u64;
    let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-query-v1");
    for expected in summary.expectations() {
        reader
            .clear_cache(&session.principal)
            .map_err(|_| error("USTE_BM01_PACKED_CACHE"))?;
        let actual = execute_query_with(Materializer::new(profile), expected.query, |request| {
            reader
                .read(
                    &mut session.filesystem,
                    &session.principal,
                    request,
                    &NeverCancel,
                )
                .map_err(|_| EngineQueryError::Engine("USTE_BM01_PACKED_QUERY".into()))
        });
        aggregate.update(&[
            expected.query.class.code(),
            expected.query.direction.code(),
            expected.query.depth,
        ]);
        aggregate.update(&expected.query.ordinal.to_be_bytes());
        match (expected.outcome, actual) {
            (
                OracleExpectedOutcome::Output {
                    visits: expected_visits,
                    relationships,
                    entities,
                    logical_result_bytes: expected_bytes,
                    output_digest,
                },
                Ok(actual),
            ) => {
                let actual_visits =
                    u64::try_from(actual.visits).map_err(|_| error("USTE_BM01_QUERY_RESULT"))?;
                let bytes =
                    logical_result_bytes(&actual).map_err(|_| error("USTE_BM01_QUERY_RESULT"))?;
                if actual_visits != expected_visits
                    || actual.relationships.len() as u64 != relationships
                    || actual.reachable_entities.len() as u64 != entities
                    || bytes != expected_bytes
                    || actual.digest() != output_digest
                {
                    return Err(error("USTE_BM01_QUERY_MISMATCH"));
                }
                successes += 1;
                visits = visits
                    .checked_add(actual_visits)
                    .ok_or_else(|| error("USTE_BM01_QUERY_RESULT"))?;
                result_bytes = result_bytes
                    .checked_add(bytes)
                    .ok_or_else(|| error("USTE_BM01_QUERY_RESULT"))?;
                aggregate.update(&[1]);
                aggregate.update(&output_digest);
            }
            (OracleExpectedOutcome::VisitLimit, Err(EngineQueryError::VisitLimit)) => {
                visit_limits += 1;
                aggregate.update(&[2]);
            }
            (OracleExpectedOutcome::ResultLimit, Err(EngineQueryError::ResultLimit)) => {
                result_limits += 1;
                aggregate.update(&[3]);
            }
            _ => return Err(error("USTE_BM01_QUERY_MISMATCH")),
        }
    }
    let cache = reader
        .cache_report(&session.principal)
        .map_err(|_| error("USTE_BM01_PACKED_CACHE"))?
        .ok_or_else(|| error("USTE_BM01_PACKED_CACHE"))?;
    let (rss, peak) = process_rss()?;
    let query_crypto = CryptoWork::from(
        session
            .coordinator
            .vault_decrypt_report()
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))?,
    )
    .delta(setup_crypto)?;
    Ok(serde_json::json!({
        "schema": if mode == QueryCacheMode::Range {
            "bm01-linux-packed-range-query-v1"
        } else {
            "bm01-linux-packed-query-v1"
        }, "engine_benchmark": false,
        "qualification": "nonqualifying-development-correctness", "filesystem_profile": "linux-x86_64-btrfs",
        "oracle_adjacency_memory_resident": false, "entities": profile.entities(), "relationships": profile.relationships(),
        "frontier": materialization_revision_count(profile), "queries": summary.expectations().len(),
        "successful_queries": successes, "expected_visit_limits": visit_limits, "expected_result_limits": result_limits,
        "visits": visits, "logical_result_bytes": result_bytes, "output_digest": hex(aggregate.finalize().as_bytes()),
        "oracle_summary_digest": hex(&summary.digest()), "query_milliseconds": started.elapsed().as_millis(),
        "current_rss_kib": rss, "process_peak_rss_kib": peak,
        "cache_budget_bytes": cache.budget_bytes, "cache_accounted_bytes": cache.accounted_bytes,
        "query_cache_configuration": mode.report_with_size(cache, wide)?,
        "cache_hits": cache.hits, "cache_misses": cache.misses, "cache_evictions": cache.evictions,
        "uste_page_cache": "cleared-before-each-query", "kernel_filesystem_device_cache": "uncontrolled",
        "preemptive_deadline_enforced": false, "complete_authenticated_io": false,
        "authenticated_io_accounting": "partial-single-owner-vault-decrypt",
        "query_vault_work": query_crypto.json(),
        "setup": session.report, "query_adapter_io": session.filesystem.snapshot()?.delta(setup)?.json()?,
    }).to_string())
}
