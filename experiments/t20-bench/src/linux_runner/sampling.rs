//! Repeated, correctness-checked BM-01 sampling over the production Linux engine path.

use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use uste_graph::GraphIndexCacheReport;

use crate::{
    Bm01Profile, Materializer, OracleBundle, OracleExpectation, OracleExpectedOutcome,
    QUALIFYING_ORACLE_BUNDLE_DIGEST, QueryClass,
    engine::{EngineQueryError, execute_query},
    oracle_summary::logical_result_bytes,
};

use super::{LinuxRunnerError, Opened, cache_delta, hex, open_completed, process_rss};

const QUALIFYING_SAMPLES: usize = 5;
const QUALIFYING_SAMPLE_SECONDS: u64 = 60;
const MAX_TIMED_EXECUTIONS_PER_SAMPLE: usize = 2_000_000;
const QUERY_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CacheState {
    Empty,
    Retained,
}

impl CacheState {
    const fn name(self) -> &'static str {
        match self {
            Self::Empty => "uste-empty",
            Self::Retained => "uste-retained-after-identical-query",
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Empty => 1,
            Self::Retained => 2,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Retained => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum OutcomeKind {
    Success,
    VisitLimit,
    ResultLimit,
}

impl OutcomeKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::VisitLimit => "visit-limit",
            Self::ResultLimit => "result-limit",
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Success => 1,
            Self::VisitLimit => 2,
            Self::ResultLimit => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GroupKey {
    cache: CacheState,
    outcome: OutcomeKind,
    class: LatencyClass,
    depth: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LatencyClass {
    All,
    Query(QueryClass),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SamplingPlan {
    samples: usize,
    minimum_duration: Duration,
}

impl SamplingPlan {
    fn for_profile(profile: Bm01Profile) -> Self {
        if profile == Bm01Profile::qualifying() {
            Self {
                samples: QUALIFYING_SAMPLES,
                minimum_duration: Duration::from_secs(QUALIFYING_SAMPLE_SECONDS),
            }
        } else {
            Self {
                samples: 1,
                minimum_duration: Duration::ZERO,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedOutcome {
    kind: OutcomeKind,
    visits: u64,
    logical_result_bytes: u64,
    digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedExecution {
    outcome: ValidatedOutcome,
    query_elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LatencySummary {
    cache: CacheState,
    outcome: OutcomeKind,
    class: LatencyClass,
    depth: u8,
    count: usize,
    p50_nanoseconds: u128,
    p95_nanoseconds: u128,
    p99_nanoseconds: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SampleReport {
    ordinal: usize,
    minimum_duration_milliseconds: u128,
    elapsed_milliseconds: u128,
    rounds: usize,
    timed_executions: usize,
    current_rss_kib: u64,
    process_peak_rss_kib: u64,
    output_digest: [u8; 32],
    cache_work: [CacheWorkReport; 2],
    latencies: Vec<LatencySummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CacheWorkReport {
    cache: CacheState,
    successful_visits: u64,
    successful_logical_result_bytes: u64,
    index: GraphIndexCacheReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CacheWorkAccumulator {
    cache: CacheState,
    successful_visits: u64,
    successful_logical_result_bytes: u64,
    index: Option<GraphIndexCacheReport>,
}

impl CacheWorkAccumulator {
    const fn new(cache: CacheState) -> Self {
        Self {
            cache,
            successful_visits: 0,
            successful_logical_result_bytes: 0,
            index: None,
        }
    }

    fn add(
        &mut self,
        outcome: ValidatedOutcome,
        delta: GraphIndexCacheReport,
    ) -> Result<(), LinuxRunnerError> {
        self.successful_visits = self
            .successful_visits
            .checked_add(outcome.visits)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
        self.successful_logical_result_bytes = self
            .successful_logical_result_bytes
            .checked_add(outcome.logical_result_bytes)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
        self.index = Some(match self.index {
            None => delta,
            Some(current) => add_cache_work(current, delta)?,
        });
        Ok(())
    }

    fn finish(self) -> Result<CacheWorkReport, LinuxRunnerError> {
        Ok(CacheWorkReport {
            cache: self.cache,
            successful_visits: self.successful_visits,
            successful_logical_result_bytes: self.successful_logical_result_bytes,
            index: self
                .index
                .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_OBSERVATIONS"))?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxSamplingReport {
    entities: u64,
    relationships: u64,
    frontier: u64,
    setup_milliseconds: u128,
    warmup_queries: usize,
    warmup_successes: usize,
    warmup_visit_limits: usize,
    warmup_result_limits: usize,
    oracle_bundle_digest: [u8; 32],
    samples: Vec<SampleReport>,
}

impl LinuxSamplingReport {
    #[must_use]
    pub fn to_json(&self) -> String {
        let qualification = if self.entities == Bm01Profile::qualifying().entities() {
            "qualification-candidate-deadline-and-environment-unverified"
        } else {
            "nonqualifying-development-sampling"
        };
        let mut output = format!(
            concat!(
                "{{\"schema\":\"bm01-linux-sampling-v1\",",
                "\"engine_benchmark\":true,\"qualification\":\"{}\",",
                "\"filesystem_profile\":\"linux-x86_64-btrfs\",",
                "\"oracle_profile\":\"bm01-oracle-bundle-v1\",",
                "\"result_size_profile\":\"bm01-result-v1\",",
                "\"cache_pairing\":\"empty-then-retained-identical-query\",",
                "\"kernel_filesystem_device_cache\":\"uncontrolled\",",
                "\"full_memory_graph_state\":true,",
                "\"query_deadline_seconds\":30,\"query_deadline_enforced\":false,",
                "\"query_deadline_postchecked\":true,",
                "\"maximum_timed_executions_per_sample\":{},",
                "\"budget_evaluation\":\"not-performed\",",
                "\"entities\":{},\"relationships\":{},\"frontier\":{},",
                "\"setup_milliseconds\":{},",
                "\"warmup\":{{\"queries\":{},\"successes\":{},",
                "\"visit_limits\":{},\"result_limits\":{}}},",
                "\"oracle_bundle_digest\":\"{}\",\"samples\":["
            ),
            qualification,
            MAX_TIMED_EXECUTIONS_PER_SAMPLE,
            self.entities,
            self.relationships,
            self.frontier,
            self.setup_milliseconds,
            self.warmup_queries,
            self.warmup_successes,
            self.warmup_visit_limits,
            self.warmup_result_limits,
            hex(&self.oracle_bundle_digest),
        );
        for (sample_index, sample) in self.samples.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            output.push_str(&sample.to_json());
        }
        output.push_str("]}");
        output
    }
}

impl SampleReport {
    fn to_json(&self) -> String {
        let mut output = format!(
            concat!(
                "{{\"sample\":{},\"minimum_duration_milliseconds\":{},",
                "\"elapsed_milliseconds\":{},\"rounds\":{},\"timed_executions\":{},",
                "\"current_rss_kib\":{},\"process_peak_rss_kib\":{},",
                "\"output_digest\":\"{}\",\"cache_work\":["
            ),
            self.ordinal,
            self.minimum_duration_milliseconds,
            self.elapsed_milliseconds,
            self.rounds,
            self.timed_executions,
            self.current_rss_kib,
            self.process_peak_rss_kib,
            hex(&self.output_digest),
        );
        for (index, work) in self.cache_work.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            output.push_str(&work.to_json());
        }
        output.push_str("],\"latency_groups\":[");
        for (index, latency) in self.latencies.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            output.push_str(&format!(
                concat!(
                    "{{\"cache\":\"{}\",\"outcome\":\"{}\",",
                    "\"class\":\"{}\",\"depth\":{},\"count\":{},",
                    "\"p50_nanoseconds\":{},\"p95_nanoseconds\":{},",
                    "\"p99_nanoseconds\":{}}}"
                ),
                latency.cache.name(),
                latency.outcome.name(),
                latency_class_name(latency.class),
                latency.depth,
                latency.count,
                latency.p50_nanoseconds,
                latency.p95_nanoseconds,
                latency.p99_nanoseconds,
            ));
        }
        output.push_str("]}");
        output
    }
}

impl CacheWorkReport {
    fn to_json(self) -> String {
        format!(
            concat!(
                "{{\"cache\":\"{}\",\"successful_visits\":{},",
                "\"successful_logical_result_bytes\":{},",
                "\"index_cache_budget_bytes\":{},\"index_cache_accounted_bytes\":{},",
                "\"index_cache_hits\":{},\"index_cache_misses\":{},",
                "\"index_cache_evictions\":{},\"authorized_reads\":{},",
                "\"index_operations\":{},\"index_pages_read\":{},",
                "\"index_fragments_visited\":{},\"authenticated_index_result_bytes\":{}}}"
            ),
            self.cache.name(),
            self.successful_visits,
            self.successful_logical_result_bytes,
            self.index.budget_bytes,
            self.index.accounted_bytes,
            self.index.hits,
            self.index.misses,
            self.index.evictions,
            self.index.completed_authorized_reads,
            self.index.completed_index_operations,
            self.index.pages_read,
            self.index.fragments_visited,
            self.index.result_bytes,
        )
    }
}

pub fn sample(
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<LinuxSamplingReport, LinuxRunnerError> {
    let bundle = read_oracle_bundle(bundle_file)?;
    validate_bundle(&bundle, profile)?;
    let mut opened = open_completed(root, password_file, profile)?;
    let frontier = opened
        .recovery
        .frontier
        .expect("validated open has a frontier")
        .get();
    let materializer = Materializer::new(profile);
    let (warmup_successes, warmup_visit_limits, warmup_result_limits) =
        run_warmup(&mut opened, materializer, bundle.warmup().expectations())?;
    let plan = SamplingPlan::for_profile(profile);
    let mut samples = Vec::with_capacity(plan.samples);
    for ordinal in 1..=plan.samples {
        samples.push(run_sample(
            &mut opened,
            materializer,
            bundle.measured().expectations(),
            plan,
            ordinal,
        )?);
    }
    Ok(LinuxSamplingReport {
        entities: profile.entities(),
        relationships: profile.relationships(),
        frontier,
        setup_milliseconds: opened.setup_elapsed.as_millis(),
        warmup_queries: bundle.warmup().expectations().len(),
        warmup_successes,
        warmup_visit_limits,
        warmup_result_limits,
        oracle_bundle_digest: bundle.digest(),
        samples,
    })
}

fn run_warmup(
    opened: &mut Opened,
    materializer: Materializer,
    expectations: &[OracleExpectation],
) -> Result<(usize, usize, usize), LinuxRunnerError> {
    let mut successes = 0;
    let mut visit_limits = 0;
    let mut result_limits = 0;
    for expected in expectations {
        clear_cache(opened)?;
        let execution = execute_expected(opened, materializer, expected)?;
        if execution.query_elapsed > QUERY_DEADLINE {
            return Err(LinuxRunnerError::new("USTE_BM01_QUERY_DEADLINE"));
        }
        match execution.outcome.kind {
            OutcomeKind::Success => successes += 1,
            OutcomeKind::VisitLimit => visit_limits += 1,
            OutcomeKind::ResultLimit => result_limits += 1,
        }
    }
    Ok((successes, visit_limits, result_limits))
}

fn run_sample(
    opened: &mut Opened,
    materializer: Materializer,
    expectations: &[OracleExpectation],
    plan: SamplingPlan,
    ordinal: usize,
) -> Result<SampleReport, LinuxRunnerError> {
    let started = Instant::now();
    let mut rounds = 0_usize;
    let mut groups = BTreeMap::<GroupKey, Vec<u128>>::new();
    let mut timed_executions = 0_usize;
    let mut cache_work = [
        CacheWorkAccumulator::new(CacheState::Empty),
        CacheWorkAccumulator::new(CacheState::Retained),
    ];
    let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-sampling-v1");
    aggregate.update(&(ordinal as u64).to_be_bytes());
    loop {
        for expected in expectations {
            clear_cache(opened)?;
            for cache in [CacheState::Empty, CacheState::Retained] {
                if timed_executions == MAX_TIMED_EXECUTIONS_PER_SAMPLE {
                    return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_OBSERVATIONS"));
                }
                let before = index_report(opened)?;
                let execution = execute_expected(opened, materializer, expected)?;
                if execution.query_elapsed > QUERY_DEADLINE {
                    return Err(LinuxRunnerError::new("USTE_BM01_QUERY_DEADLINE"));
                }
                let delta = cache_delta(before, index_report(opened)?)?;
                timed_executions += 1;
                cache_work[cache.index()].add(execution.outcome, delta)?;
                aggregate.update(&[
                    cache.code(),
                    execution.outcome.kind.code(),
                    expected.query.class.code(),
                    expected.query.direction.code(),
                    expected.query.depth,
                ]);
                aggregate.update(&expected.query.ordinal.to_be_bytes());
                aggregate.update(&execution.outcome.digest);
                for class in [LatencyClass::All, LatencyClass::Query(expected.query.class)] {
                    record_latency(
                        &mut groups,
                        GroupKey {
                            cache,
                            outcome: execution.outcome.kind,
                            class,
                            depth: expected.query.depth,
                        },
                        execution.query_elapsed.as_nanos(),
                    )?;
                }
            }
        }
        rounds = rounds
            .checked_add(1)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_OBSERVATIONS"))?;
        if started.elapsed() >= plan.minimum_duration {
            break;
        }
    }
    let elapsed = started.elapsed();
    let (current_rss_kib, peak_rss_kib) = process_rss()?;
    Ok(SampleReport {
        ordinal,
        minimum_duration_milliseconds: plan.minimum_duration.as_millis(),
        elapsed_milliseconds: elapsed.as_millis(),
        rounds,
        timed_executions,
        current_rss_kib,
        process_peak_rss_kib: peak_rss_kib,
        output_digest: *aggregate.finalize().as_bytes(),
        cache_work: [cache_work[0].finish()?, cache_work[1].finish()?],
        latencies: summarize(groups),
    })
}

fn execute_expected(
    opened: &mut Opened,
    materializer: Materializer,
    expected: &OracleExpectation,
) -> Result<ValidatedExecution, LinuxRunnerError> {
    let started = Instant::now();
    let actual = execute_query(
        &opened.coordinator,
        &mut opened.filesystem,
        &opened.principal,
        &opened.view,
        &opened.root,
        materializer,
        expected.query,
    );
    let query_elapsed = started.elapsed();
    let outcome = match (expected.outcome, actual) {
        (
            OracleExpectedOutcome::Output {
                visits,
                relationships,
                entities,
                logical_result_bytes: expected_bytes,
                output_digest,
            },
            Ok(actual),
        ) => {
            let actual_visits = u64::try_from(actual.visits)
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
            let actual_relationships = u64::try_from(actual.relationships.len())
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
            let actual_entities = u64::try_from(actual.reachable_entities.len())
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
            let actual_bytes = logical_result_bytes(&actual)
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
            let actual_digest = actual.digest();
            if actual_visits != visits
                || actual_relationships != relationships
                || actual_entities != entities
                || actual_bytes != expected_bytes
                || actual_digest != output_digest
            {
                return Err(LinuxRunnerError::new("USTE_BM01_QUERY_MISMATCH"));
            }
            ValidatedOutcome {
                kind: OutcomeKind::Success,
                visits: actual_visits,
                logical_result_bytes: actual_bytes,
                digest: actual_digest,
            }
        }
        (OracleExpectedOutcome::VisitLimit, Err(EngineQueryError::VisitLimit)) => {
            ValidatedOutcome {
                kind: OutcomeKind::VisitLimit,
                visits: 0,
                logical_result_bytes: 0,
                digest: [0; 32],
            }
        }
        (OracleExpectedOutcome::ResultLimit, Err(EngineQueryError::ResultLimit)) => {
            ValidatedOutcome {
                kind: OutcomeKind::ResultLimit,
                visits: 0,
                logical_result_bytes: 0,
                digest: [0; 32],
            }
        }
        _ => return Err(LinuxRunnerError::new("USTE_BM01_QUERY_MISMATCH")),
    };
    Ok(ValidatedExecution {
        outcome,
        query_elapsed,
    })
}

fn clear_cache(opened: &Opened) -> Result<(), LinuxRunnerError> {
    opened
        .coordinator
        .clear_index_cache(&opened.principal, &opened.root)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_CLEAR"))
}

fn index_report(opened: &Opened) -> Result<GraphIndexCacheReport, LinuxRunnerError> {
    opened
        .coordinator
        .index_report(&opened.principal, &opened.root)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_REPORT"))
}

fn add_cache_work(
    current: GraphIndexCacheReport,
    delta: GraphIndexCacheReport,
) -> Result<GraphIndexCacheReport, LinuxRunnerError> {
    if current.budget_bytes != delta.budget_bytes {
        return Err(LinuxRunnerError::new("USTE_BM01_INDEX_COUNTER"));
    }
    let add = |left: u64, right: u64| {
        left.checked_add(right)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_INDEX_COUNTER"))
    };
    Ok(GraphIndexCacheReport {
        budget_bytes: delta.budget_bytes,
        accounted_bytes: delta.accounted_bytes,
        hits: add(current.hits, delta.hits)?,
        misses: add(current.misses, delta.misses)?,
        evictions: add(current.evictions, delta.evictions)?,
        completed_authorized_reads: add(
            current.completed_authorized_reads,
            delta.completed_authorized_reads,
        )?,
        completed_index_operations: add(
            current.completed_index_operations,
            delta.completed_index_operations,
        )?,
        pages_read: add(current.pages_read, delta.pages_read)?,
        fragments_visited: add(current.fragments_visited, delta.fragments_visited)?,
        result_bytes: add(current.result_bytes, delta.result_bytes)?,
    })
}

fn record_latency(
    groups: &mut BTreeMap<GroupKey, Vec<u128>>,
    key: GroupKey,
    latency: u128,
) -> Result<(), LinuxRunnerError> {
    let values = groups.entry(key).or_default();
    values
        .try_reserve(1)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_OBSERVATIONS"))?;
    values.push(latency);
    Ok(())
}

fn summarize(groups: BTreeMap<GroupKey, Vec<u128>>) -> Vec<LatencySummary> {
    groups
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_unstable();
            LatencySummary {
                cache: key.cache,
                outcome: key.outcome,
                class: key.class,
                depth: key.depth,
                count: values.len(),
                p50_nanoseconds: percentile(&values, 50),
                p95_nanoseconds: percentile(&values, 95),
                p99_nanoseconds: percentile(&values, 99),
            }
        })
        .collect()
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = sorted
        .len()
        .checked_mul(percentile)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .unwrap_or(sorted.len());
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn read_oracle_bundle(path: &Path) -> Result<OracleBundle, LinuxRunnerError> {
    let mut file = File::open(path).map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_OPEN"))?;
    let mut input = String::new();
    file.by_ref()
        .take(
            u64::try_from(crate::MAX_ORACLE_BUNDLE_BYTES + 1)
                .expect("oracle bundle bound fits u64"),
        )
        .read_to_string(&mut input)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_READ"))?;
    if input.len() > crate::MAX_ORACLE_BUNDLE_BYTES {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_SIZE"));
    }
    OracleBundle::parse(&input).map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_INVALID"))
}

fn validate_bundle(bundle: &OracleBundle, profile: Bm01Profile) -> Result<(), LinuxRunnerError> {
    if bundle.profile() != profile {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_PROFILE"));
    }
    if profile == Bm01Profile::qualifying() && bundle.digest() != QUALIFYING_ORACLE_BUNDLE_DIGEST {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_ACCEPTANCE"));
    }
    Ok(())
}

const fn latency_class_name(class: LatencyClass) -> &'static str {
    match class {
        LatencyClass::All => "all",
        LatencyClass::Query(QueryClass::Uniform) => "uniform",
        LatencyClass::Query(QueryClass::Hub) => "hub",
        LatencyClass::Query(QueryClass::Cycle) => "cycle",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        CacheState, CacheWorkAccumulator, CacheWorkReport, GroupKey, LatencyClass, LatencySummary,
        LinuxSamplingReport, OutcomeKind, SampleReport, SamplingPlan, ValidatedOutcome,
        latency_class_name, summarize,
    };
    use crate::{Bm01Profile, QueryClass};
    use uste_graph::GraphIndexCacheReport;

    #[test]
    fn qualifying_plan_cannot_be_lowered_and_development_runs_one_round() {
        assert_eq!(
            SamplingPlan::for_profile(Bm01Profile::qualifying()),
            SamplingPlan {
                samples: 5,
                minimum_duration: Duration::from_secs(60),
            }
        );
        assert_eq!(
            SamplingPlan::for_profile(Bm01Profile::new(20).unwrap()),
            SamplingPlan {
                samples: 1,
                minimum_duration: Duration::ZERO,
            }
        );
    }

    #[test]
    fn latency_groups_keep_cache_outcome_class_and_depth_separate() {
        let key = GroupKey {
            cache: CacheState::Retained,
            outcome: OutcomeKind::Success,
            class: LatencyClass::Query(QueryClass::Hub),
            depth: 4,
        };
        let groups = summarize([(key, vec![40, 10, 30, 20])].into());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 4);
        assert_eq!(groups[0].p50_nanoseconds, 20);
        assert_eq!(groups[0].p95_nanoseconds, 40);
        assert_eq!(groups[0].p99_nanoseconds, 40);
        assert_eq!(latency_class_name(groups[0].class), "hub");
    }

    #[test]
    fn sampling_report_discloses_deadline_and_keeps_outcomes_named() {
        let cache_report = GraphIndexCacheReport {
            budget_bytes: 1,
            accounted_bytes: 2,
            hits: 3,
            misses: 4,
            evictions: 5,
            completed_authorized_reads: 6,
            completed_index_operations: 7,
            pages_read: 8,
            fragments_visited: 9,
            result_bytes: 10,
        };
        let report = LinuxSamplingReport {
            entities: 20,
            relationships: 200,
            frontier: 4,
            setup_milliseconds: 11,
            warmup_queries: 96,
            warmup_successes: 96,
            warmup_visit_limits: 0,
            warmup_result_limits: 0,
            oracle_bundle_digest: [1; 32],
            samples: vec![SampleReport {
                ordinal: 1,
                minimum_duration_milliseconds: 0,
                elapsed_milliseconds: 12,
                rounds: 1,
                timed_executions: 768,
                current_rss_kib: 15,
                process_peak_rss_kib: 16,
                output_digest: [2; 32],
                cache_work: [
                    CacheWorkReport {
                        cache: CacheState::Empty,
                        successful_visits: 13,
                        successful_logical_result_bytes: 14,
                        index: cache_report,
                    },
                    CacheWorkReport {
                        cache: CacheState::Retained,
                        successful_visits: 21,
                        successful_logical_result_bytes: 22,
                        index: cache_report,
                    },
                ],
                latencies: vec![LatencySummary {
                    cache: CacheState::Retained,
                    outcome: OutcomeKind::ResultLimit,
                    class: LatencyClass::All,
                    depth: 4,
                    count: 17,
                    p50_nanoseconds: 18,
                    p95_nanoseconds: 19,
                    p99_nanoseconds: 20,
                }],
            }],
        }
        .to_json();
        assert!(report.contains("\"query_deadline_enforced\":false"));
        assert!(report.contains("\"query_deadline_postchecked\":true"));
        assert!(report.contains("\"class\":\"all\""));
        assert!(report.contains("\"outcome\":\"result-limit\""));
        assert!(report.contains("\"successful_visits\":13"));
        assert!(report.contains("\"process_peak_rss_kib\":16"));
        assert!(!report.contains('/'));
    }

    #[test]
    fn cache_work_accumulates_each_state_with_latest_residency() {
        let mut work = CacheWorkAccumulator::new(CacheState::Empty);
        let outcome = ValidatedOutcome {
            kind: OutcomeKind::Success,
            visits: 2,
            logical_result_bytes: 3,
            digest: [0; 32],
        };
        let first = GraphIndexCacheReport {
            budget_bytes: 100,
            accounted_bytes: 10,
            hits: 1,
            misses: 2,
            evictions: 3,
            completed_authorized_reads: 4,
            completed_index_operations: 5,
            pages_read: 6,
            fragments_visited: 7,
            result_bytes: 8,
        };
        let second = GraphIndexCacheReport {
            budget_bytes: 100,
            accounted_bytes: 20,
            hits: 10,
            misses: 20,
            evictions: 30,
            completed_authorized_reads: 40,
            completed_index_operations: 50,
            pages_read: 60,
            fragments_visited: 70,
            result_bytes: 80,
        };
        work.add(outcome, first).unwrap();
        work.add(outcome, second).unwrap();
        let report = work.finish().unwrap();
        assert_eq!(report.successful_visits, 4);
        assert_eq!(report.successful_logical_result_bytes, 6);
        assert_eq!(report.index.accounted_bytes, 20);
        assert_eq!(report.index.hits, 11);
        assert_eq!(report.index.result_bytes, 88);
    }
}
