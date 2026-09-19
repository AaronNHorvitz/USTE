//! Separate bounded oracle summary verification over the native disk reader.
use super::*;
use crate::{engine::execute_query_with, oracle_summary::logical_result_bytes};
use uste_graph::GraphDiskExpansionLimits;

pub fn query_correctness(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    let summary = read_oracle_summary(oracle_file)?;
    validate_measured_summary(&summary, profile)?;
    let (mut session, _) = prepare_session(root, password_file, profile, "open")?;
    let reader = AuthorizedDiskReader::new_with_cache_budget(
        &session.coordinator,
        &session.policy,
        GraphDiskReadLimits {
            current: IndexGetLimits::new(64, 16 * 1024).map_err(|_| error("USTE_BM01_LIMITS"))?,
            historical: IndexPredecessorLimits::new(64, 16 * 1024)
                .map_err(|_| error("USTE_BM01_LIMITS"))?,
            expansion: Some(
                GraphDiskExpansionLimits::new(1_000_000, 1_000_000, 64 * 1024 * 1024, 2_000_000)
                    .map_err(|_| error("USTE_BM01_LIMITS"))?,
            ),
        },
        64 * 1024 * 1024,
    )
    .map_err(|_| error("USTE_BM01_AUTHORIZED_OPEN"))?;
    let mut timings = Vec::with_capacity(summary.expectations().len());
    let mut successes = 0_u64;
    let mut visit_limits = 0_u64;
    let mut result_limits = 0_u64;
    let mut visits = 0_u64;
    let mut result_bytes = 0_u64;
    let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-query-v1");
    let materializer = Materializer::new(profile);
    let query_started = Instant::now();
    for expected in summary.expectations() {
        reader
            .clear_cache(&session.principal)
            .map_err(|_| error("USTE_BM01_INDEX_CLEAR"))?;
        let started = Instant::now();
        let actual = execute_query_with(materializer, expected.query, |request| {
            reader
                .read(
                    &mut session.filesystem,
                    &session.principal,
                    request,
                    &NeverCancel,
                )
                .map_err(|_| EngineQueryError::Engine("USTE_BM01_DISK_QUERY".into()))
        });
        timings.push(started.elapsed().as_nanos());
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
                let actual_relationships = u64::try_from(actual.relationships.len())
                    .map_err(|_| error("USTE_BM01_QUERY_RESULT"))?;
                let actual_entities = u64::try_from(actual.reachable_entities.len())
                    .map_err(|_| error("USTE_BM01_QUERY_RESULT"))?;
                let actual_bytes =
                    logical_result_bytes(&actual).map_err(|_| error("USTE_BM01_QUERY_RESULT"))?;
                let actual_digest = actual.digest();
                if actual_visits != expected_visits
                    || actual_relationships != relationships
                    || actual_entities != entities
                    || actual_bytes != expected_bytes
                    || actual_digest != output_digest
                {
                    return Err(error("USTE_BM01_QUERY_MISMATCH"));
                }
                successes += 1;
                visits = visits
                    .checked_add(actual_visits)
                    .ok_or_else(|| error("USTE_BM01_QUERY_RESULT"))?;
                result_bytes = result_bytes
                    .checked_add(actual_bytes)
                    .ok_or_else(|| error("USTE_BM01_QUERY_RESULT"))?;
                aggregate.update(&[1]);
                aggregate.update(&actual_digest);
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
    let elapsed = query_started.elapsed();
    let cache = reader
        .cache_report(&session.principal)
        .map_err(|_| error("USTE_BM01_INDEX_REPORT"))?;
    let (rss, peak) = process_rss()?;
    timings.sort_unstable();
    Ok(format!(
        concat!(
            "{{\"schema\":\"bm01-linux-disk-query-v1\",\"engine_benchmark\":false,",
            "\"qualification\":\"nonqualifying-development-correctness\",",
            "\"filesystem_profile\":\"linux-x86_64-btrfs\",\"oracle_adjacency_memory_resident\":false,",
            "\"full_memory_graph_state\":false,\"full_memory_coordinator_metadata\":false,",
            "\"storage_metadata_memory_resident\":true,\"uste_page_cache\":\"cleared-before-each-query\",",
            "\"kernel_filesystem_device_cache\":\"uncontrolled\",\"preemptive_deadline_enforced\":false,",
            "\"entities\":{},\"relationships\":{},\"frontier\":{},\"queries\":{},",
            "\"successful_queries\":{},\"expected_visit_limits\":{},\"expected_result_limits\":{},",
            "\"visits\":{},\"logical_result_bytes\":{},\"setup_milliseconds\":{},\"query_milliseconds\":{},",
            "\"p50_nanoseconds\":{},\"p95_nanoseconds\":{},\"p99_nanoseconds\":{},",
            "\"current_rss_kib\":{},\"process_peak_rss_kib\":{},\"oracle_summary_digest\":\"{}\",",
            "\"output_digest\":\"{}\",\"cache_budget_bytes\":{},\"cache_accounted_bytes\":{},",
            "\"cache_hits\":{},\"cache_misses\":{},\"cache_evictions\":{}}}"
        ),
        profile.entities(),
        profile.relationships(),
        session.frontier,
        summary.expectations().len(),
        successes,
        visit_limits,
        result_limits,
        visits,
        result_bytes,
        session.setup_elapsed.as_millis(),
        elapsed.as_millis(),
        percentile(&timings, 50),
        percentile(&timings, 95),
        percentile(&timings, 99),
        rss,
        peak,
        hex(&summary.digest()),
        hex(aggregate.finalize().as_bytes()),
        cache.budget_bytes,
        cache.accounted_bytes,
        cache.hits,
        cache.misses,
        cache.evictions
    ))
}
