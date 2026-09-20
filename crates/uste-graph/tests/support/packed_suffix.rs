use super::*;
#[path = "packed_authorized_writes.rs"]
mod authorized_write;
#[path = "packed_origin.rs"]
mod origin;
use uste_graph::{
    PackedGraphAdmissionLimits, PackedGraphSuffixRecoveryLimits, PackedGraphSuffixRecoveryReport,
    admit_packed_graph_base, recover_packed_graph_suffix,
};
use uste_storage::{
    journal::PackedRootDiscoveryLimits, packed_tree_validation::TreeValidationLimits,
};
use uste_txn::{
    PackedCoordinatorAdmissionLimits, PackedQuotaAdmissionLimits, admit_packed_coordinator_prefix,
    admit_packed_quota_prefix,
};

struct Input {
    fs: Fs,
    recovery: Recovery,
    base: PackedGraphBase,
    graph_root: CertifiedPackedRoot,
    primary: PackedCoordinatorPrefix,
    quota: PackedQuotaPrefix,
    primary_root: CertifiedPackedRoot,
    quota_root: CertifiedPackedRoot,
}
fn family_limits() -> TreeValidationLimits {
    TreeValidationLimits {
        maximum_path_branches: 512,
        maximum_nodes: 10000,
        maximum_logical_bytes: 16 * 1024 * 1024,
        maximum_pages: 100000,
        maximum_encoded_bytes: 100000 * 20545,
    }
}
fn graph_admission_limits() -> PackedGraphAdmissionLimits {
    PackedGraphAdmissionLimits {
        canonical: family_limits(),
        semantic: admission_limits(),
        scan: export_limits(),
        lookup: preparation_limits().lookup,
        maximum_lookup_encoded_bytes: 100000 * 20545,
    }
}
fn suffix_limits() -> PackedGraphSuffixRecoveryLimits {
    PackedGraphSuffixRecoveryLimits {
        maximum_revisions: 2,
        preparation: preparation_limits(),
        deltas: GraphStateDeltaLimits::new(1000, 16 * 1024 * 1024).unwrap(),
        graph: stage_limits(2),
        metadata: rebase_limits(),
    }
}
fn root(recovery: &Recovery, fs: &mut Fs, profile: [u8; 32], revision: u64) -> CertifiedPackedRoot {
    let (roots, _) = recovery
        .discover_packed_roots_at_revision(
            fs,
            profile,
            CommitRevision::new(revision).unwrap(),
            limits(2).certificates,
            PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap(),
        )
        .unwrap();
    roots
        .into_iter()
        .max_by_key(|root| root.manifest().claims().generation)
        .unwrap()
}
fn reopen(mut fs: Fs, revision: u64, entropy: u64) -> Input {
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let name = EntryName::new("packed-graph-bridge").unwrap();
    let (mut recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        &mut fs,
        &name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 10_000),
        &mut TestKeyAdapter,
        limits(2).certificates,
    )
    .unwrap();
    let graph_root = root(&recovery, &mut fs, GRAPH_PACKED_PROFILE_V1, revision);
    let primary_root = root(&recovery, &mut fs, COORDINATOR_PACKED_PROFILE_V1, revision);
    let quota_root = root(
        &recovery,
        &mut fs,
        COORDINATOR_PACKED_USAGE_PROFILE_V1,
        revision,
    );
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::new(revision).unwrap(),
            CommitRevision::new(revision).unwrap(),
            1,
            1024 * 1024,
        )
        .unwrap();
    let transaction = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    recovery.finish_transaction_cursor(cursor).unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
        .unwrap();
    let base =
        admit_packed_graph_base(&maintenance, &mut fs, &graph_root, graph_admission_limits())
            .unwrap()
            .0;
    let primary = admit_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        &primary_root,
        PackedCoordinatorAdmissionLimits {
            certificates: limits(2).certificates,
            family: family_limits(),
            lookup: preparation_limits().lookup,
            maximum_groups: revision,
            maximum_journal_bytes: revision * 1024 * 1024,
            maximum_references: 4,
            maximum_owners: 0,
            maximum_lookup_pages: 100000,
            maximum_lookup_bytes: 100000 * 20545,
        },
    )
    .unwrap()
    .0;
    let quota = admit_packed_quota_prefix(
        &mut recovery,
        &mut fs,
        &primary,
        &quota_root,
        PackedQuotaAdmissionLimits {
            certificates: limits(2).certificates,
            family: family_limits(),
            cursor: export_limits(),
            lookup: preparation_limits().lookup,
            maximum_owners: 0,
            maximum_lookup_pages: 100000,
            maximum_lookup_bytes: 100000 * 20545,
        },
    )
    .unwrap()
    .0;
    Input {
        fs,
        recovery,
        base,
        graph_root,
        primary,
        quota,
        primary_root,
        quota_root,
    }
}
fn fixture_suffix(count: u8) -> (Input, Vec<(GraphTransaction, TransactionOutcome)>, [u8; 32]) {
    let parts = parts();
    let mut fs = parts.fs;
    drop(parts.recovery);
    let name = EntryName::new("packed-graph-bridge").unwrap();
    fs.restart().unwrap();
    let (mut model, _) = CommitCoordinator::open(
        &mut fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_290_000),
        CounterEntropy(1_300_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let mut expected = Vec::new();
    for (offset, tx) in [cases()[0].clone(), cases()[20].clone()]
        .into_iter()
        .take(count.into())
        .enumerate()
    {
        let outcome = commit(&mut model, &mut fs, offset as u8 + 4, tx.clone());
        expected.push((tx, outcome));
    }
    let digest = GraphState::logical_state_digest(model.read_view().unwrap().state()).unwrap();
    drop(model);
    (reopen(fs, 3, 1_310_000), expected, digest)
}
fn recover(
    input: Input,
    limits: PackedGraphSuffixRecoveryLimits,
) -> (
    Fs,
    Result<(Live, Option<PackedGraphSuffixRecoveryReport>), GraphDiskError>,
) {
    let mut fs = input.fs;
    let result = recover_packed_graph_suffix(
        input.recovery,
        &mut fs,
        input.base,
        input.graph_root,
        input.primary,
        input.quota,
        &input.primary_root,
        &input.quota_root,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(2, 0).unwrap(),
        limits,
    );
    (fs, result)
}

#[test]
fn packed_graph_suffix_recovery_cold_pairing_reference_and_continued_live_writes() {
    buffered_suffix_reference(None);
}
#[test]
fn packed_graph_buffered_staging_suffix_recovers_cold_pair_and_continues_writes() {
    buffered_suffix_reference(Some(uste_storage::MIN_INDEX_CACHE_BYTES));
}
fn buffered_suffix_reference(cache: Option<usize>) {
    let mut selected_limits = suffix_limits();
    selected_limits.graph.staging_cache_bytes = cache;
    selected_limits.metadata.staging.staging_cache_bytes = cache;
    for count in 0..=2 {
        let (mut input, expected, digest) = fixture_suffix(count);
        input.fs.arm(FaultPlan::default()).unwrap();
        let (mut fs, recovered) = recover(input, selected_limits);
        let (mut live, report) = recovered.unwrap();
        if count == 0 {
            assert!(report.is_none());
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
            assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
        } else {
            let report = report.unwrap();
            assert_eq!(report.journal.groups, u64::from(count));
            assert!(report.proof.pages > 0);
            assert!(report.graph.written_pages > 0);
            assert_eq!(
                report.graph.buffered_batches,
                if cache.is_some() {
                    report.graph.batches
                } else {
                    0
                }
            );
            if cache.is_some() {
                assert_eq!(
                    report.graph.cache_hits + report.graph.cache_misses,
                    report.graph.read_pages
                );
            }
            assert!(report.metadata_written_pages > 0);
        }
        assert_eq!(live.overlay_counts(), (0, 0));
        assert_eq!(live.state().unwrap().revision().get(), 3 + u64::from(count));
        for (tx, outcome) in &expected {
            assert_eq!(
                retry(&mut fs, &mut live, outcome.revision.get() as u8, tx),
                *outcome
            );
        }
        drop(live);
        let input = reopen(fs, 3 + u64::from(count), 1_330_000);
        assert_eq!(input.base.source_v1_digest(), Some(&digest));
        let (mut fs, recovered) = recover(input, selected_limits);
        let (mut live, report) = recovered.unwrap();
        assert!(report.is_none());
        let tx = GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(20),
                    entity_type: text("after-suffix"),
                    schema_version: 1,
                    properties: Value::RecordRef(record(1)),
                }),
            }],
        );
        let outcome = append(&mut fs, &mut live, 4 + count, &tx);
        publish_packed_graph_live_base(&mut live, &mut fs, outcome, selected_limits.graph, 8)
            .unwrap();
        live.rebase_metadata(&mut fs, selected_limits.metadata)
            .unwrap();
        assert_eq!(retry(&mut fs, &mut live, 4 + count, &tx), outcome);
    }
}

fn no_intermediate_roots(input: &mut Input) {
    for profile in [
        GRAPH_PACKED_PROFILE_V1,
        COORDINATOR_PACKED_PROFILE_V1,
        COORDINATOR_PACKED_USAGE_PROFILE_V1,
    ] {
        assert!(
            input
                .recovery
                .discover_packed_roots_at_revision(
                    &mut input.fs,
                    profile,
                    CommitRevision::new(4).unwrap(),
                    limits(2).certificates,
                    PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap()
                )
                .unwrap()
                .0
                .is_empty()
        );
    }
}

#[test]
fn packed_graph_suffix_recovery_limits_and_foreign_owner_keep_old_triple() {
    let (input, _, _) = fixture_suffix(2);
    let (fs, result) = recover(input, suffix_limits());
    let (live, report) = result.unwrap();
    let encoded = report.unwrap().journal.encoded_bytes;
    drop(live);
    drop(fs);
    let mut exact = suffix_limits();
    exact.metadata.maximum_encoded_bytes = encoded;
    let (input, _, _) = fixture_suffix(2);
    let (_, result) = recover(input, exact);
    assert!(result.is_ok());
    drop(result);
    for variant in 0..7 {
        let (mut input, _, digest) = fixture_suffix(2);
        let mut narrow = exact;
        match variant {
            0 => narrow.maximum_revisions = 1,
            1 => narrow.metadata.maximum_groups = 1,
            2 => narrow.metadata.maximum_encoded_bytes -= 1,
            3 => narrow.graph.maximum_batches = 0,
            4 => narrow.deltas = GraphStateDeltaLimits::new(1, 16 * 1024 * 1024).unwrap(),
            5 => {
                narrow.preparation.proof =
                    GraphDiskPreparationLimits::new(100, 1000, 100, 100, 1).unwrap()
            }
            6 => narrow.metadata.maximum_publication_attempts = 0,
            _ => unreachable!(),
        }
        input.fs.arm(FaultPlan::default()).unwrap();
        let (fs, result) = recover(input, narrow);
        assert!(result.is_err(), "limit {variant}");
        if matches!(variant, 0 | 1 | 6) {
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
            assert_eq!(fs.operation_count(FsOp::CreateNew), 0);
        }
        let mut input = reopen(fs, 3, 1_350_000);
        no_intermediate_roots(&mut input);
        let (fs, result) = recover(input, suffix_limits());
        drop(result.unwrap());
        let input = reopen(fs, 5, 1_370_000);
        assert_eq!(input.base.source_v1_digest(), Some(&digest));
    }
    let (mut input, _, _) = fixture_suffix(2);
    let (mut foreign, _, _) = fixture_suffix(2);
    std::mem::swap(&mut input.base, &mut foreign.base);
    input.fs.arm(FaultPlan::default()).unwrap();
    let (fs, result) = recover(input, suffix_limits());
    assert!(result.is_err());
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::CreateNew), 0);
}

#[test]
fn packed_graph_suffix_recovery_every_observed_fault_restarts_without_intermediate_roots() {
    let (mut input, _, _) = fixture_suffix(2);
    input.fs.arm(FaultPlan::default()).unwrap();
    let (observed, result) = recover(input, suffix_limits());
    drop(result.unwrap());
    let counts = [
        FsOp::OpenExisting,
        FsOp::Metadata,
        FsOp::ReadAt,
        FsOp::CreateNew,
        FsOp::WriteAt,
        FsOp::SetLen,
        FsOp::SyncAll,
        FsOp::SyncData,
        FsOp::SyncDirectory,
    ]
    .map(|op| (op, observed.operation_count(op)));
    let mut checked = 0;
    for (operation, count) in counts {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut input, expected, digest) = fixture_suffix(2);
                input
                    .fs
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                let (fs, result) = recover(input, suffix_limits());
                assert!(result.is_err(), "{operation:?} {occurrence} {action:?}");
                assert_eq!(fs.pending_faults(), 0);
                let mut input = reopen(fs, 3, 1_350_000);
                no_intermediate_roots(&mut input);
                let (mut fs, result) = recover(input, suffix_limits());
                let (mut live, report) = result.unwrap();
                assert_eq!(report.unwrap().journal.groups, 2);
                assert_eq!(live.overlay_counts(), (0, 0));
                for (tx, outcome) in expected {
                    assert_eq!(
                        retry(&mut fs, &mut live, outcome.revision.get() as u8, &tx),
                        outcome
                    );
                }
                drop(live);
                let input = reopen(fs, 5, 1_370_000);
                assert_eq!(input.base.source_v1_digest(), Some(&digest));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 609);
}

#[test]
fn packed_graph_suffix_recovery_late_ciphertext_corruption_never_installs_a_triple() {
    use uste_storage::FileSystem;
    for variant in 0..4 {
        let (mut input, _, digest) = fixture_suffix(2);
        let location = match variant {
            0 => None,
            1 => Some((
                input.base.families()[7].root.unwrap(),
                GRAPH_PACKED_PROFILE_V1,
                8,
            )),
            2 => Some((
                input.primary.families()[0].root.unwrap(),
                COORDINATOR_PACKED_PROFILE_V1,
                1,
            )),
            3 => Some((
                input.quota.families()[0].root.unwrap(),
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                1,
            )),
            _ => unreachable!(),
        };
        let (filename, offset) = if let Some((locator, profile, family)) = location {
            let p = locator
                .resolve(scope(), profile, family, CommitRevision::new(3).unwrap())
                .unwrap();
            (
                format!(
                    "pack-{}",
                    p.object
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                ),
                p.page * 20545 + 137,
            )
        } else {
            ("CERTIFICATES".to_owned(), 5 * 4161 + 137)
        };
        let directory = input
            .fs
            .open_directory(
                &input.fs.root(),
                &EntryName::new("packed-graph-bridge").unwrap(),
            )
            .unwrap();
        let file = input
            .fs
            .open_existing(&directory, &EntryName::new(filename).unwrap())
            .unwrap();
        let mut original = [0];
        assert_eq!(input.fs.read_at(&file, offset, &mut original).unwrap(), 1);
        assert_eq!(
            input
                .fs
                .write_at(&file, offset, &[original[0] ^ 1])
                .unwrap(),
            1
        );
        input.fs.sync_all(&file).unwrap();
        let (mut fs, result) = recover(input, suffix_limits());
        assert!(result.is_err(), "ciphertext variant {variant}");
        assert_eq!(fs.write_at(&file, offset, &original).unwrap(), 1);
        fs.sync_all(&file).unwrap();
        let mut input = reopen(fs, 3, 1_350_000);
        no_intermediate_roots(&mut input);
        let (fs, result) = recover(input, suffix_limits());
        drop(result.unwrap());
        let input = reopen(fs, 5, 1_370_000);
        assert_eq!(input.base.source_v1_digest(), Some(&digest));
    }
}

#[test]
fn packed_graph_suffix_recovery_authenticated_collision_and_false_result_refuse_terminal_roots() {
    use sha2::{Digest, Sha256};
    use uste_storage::journal::{CommitInput, JournalStore};
    for variant in 0..3 {
        let (input, _, _) = fixture_suffix(1);
        let mut fs = input.fs;
        drop(input.recovery);
        let name = EntryName::new("packed-graph-bridge").unwrap();
        let (mut model, _) = CommitCoordinator::open(
            &mut fs,
            &name,
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(1_390_000),
            CounterEntropy(1_400_000),
            &mut TestKeyAdapter,
            GraphState::new(scope()),
        )
        .unwrap();
        let bytes = encode_transaction(&cases()[20]).unwrap();
        let prepared = model
            .reducer_and_index_maintenance()
            .unwrap()
            .reducer
            .prepare(&bytes, None, CommitRevision::new(5).unwrap())
            .unwrap();
        let mut result = GraphState::result_digest(&prepared);
        if variant == 2 {
            result[0] ^= 1;
        }
        drop(model);
        let (mut journal, _) = JournalStore::open(
            &mut fs,
            &name,
            scope().database(),
            CounterEntropy(1_410_000),
            CounterEntropy(1_420_000),
            &mut TestKeyAdapter,
            |_| Ok(()),
        )
        .unwrap();
        let mut group: Vec<u8> = include_str!("../../../../acceptance/r1/txn-group-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        group.truncate(192);
        group[8..24].copy_from_slice(scope().namespace().as_bytes());
        group[24..56].fill(0x90);
        group[56..72].fill(if variant == 0 { 3 } else { 5 });
        group[72..88].fill(if variant == 1 { 35 } else { 37 });
        group[88..96].copy_from_slice(&5_i64.to_be_bytes());
        group[96..100].fill(0);
        group[100..108].copy_from_slice(&(5_i64 + 30 * 86400).to_be_bytes());
        group[108..112].fill(0);
        group[112..120].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
        group[120..152].copy_from_slice(&Sha256::digest(&bytes));
        group[152..184].copy_from_slice(&result);
        group.extend_from_slice(&bytes);
        journal
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: &group,
                    logical_event_digest: Sha256::digest(&group).into(),
                },
            )
            .unwrap();
        drop(journal);
        let mut input = reopen(fs, 3, 1_430_000);
        input.fs.arm(FaultPlan::default()).unwrap();
        let (fs, result) = recover(input, suffix_limits());
        assert!(result.is_err(), "authenticated variant {variant}");
        assert!(fs.operation_count(FsOp::CreateNew) > 0);
        if variant == 2 {
            assert!(matches!(result, Err(GraphDiskError::RootStateMismatch)));
        }
        let mut input = reopen(fs, 3, 1_450_000);
        no_intermediate_roots(&mut input);
        for profile in [
            GRAPH_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
        ] {
            assert!(
                input
                    .recovery
                    .discover_packed_roots_at_revision(
                        &mut input.fs,
                        profile,
                        CommitRevision::new(5).unwrap(),
                        limits(2).certificates,
                        PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap()
                    )
                    .unwrap()
                    .0
                    .is_empty()
            );
        }
    }
}
