//! Packed native sampling with the frozen plan, oracle and owned-worker deadline protocol.
use super::crypto_work::CryptoWork;
use super::query_cache::{QueryCacheMode, Reader};
use super::*;
use crate::linux_runner::{
    disk::sampling::CacheWork,
    sampling::{
        CacheState, GroupKey, LatencyClass, MAX_TIMED_EXECUTIONS_PER_SAMPLE, OutcomeKind,
        ProtocolObserver, QUERY_DEADLINE, QueryObserver, SamplingPlan, ValidatedOutcome,
        latency_class_name, read_oracle_bundle, record_latency, summarize, validate_bundle,
        validate_outcome,
    },
};
use crate::{OracleExpectation, engine::execute_query_with};
use std::{collections::BTreeMap, time::Duration};
use uste_storage::packed_page_cache::PackedCacheReport;
use uste_txn::AuthorizedDiskCacheReport;
mod lookup_work;
use lookup_work::LookupWork;

pub fn sample_worker(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    sample_worker_mode(
        root,
        password,
        bundle,
        profile,
        QueryCacheMode::Pages,
        false,
    )
}

pub fn sample_worker_with_lookup(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    sample_worker_mode(
        root,
        password,
        bundle,
        profile,
        QueryCacheMode::Positive,
        false,
    )
}

pub fn sample_worker_wide(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    sample_worker_mode(root, password, bundle, profile, QueryCacheMode::Pages, true)
}
pub fn sample_worker_wide_with_lookup(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    sample_worker_mode(
        root,
        password,
        bundle,
        profile,
        QueryCacheMode::Positive,
        true,
    )
}

fn sample_worker_mode(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
    mode: QueryCacheMode,
    wide: bool,
) -> Result<(), LinuxRunnerError> {
    let mut observer = ProtocolObserver::new();
    match sample(root, password, bundle, profile, mode, wide, &mut observer) {
        Ok(report) => observer.report(&report),
        Err(error) => {
            let _ = observer.error(error.code());
            Err(error)
        }
    }
}
fn sample(
    root: &Path,
    password: &Path,
    bundle: &Path,
    profile: Bm01Profile,
    mode: QueryCacheMode,
    wide: bool,
    observer: &mut dyn QueryObserver,
) -> Result<String, LinuxRunnerError> {
    disk::validate_native_profile(profile)?;
    let bundle = read_oracle_bundle(bundle)?;
    validate_bundle(&bundle, profile)?;
    let mut session = prepare(root, password, profile, "open")?;
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
    let mut engine = Engine {
        reader,
        coordinator: &session.coordinator,
        filesystem: &mut session.filesystem,
        principal: &session.principal,
        materializer: Materializer::new(profile),
    };
    let setup_cache = engine.report()?;
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
    let warmup_io = engine.filesystem.snapshot()?.delta(setup)?;
    let warmup_crypto = engine.crypto_report()?.delta(setup_crypto)?;
    let mut warmup_lookup = LookupWork::default();
    warmup_lookup.add(setup_cache.lookup, engine.report()?.lookup)?;
    let plan = SamplingPlan::for_profile(profile);
    let mut samples = Vec::with_capacity(plan.samples);
    for ordinal in 1..=plan.samples {
        samples.push(engine.sample(bundle.measured().expectations(), plan, ordinal, observer)?);
    }
    Ok(serde_json::json!({
        "schema": match (mode, wide) {
            (QueryCacheMode::Pages, false) => "bm01-linux-packed-sampling-v1",
            (QueryCacheMode::Positive, false) => "bm01-linux-packed-lookup-sampling-v1",
            (QueryCacheMode::Range, false) => "bm01-linux-packed-range-sampling-v1",
            (QueryCacheMode::Pages, true) => "bm01-linux-packed-wide-sampling-v1",
            (QueryCacheMode::Positive, true) => "bm01-linux-packed-wide-lookup-sampling-v1",
            (QueryCacheMode::Range, true) => "bm01-linux-packed-wide-range-sampling-v1",
        }, "engine_benchmark": true,
        "qualification": "nonqualifying-development-sampling", "budget_evaluation": "not-performed",
        "filesystem_profile": "linux-x86_64-btrfs", "oracle_profile": "bm01-oracle-bundle-v1",
        "result_size_profile": "bm01-result-v1", "oracle_adjacency_memory_resident": false,
        "cache_pairing": "empty-then-retained-identical-query", "kernel_filesystem_device_cache": "uncontrolled",
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "storage_metadata_mode": "disk-certificate-and-blob-recovery", "complete_authenticated_io": false,
        "authenticated_io_accounting": "partial-single-owner-vault-decrypt", "adapter_io_accounting": "filesystem-adapter-calls",
        "setup_vault_work": setup_crypto.json(), "setup_vault_work_scope": "last-cold-open-owner-only",
        "warmup_vault_work": warmup_crypto.json(),
        "warmup_lookup_cache_work": warmup_lookup.json(CacheState::Empty)?,
        "query_cache_configuration": mode.report_with_size(engine.report()?, wide)?,
        "setup": session.report, "setup_adapter_io": setup.json()?, "warmup_adapter_io": warmup_io.json()?,
        "query_deadline_seconds": 30, "query_deadline_enforced": false, "query_deadline_postchecked": true,
        "maximum_timed_executions_per_sample": MAX_TIMED_EXECUTIONS_PER_SAMPLE,
        "entities": profile.entities(), "relationships": profile.relationships(),
        "frontier": materialization_revision_count(profile), "development_entity_limit": disk::MAX_NATIVE_DEVELOPMENT_ENTITIES,
        "warmup": { "queries": bundle.warmup().expectations().len(), "successes": warmup[0], "visit_limits": warmup[1], "result_limits": warmup[2] },
        "oracle_bundle_digest": hex(&bundle.digest()), "samples": samples,
    }).to_string())
}

struct Engine<'a> {
    reader: Reader<'a>,
    coordinator: &'a Packed,
    filesystem: &'a mut Fs,
    principal: &'a AuthenticatedPrincipal,
    materializer: Materializer,
}
impl Engine<'_> {
    fn crypto_report(&self) -> Result<CryptoWork, LinuxRunnerError> {
        self.coordinator
            .vault_decrypt_report()
            .map(CryptoWork::from)
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))
    }
    fn clear(&self) -> Result<(), LinuxRunnerError> {
        self.reader
            .clear_cache(self.principal)
            .map_err(|_| error("USTE_BM01_PACKED_CACHE"))
    }
    fn report(&self) -> Result<PackedCacheReport, LinuxRunnerError> {
        let cache = self
            .reader
            .cache_report(self.principal)
            .map_err(|_| error("USTE_BM01_PACKED_CACHE"))?
            .ok_or_else(|| error("USTE_BM01_PACKED_CACHE"))?;
        Ok(cache)
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
                .map_err(|_| EngineQueryError::Engine("USTE_BM01_PACKED_QUERY".into()))
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
        let mut lookup_work = [LookupWork::default(); 2];
        let mut io_work = [disk::io::IoSnapshot::default(); 2];
        let mut crypto_work = [CryptoWork::default(); 2];
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
                    let before_io = self.filesystem.snapshot()?;
                    let before_crypto = self.crypto_report()?;
                    let (outcome, elapsed) = self.execute(expected, observer)?;
                    let after = self.report()?;
                    work[cache.index()].add(
                        outcome,
                        page_counters(before),
                        page_counters(after),
                    )?;
                    lookup_work[cache.index()].add(before.lookup, after.lookup)?;
                    io_work[cache.index()]
                        .accumulate(self.filesystem.snapshot()?.delta(before_io)?)?;
                    crypto_work[cache.index()]
                        .accumulate(self.crypto_report()?.delta(before_crypto)?)?;
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
        let (rss, peak) = process_rss()?;
        let latencies: Vec<_> = summarize(groups).into_iter().map(|latency| serde_json::json!({
            "cache": latency.cache.name(), "outcome": latency.outcome.name(), "class": latency_class_name(latency.class),
            "depth": latency.depth, "count": latency.count, "p50_nanoseconds": latency.p50_nanoseconds,
            "p95_nanoseconds": latency.p95_nanoseconds, "p99_nanoseconds": latency.p99_nanoseconds,
        })).collect();
        Ok(serde_json::json!({
            "sample": ordinal, "minimum_duration_milliseconds": plan.minimum_duration.as_millis(),
            "elapsed_milliseconds": started.elapsed().as_millis(), "rounds": rounds, "timed_executions": executions,
            "current_rss_kib": rss, "process_peak_rss_kib": peak, "output_digest": hex(aggregate.finalize().as_bytes()),
            "cache_work": [work[0].json(CacheState::Empty)?, work[1].json(CacheState::Retained)?],
            "lookup_cache_work": [lookup_work[0].json(CacheState::Empty)?, lookup_work[1].json(CacheState::Retained)?],
            "adapter_io": [{"cache": CacheState::Empty.name(), "work": io_work[0].json()?},
                {"cache": CacheState::Retained.name(), "work": io_work[1].json()?}], "latency_groups": latencies,
            "vault_work": [{"cache": CacheState::Empty.name(), "work": crypto_work[0].json()},
                {"cache": CacheState::Retained.name(), "work": crypto_work[1].json()}],
        }))
    }
}

fn page_counters(cache: PackedCacheReport) -> AuthorizedDiskCacheReport {
    // The total includes result-cache residency; observations are strictly page-only.
    // Separate LookupWork records positive-cache deltas, never fabricated proof/device work.
    AuthorizedDiskCacheReport {
        budget_bytes: cache.budget_bytes,
        accounted_bytes: cache.accounted_bytes,
        hits: cache.hits,
        misses: cache.misses,
        evictions: cache.evictions,
    }
}
