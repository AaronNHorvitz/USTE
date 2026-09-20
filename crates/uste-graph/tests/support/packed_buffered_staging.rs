use super::*;
use uste_storage::{MAX_INDEX_CACHE_BYTES, MIN_INDEX_CACHE_BYTES};

#[test]
fn packed_graph_buffered_staging_reference_proof_budgets_and_fresh_residency_are_exact() {
    let mut input = input(&complex_request());
    let (expected, work) = stage_packed_graph_delta(
        &mut input.recovery,
        &mut input.fs,
        &input.base,
        &input.target,
        &input.plan,
        stage_limits(2),
    )
    .unwrap();
    assert_eq!(work.buffered_batches, 0);
    for budget in [MIN_INDEX_CACHE_BYTES, 1024 * 1024] {
        let exact = PackedGraphStageLimits {
            staging_cache_bytes: Some(budget),
            maximum_batches: work.batches,
            maximum_read_pages: work.read_pages,
            maximum_written_pages: work.written_pages,
            ..stage_limits(2)
        };
        let mut previous_report = None;
        for _ in 0..2 {
            let (actual, report) = stage_packed_graph_delta(
                &mut input.recovery,
                &mut input.fs,
                &input.base,
                &input.target,
                &input.plan,
                exact,
            )
            .unwrap();
            assert_eq!(
                actual.publication_claims().state_digest,
                expected.publication_claims().state_digest
            );
            assert_eq!(
                actual.families().map(|f| f.commitment),
                expected.families().map(|f| f.commitment)
            );
            assert_eq!(
                (
                    report.batches,
                    report.read_pages,
                    report.written_pages,
                    report.peak_batch_deltas
                ),
                (
                    work.batches,
                    work.read_pages,
                    work.written_pages,
                    work.peak_batch_deltas
                )
            );
            assert_eq!(report.buffered_batches, report.batches);
            assert_eq!(report.cache_hits + report.cache_misses, report.read_pages);
            assert!(report.cache_hits > 0 && report.cache_misses > 0);
            assert!(report.peak_cache_accounted_bytes <= budget);
            if let Some(previous) = previous_report {
                assert_eq!(report, previous);
            }
            previous_report = Some(report);
            assert_eq!(
                packed_graph_v1_digest(
                    &mut input.recovery,
                    &mut input.fs,
                    &actual,
                    &input.target,
                    limits(2).certificates,
                    export_limits(),
                    4 * 1024 * 1024
                )
                .unwrap(),
                input.new_digest
            );
        }
        for narrow in [
            PackedGraphStageLimits {
                maximum_read_pages: work.read_pages - 1,
                ..exact
            },
            PackedGraphStageLimits {
                maximum_written_pages: work.written_pages - 1,
                ..exact
            },
            PackedGraphStageLimits {
                maximum_batches: work.batches - 1,
                ..exact
            },
        ] {
            assert!(
                stage_packed_graph_delta(
                    &mut input.recovery,
                    &mut input.fs,
                    &input.base,
                    &input.target,
                    &input.plan,
                    narrow
                )
                .is_err()
            );
        }
    }
    for bytes in [0, MIN_INDEX_CACHE_BYTES - 1, MAX_INDEX_CACHE_BYTES + 1] {
        input.fs.arm(FaultPlan::default()).unwrap();
        assert!(
            stage_packed_graph_delta(
                &mut input.recovery,
                &mut input.fs,
                &input.base,
                &input.target,
                &input.plan,
                PackedGraphStageLimits {
                    staging_cache_bytes: Some(bytes),
                    ..stage_limits(2)
                }
            )
            .is_err()
        );
        assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
        assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
    }
    assert_eq!(
        packed_graph_v1_digest(
            &mut input.recovery,
            &mut input.fs,
            &input.base,
            &input.old,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .unwrap(),
        input.old_digest
    );
}
