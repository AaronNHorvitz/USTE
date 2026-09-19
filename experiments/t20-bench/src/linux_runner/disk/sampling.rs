//! Development disk sampling with shared frozen plans, validators and worker protocol.
use super::*;
use crate::linux_runner::sampling::{
    CacheState, GroupKey, LatencyClass, MAX_TIMED_EXECUTIONS_PER_SAMPLE, OutcomeKind,
    ProtocolObserver, QUERY_DEADLINE, QueryObserver, SamplingPlan, ValidatedOutcome,
    latency_class_name, read_oracle_bundle, record_latency, summarize, validate_bundle,
    validate_outcome,
};
use crate::{OracleExpectation, engine::execute_query_with};
use std::{collections::BTreeMap, time::Duration};
use uste_graph::GraphDiskExpansionLimits;
use uste_txn::AuthorizedDiskCacheReport;

type Reader<'a> = AuthorizedDiskReader<
    'a,
    GraphDiskLiveState,
    LinuxFileSystem,
    RecoveryEnvelope,
    OsEntropy,
    OsEntropy,
>;

pub fn sample_worker(
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let mut observer = ProtocolObserver::new();
    match sample(root, password_file, bundle_file, profile, &mut observer) {
        Ok(report) => observer.report(&report),
        Err(error) => {
            let _ = observer.error(error.code());
            Err(error)
        }
    }
}

fn sample(
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
    observer: &mut dyn QueryObserver,
) -> Result<String, LinuxRunnerError> {
    let bundle = read_oracle_bundle(bundle_file)?;
    validate_bundle(&bundle, profile)?;
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
    let mut engine = Engine {
        reader,
        filesystem: &mut session.filesystem,
        principal: &session.principal,
        materializer: Materializer::new(profile),
    };
    let mut warmup = [0_usize; 3];
    for expected in bundle.warmup().expectations() {
        engine.clear()?;
        let (outcome, _) = engine.execute(expected, observer)?;
        warmup[match outcome.kind {
            OutcomeKind::Success => 0,
            OutcomeKind::VisitLimit => 1,
            OutcomeKind::ResultLimit => 2,
        }] += 1;
    }
    let plan = SamplingPlan::for_profile(profile);
    let mut samples = Vec::with_capacity(plan.samples);
    for ordinal in 1..=plan.samples {
        samples.push(engine.sample(bundle.measured().expectations(), plan, ordinal, observer)?);
    }
    Ok(serde_json::json!({
        "schema": "bm01-linux-disk-sampling-v1", "engine_benchmark": true,
        "qualification": "nonqualifying-development-sampling",
        "filesystem_profile": "linux-x86_64-btrfs", "oracle_profile": "bm01-oracle-bundle-v1",
        "result_size_profile": "bm01-result-v1", "oracle_adjacency_memory_resident": false,
        "cache_pairing": "empty-then-retained-identical-query", "kernel_filesystem_device_cache": "uncontrolled",
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "storage_metadata_memory_resident": true, "authenticated_io_accounting": "not-measured",
        "query_deadline_seconds": 30, "query_deadline_enforced": false, "query_deadline_postchecked": true,
        "maximum_timed_executions_per_sample": MAX_TIMED_EXECUTIONS_PER_SAMPLE,
        "budget_evaluation": "not-performed", "entities": profile.entities(),
        "relationships": profile.relationships(), "frontier": session.frontier,
        "setup_milliseconds": session.setup_elapsed.as_millis(),
        "warmup": { "queries": bundle.warmup().expectations().len(), "successes": warmup[0],
            "visit_limits": warmup[1], "result_limits": warmup[2] },
        "oracle_bundle_digest": hex(&bundle.digest()), "samples": samples,
    }).to_string())
}

struct Engine<'a> {
    reader: Reader<'a>,
    filesystem: &'a mut LinuxFileSystem,
    principal: &'a AuthenticatedPrincipal,
    materializer: Materializer,
}
impl Engine<'_> {
    fn clear(&self) -> Result<(), LinuxRunnerError> {
        self.reader
            .clear_cache(self.principal)
            .map_err(|_| error("USTE_BM01_INDEX_CLEAR"))
    }
    fn report(&self) -> Result<AuthorizedDiskCacheReport, LinuxRunnerError> {
        self.reader
            .cache_report(self.principal)
            .map_err(|_| error("USTE_BM01_INDEX_REPORT"))
    }
    fn execute(
        &mut self,
        expected: &OracleExpectation,
        observer: &mut dyn QueryObserver,
    ) -> Result<(ValidatedOutcome, Duration), LinuxRunnerError> {
        observer.query_started()?;
        let started = Instant::now();
        let actual = execute_query_with(self.materializer, expected.query, |request| {
            self.reader
                .read(self.filesystem, self.principal, request, &NeverCancel)
                .map_err(|_| EngineQueryError::Engine("USTE_BM01_DISK_QUERY".into()))
        });
        let elapsed = started.elapsed();
        observer.query_finished()?;
        if elapsed > QUERY_DEADLINE {
            return Err(error("USTE_BM01_QUERY_DEADLINE"));
        }
        Ok((validate_outcome(expected, actual)?, elapsed))
    }
    fn sample(
        &mut self,
        expectations: &[OracleExpectation],
        plan: SamplingPlan,
        ordinal: usize,
        observer: &mut dyn QueryObserver,
    ) -> Result<serde_json::Value, LinuxRunnerError> {
        let started = Instant::now();
        let mut rounds = 0;
        let mut groups = BTreeMap::new();
        let mut executions = 0;
        let mut work = [CacheWork::default(); 2];
        let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-sampling-v1");
        aggregate.update(&(ordinal as u64).to_be_bytes());
        loop {
            for expected in expectations {
                self.clear()?;
                for cache in [CacheState::Empty, CacheState::Retained] {
                    if executions == MAX_TIMED_EXECUTIONS_PER_SAMPLE {
                        return Err(error("USTE_BM01_SAMPLE_OBSERVATIONS"));
                    }
                    let before = self.report()?;
                    let (outcome, elapsed) = self.execute(expected, observer)?;
                    work[cache.index()].add(outcome, before, self.report()?)?;
                    executions += 1;
                    aggregate.update(&[
                        cache.code(),
                        outcome.kind.code(),
                        expected.query.class.code(),
                        expected.query.direction.code(),
                        expected.query.depth,
                    ]);
                    aggregate.update(&expected.query.ordinal.to_be_bytes());
                    aggregate.update(&outcome.digest);
                    for class in [LatencyClass::All, LatencyClass::Query(expected.query.class)] {
                        record_latency(
                            &mut groups,
                            GroupKey {
                                cache,
                                outcome: outcome.kind,
                                class,
                                depth: expected.query.depth,
                            },
                            elapsed.as_nanos(),
                        )?;
                    }
                }
            }
            rounds += 1;
            if started.elapsed() >= plan.minimum_duration {
                break;
            }
        }
        let elapsed = started.elapsed();
        let (rss, peak) = process_rss()?;
        let latencies: Vec<_> = summarize(groups).into_iter().map(|latency| serde_json::json!({
            "cache": latency.cache.name(), "outcome": latency.outcome.name(),
            "class": latency_class_name(latency.class), "depth": latency.depth, "count": latency.count,
            "p50_nanoseconds": latency.p50_nanoseconds, "p95_nanoseconds": latency.p95_nanoseconds,
            "p99_nanoseconds": latency.p99_nanoseconds,
        })).collect();
        Ok(
            serde_json::json!({ "sample": ordinal, "minimum_duration_milliseconds": plan.minimum_duration.as_millis(),
                "elapsed_milliseconds": elapsed.as_millis(), "rounds": rounds, "timed_executions": executions,
                "current_rss_kib": rss, "process_peak_rss_kib": peak, "output_digest": hex(aggregate.finalize().as_bytes()),
                "cache_work": [work[0].json(CacheState::Empty)?, work[1].json(CacheState::Retained)?],
                "latency_groups": latencies,
            }),
        )
    }
}

#[derive(Clone, Copy, Default)]
struct CacheWork {
    visits: u64,
    bytes: u64,
    cache: Option<AuthorizedDiskCacheReport>,
}
impl CacheWork {
    fn add(
        &mut self,
        outcome: ValidatedOutcome,
        before: AuthorizedDiskCacheReport,
        after: AuthorizedDiskCacheReport,
    ) -> Result<(), LinuxRunnerError> {
        if before.budget_bytes != after.budget_bytes
            || self
                .cache
                .is_some_and(|cache| cache.budget_bytes != after.budget_bytes)
        {
            return Err(error("USTE_BM01_INDEX_COUNTER"));
        }
        let delta = |old: u64, new: u64, accumulated: u64| {
            new.checked_sub(old)
                .and_then(|value| accumulated.checked_add(value))
                .ok_or_else(|| error("USTE_BM01_INDEX_COUNTER"))
        };
        let current = self.cache.unwrap_or(AuthorizedDiskCacheReport {
            budget_bytes: after.budget_bytes,
            accounted_bytes: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        });
        let updated = AuthorizedDiskCacheReport {
            hits: delta(before.hits, after.hits, current.hits)?,
            misses: delta(before.misses, after.misses, current.misses)?,
            evictions: delta(before.evictions, after.evictions, current.evictions)?,
            ..after
        };
        self.visits = self
            .visits
            .checked_add(outcome.visits)
            .ok_or_else(|| error("USTE_BM01_INDEX_COUNTER"))?;
        self.bytes = self
            .bytes
            .checked_add(outcome.logical_result_bytes)
            .ok_or_else(|| error("USTE_BM01_INDEX_COUNTER"))?;
        self.cache = Some(updated);
        Ok(())
    }
    fn json(self, state: CacheState) -> Result<serde_json::Value, LinuxRunnerError> {
        let cache = self
            .cache
            .ok_or_else(|| error("USTE_BM01_SAMPLE_OBSERVATIONS"))?;
        Ok(
            serde_json::json!({ "cache": state.name(), "successful_visits": self.visits,
                "successful_logical_result_bytes": self.bytes, "index_cache_budget_bytes": cache.budget_bytes,
                "index_cache_accounted_bytes": cache.accounted_bytes, "index_cache_hits": cache.hits,
                "index_cache_misses": cache.misses, "index_cache_evictions": cache.evictions,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_cache_deltas_are_checked_and_never_invent_io_counters() {
        let before = AuthorizedDiskCacheReport {
            budget_bytes: 1024,
            accounted_bytes: 100,
            hits: 10,
            misses: 20,
            evictions: 30,
        };
        let after = AuthorizedDiskCacheReport {
            accounted_bytes: 90,
            hits: 12,
            misses: 23,
            evictions: 34,
            ..before
        };
        let outcome = ValidatedOutcome {
            kind: OutcomeKind::Success,
            visits: 5,
            logical_result_bytes: 7,
            digest: [0; 32],
        };
        let mut work = CacheWork::default();
        work.add(outcome, before, after).unwrap();
        work.add(outcome, before, after).unwrap();
        let json = work.json(CacheState::Empty).unwrap();
        assert_eq!(json["index_cache_hits"], 4);
        assert_eq!(json["index_cache_misses"], 6);
        assert_eq!(json["index_cache_evictions"], 8);
        assert_eq!(json["index_cache_accounted_bytes"], 90);
        assert_eq!(json["successful_visits"], 10);
        assert!(json.get("index_pages_read").is_none());
        assert!(work.add(outcome, after, before).is_err());
        assert!(
            work.add(
                outcome,
                before,
                AuthorizedDiskCacheReport {
                    budget_bytes: 2048,
                    ..after
                }
            )
            .is_err()
        );
        work.cache.as_mut().unwrap().hits = u64::MAX;
        assert!(work.add(outcome, before, after).is_err());
    }

    #[test]
    fn native_sampling_does_not_relabel_engine_errors_as_expected_limits() {
        let expected = OracleExpectation {
            query: crate::measured_queries(Bm01Profile::new(20).unwrap())[0],
            outcome: OracleExpectedOutcome::VisitLimit,
        };
        assert!(validate_outcome(&expected, Err(EngineQueryError::VisitLimit)).is_ok());
        assert!(validate_outcome(&expected, Err(EngineQueryError::ResultLimit)).is_err());
        assert!(
            validate_outcome(&expected, Err(EngineQueryError::Engine("refused".into()))).is_err()
        );
    }
}
