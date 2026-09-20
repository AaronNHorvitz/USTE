use super::*;
use uste_graph::{
    PackedGraphOriginRecoveryLimits, PackedGraphOriginRecoveryReport, recover_packed_graph_origin,
};

fn origin_limits() -> PackedGraphOriginRecoveryLimits {
    PackedGraphOriginRecoveryLimits {
        maximum_genesis_encoded_bytes: 4 * 1024 * 1024,
        genesis: genesis::genesis_limits(2),
        suffix: suffix_limits(),
    }
}
fn open_origin(fs: &mut Fs, entropy: u64) -> Recovery {
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        fs,
        &EntryName::new("packed-graph-bridge").unwrap(),
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 10000),
        &mut TestKeyAdapter,
        limits(2).certificates,
    )
    .unwrap()
    .0
}
fn origin_fixture(
    count: u8,
) -> (
    Fs,
    Recovery,
    Vec<(GraphTransaction, TransactionOutcome)>,
    [u8; 32],
) {
    let (mut fs, recovery, genesis, _) = genesis::genesis_fixture_named(2, "packed-graph-bridge");
    let mut expected = vec![(
        uste_graph::decode_transaction(genesis.transaction().canonical_request()).unwrap(),
        genesis.transaction().outcome(),
    )];
    drop(genesis);
    drop(recovery);
    let (mut model, _) = CommitCoordinator::open(
        &mut fs,
        &EntryName::new("packed-graph-bridge").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(2_210_000),
        CounterEntropy(2_220_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    for (offset, tx) in [cases()[0].clone(), cases()[20].clone()]
        .into_iter()
        .take(count.into())
        .enumerate()
    {
        let outcome = commit(&mut model, &mut fs, offset as u8 + 2, tx.clone());
        expected.push((tx, outcome));
    }
    let digest = GraphState::logical_state_digest(model.read_view().unwrap().state()).unwrap();
    drop(model);
    let recovery = open_origin(&mut fs, 2_230_000);
    (fs, recovery, expected, digest)
}
fn origin_recover(
    fs: &mut Fs,
    recovery: Recovery,
    budget: PackedGraphOriginRecoveryLimits,
) -> Result<(Live, PackedGraphOriginRecoveryReport), GraphDiskError> {
    recover_packed_graph_origin(
        recovery,
        fs,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(0, 0).unwrap(),
        budget,
    )
}
fn no_origin_roots(recovery: &Recovery, fs: &mut Fs, terminal: u64) {
    for revision in 1..terminal {
        for profile in [
            GRAPH_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
        ] {
            assert!(
                recovery
                    .discover_packed_roots_at_revision(
                        fs,
                        profile,
                        CommitRevision::new(revision).unwrap(),
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

#[test]
fn packed_graph_origin_reference_root_free_terminal_only_and_continued_writes() {
    buffered_origin_reference(None);
}

#[test]
fn packed_graph_buffered_proof_origin_reference_terminal_only_and_continued_writes() {
    buffered_origin_reference(Some(1024 * 1024));
}

fn buffered_origin_reference(cache: Option<usize>) {
    let mut selected = origin_limits();
    selected.suffix.proof_cache_bytes = cache;
    for count in 0..=2 {
        let (mut fs, recovery, expected, digest) = origin_fixture(count);
        no_origin_roots(&recovery, &mut fs, u64::from(count) + 2);
        let (mut live, report) = origin_recover(&mut fs, recovery, selected).unwrap();
        assert_eq!(report.suffix.journal.groups, u64::from(count));
        assert_eq!(
            report.suffix.buffered_preparations,
            if cache.is_some() { u64::from(count) } else { 0 }
        );
        if let Some(budget) = cache {
            assert_eq!(
                report.suffix.preparation_cache_hits + report.suffix.preparation_cache_misses,
                report.suffix.proof.pages
            );
            assert!(report.suffix.peak_preparation_cache_accounted_bytes <= budget);
        }
        assert_eq!(live.overlay_counts(), (0, 0));
        assert_eq!(live.state().unwrap().revision().get(), u64::from(count) + 1);
        for (tx, outcome) in &expected {
            assert_eq!(
                retry(&mut fs, &mut live, outcome.revision.get() as u8, tx),
                *outcome
            );
        }
        drop(live);
        let mut input = reopen(fs, u64::from(count) + 1, 2_250_000);
        assert_eq!(input.base.source_v1_digest(), Some(&digest));
        no_origin_roots(&input.recovery, &mut input.fs, u64::from(count) + 1);
        let (mut fs, result) = recover(input, suffix_limits());
        let (mut live, report) = result.unwrap();
        assert!(report.is_none());
        let tx = GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(20),
                    entity_type: text("after-origin"),
                    schema_version: 1,
                    properties: Value::RecordRef(record(1)),
                }),
            }],
        );
        let outcome = append(&mut fs, &mut live, count + 2, &tx);
        publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8).unwrap();
        live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
        assert_eq!(retry(&mut fs, &mut live, count + 2, &tx), outcome);
    }
}

#[test]
fn packed_graph_origin_limits_refuse_without_terminal_publication() {
    let (mut fs, recovery, _, _) = origin_fixture(2);
    let (live, report) = origin_recover(&mut fs, recovery, origin_limits()).unwrap();
    let encoded = report.suffix.journal.encoded_bytes;
    drop(live);
    let mut exact = origin_limits();
    exact.suffix.metadata.maximum_encoded_bytes = encoded;
    let (mut fs, recovery, _, _) = origin_fixture(2);
    drop(origin_recover(&mut fs, recovery, exact).unwrap());
    for variant in 0..8 {
        let (mut fs, recovery, _, digest) = origin_fixture(2);
        let mut narrow = exact;
        match variant {
            0 => narrow.suffix.maximum_revisions = 1,
            1 => narrow.suffix.metadata.maximum_groups = 1,
            2 => narrow.suffix.metadata.maximum_publication_attempts = 0,
            3 => narrow.maximum_genesis_encoded_bytes = 0,
            4 => narrow.genesis.maximum_entries = 0,
            5 => narrow.suffix.metadata.maximum_encoded_bytes -= 1,
            6 => narrow.suffix.graph.maximum_batches = 0,
            7 => narrow.suffix.metadata.certificate_window = 0,
            _ => unreachable!(),
        }
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            origin_recover(&mut fs, recovery, narrow).is_err(),
            "limit {variant}"
        );
        if variant < 3 {
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
            assert_eq!(fs.operation_count(FsOp::CreateNew), 0);
        }
        let recovery = open_origin(&mut fs, 2_270_000);
        no_origin_roots(&recovery, &mut fs, 4);
        drop(origin_recover(&mut fs, recovery, origin_limits()).unwrap());
        assert_eq!(
            reopen(fs, 3, 2_290_000).base.source_v1_digest(),
            Some(&digest)
        );
    }
}

#[test]
fn packed_graph_origin_every_observed_fault_restarts_without_intermediate_roots() {
    let (mut observed, recovery, _, _) = origin_fixture(2);
    observed.arm(FaultPlan::default()).unwrap();
    drop(origin_recover(&mut observed, recovery, origin_limits()).unwrap());
    let counts = [
        FsOp::CreateDirectory,
        FsOp::OpenDirectory,
        FsOp::OpenExisting,
        FsOp::Metadata,
        FsOp::ReadAt,
        FsOp::CreateNew,
        FsOp::WriteAt,
        FsOp::SetLen,
        FsOp::SyncAll,
        FsOp::SyncData,
        FsOp::SyncDirectory,
        FsOp::RenameNoReplace,
        FsOp::RemoveFile,
        FsOp::TryLockExclusive,
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
                let (mut fs, recovery, expected, digest) = origin_fixture(2);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    origin_recover(&mut fs, recovery, origin_limits()).is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                let recovery = open_origin(&mut fs, 2_310_000);
                no_origin_roots(&recovery, &mut fs, 3);
                let (mut live, report) =
                    origin_recover(&mut fs, recovery, origin_limits()).unwrap();
                assert_eq!(report.suffix.journal.groups, 2);
                assert_eq!(live.overlay_counts(), (0, 0));
                for (tx, outcome) in expected {
                    assert_eq!(
                        retry(&mut fs, &mut live, outcome.revision.get() as u8, &tx),
                        outcome
                    );
                }
                drop(live);
                assert_eq!(
                    reopen(fs, 3, 2_330_000).base.source_v1_digest(),
                    Some(&digest)
                );
                checked += 1;
            }
        }
    }
    println!("origin fault cases: {checked}");
    assert_eq!(checked, 909);
}

fn flip(fs: &mut Fs, filename: &str, offset: u64) -> (uste_storage::memory::MemoryFile, u8) {
    use uste_storage::FileSystem;
    let directory = fs
        .open_directory(&fs.root(), &EntryName::new("packed-graph-bridge").unwrap())
        .unwrap();
    let file = fs
        .open_existing(&directory, &EntryName::new(filename).unwrap())
        .unwrap();
    let mut byte = [0];
    assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    assert_eq!(fs.write_at(&file, offset, &[byte[0] ^ 1]).unwrap(), 1);
    fs.sync_all(&file).unwrap();
    (file, byte[0])
}

#[test]
fn packed_graph_origin_retained_and_corrupt_derived_roots_are_not_authority() {
    for corrupt in [false, true] {
        let (mut fs, recovery, _, digest) = origin_fixture(2);
        drop(origin_recover(&mut fs, recovery, origin_limits()).unwrap());
        let input = reopen(fs, 3, 2_350_000);
        let old_generation = input.graph_root.manifest().claims().generation;
        let mut fs = input.fs;
        if corrupt {
            let locator = input.base.families()[1]
                .root
                .unwrap()
                .resolve(
                    scope(),
                    GRAPH_PACKED_PROFILE_V1,
                    2,
                    CommitRevision::new(3).unwrap(),
                )
                .unwrap();
            let filename = format!(
                "pack-{}",
                locator
                    .object
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            flip(&mut fs, &filename, locator.page * 20545 + 137);
            // Ordinary semantic admission must refuse that root; explicit origin rebuild does not read it.
            let mut recovery = input.recovery;
            let revision = CommitRevision::new(3).unwrap();
            let mut cursor = recovery
                .open_transaction_cursor(revision, revision, 1, 1024 * 1024)
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
            assert!(
                admit_packed_graph_base(
                    &maintenance,
                    &mut fs,
                    &input.graph_root,
                    graph_admission_limits()
                )
                .is_err()
            );
            drop(recovery);
        } else {
            drop(input.recovery);
        }
        let recovery = open_origin(&mut fs, 2_370_000);
        drop(origin_recover(&mut fs, recovery, origin_limits()).unwrap());
        let mut input = reopen(fs, 3, 2_390_000);
        assert_eq!(input.base.source_v1_digest(), Some(&digest));
        assert!(input.graph_root.manifest().claims().generation > old_generation);
        no_origin_roots(&input.recovery, &mut input.fs, 3);
    }
}

#[test]
fn packed_graph_origin_late_authoritative_ciphertext_corruption_fails_closed() {
    use uste_storage::FileSystem;
    for revision in [1, 3] {
        let (mut fs, recovery, _, digest) = origin_fixture(2);
        let offset = revision * 4161 + 137;
        let (file, byte) = flip(&mut fs, "CERTIFICATES", offset);
        assert!(origin_recover(&mut fs, recovery, origin_limits()).is_err());
        assert_eq!(fs.write_at(&file, offset, &[byte]).unwrap(), 1);
        fs.sync_all(&file).unwrap();
        let recovery = open_origin(&mut fs, 2_410_000);
        no_origin_roots(&recovery, &mut fs, 4);
        drop(origin_recover(&mut fs, recovery, origin_limits()).unwrap());
        assert_eq!(
            reopen(fs, 3, 2_430_000).base.source_v1_digest(),
            Some(&digest)
        );
    }
}

#[test]
fn packed_graph_origin_authenticated_collisions_and_false_results_refuse_all_roots() {
    use sha2::{Digest, Sha256};
    use uste_storage::journal::{CommitInput, JournalStore};
    for variant in 0..3 {
        let (mut fs, recovery, _, _) = origin_fixture(1);
        drop(recovery);
        let name = EntryName::new("packed-graph-bridge").unwrap();
        let (mut model, _) = CommitCoordinator::open(
            &mut fs,
            &name,
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(2_450_000),
            CounterEntropy(2_460_000),
            &mut TestKeyAdapter,
            GraphState::new(scope()),
        )
        .unwrap();
        let bytes = encode_transaction(&cases()[20]).unwrap();
        let prepared = model
            .reducer_and_index_maintenance()
            .unwrap()
            .reducer
            .prepare(&bytes, None, CommitRevision::new(3).unwrap())
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
            CounterEntropy(2_470_000),
            CounterEntropy(2_480_000),
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
        group[56..72].fill(if variant == 0 { 1 } else { 3 });
        group[72..88].fill(if variant == 1 { 33 } else { 35 });
        group[88..96].copy_from_slice(&3_i64.to_be_bytes());
        group[96..100].fill(0);
        group[100..108].copy_from_slice(&(3_i64 + 30 * 86400).to_be_bytes());
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
        let recovery = open_origin(&mut fs, 2_490_000);
        let result = origin_recover(&mut fs, recovery, origin_limits());
        assert!(result.is_err(), "authenticated variant {variant}");
        if variant == 2 {
            assert!(matches!(result, Err(GraphDiskError::RootStateMismatch)));
        }
        let recovery = open_origin(&mut fs, 2_510_000);
        no_origin_roots(&recovery, &mut fs, 4);
    }
}

#[test]
fn packed_graph_origin_refuses_inventory_at_genesis_or_suffix() {
    // A deliberately incompatible test reducer certifies graph-shaped requests with inventory.
    // The graph-only recovery entry point must refuse both genesis and suffix cases.
    struct InventoryReducer(GraphState);
    impl TransactionState for InventoryReducer {
        type Prepared = <GraphState as TransactionState>::Prepared;
        type Snapshot = <GraphState as TransactionState>::Snapshot;
        fn prepare(
            &self,
            request: &[u8],
            _: Option<&uste_storage::BlobInventory>,
            revision: CommitRevision,
        ) -> Result<Self::Prepared, uste_txn::ApplyError> {
            self.0.prepare(request, None, revision)
        }
        fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
            GraphState::result_digest(prepared)
        }
        fn publish(&mut self, prepared: Self::Prepared) {
            self.0.publish(prepared);
        }
        fn snapshot(&self) -> Self::Snapshot {
            self.0.snapshot()
        }
    }
    for inventory_at in [1, 2] {
        let mut memory = MemoryFileSystem::new(64 * 1024 * 1024);
        let mut model = CommitCoordinator::create(
            &mut memory,
            scope(),
            RetentionDays::new(30).unwrap(),
            EntryName::new("packed-graph-bridge").unwrap(),
            create_vault(scope().database(), 2_530_000),
            CounterEntropy(2_540_000),
            InventoryReducer(GraphState::new(scope())),
        )
        .unwrap();
        let empty = uste_storage::BlobInventory::new(scope(), []).unwrap();
        let mut upload = model.start_blob_upload(scope()).unwrap();
        model
            .write_blob_upload(&mut memory, &mut upload, b"synthetic inventory-only source")
            .unwrap();
        let reference = model.finish_blob_upload(&mut memory, &mut upload).unwrap();
        let inventory = uste_storage::BlobInventory::new(scope(), [reference]).unwrap();
        for revision in 1..=inventory_at {
            let tx = GraphTransaction::new(
                scope(),
                vec![Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: record(revision),
                        entity_type: text("inventory-refusal"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                }],
            );
            let bytes = encode_transaction(&tx).unwrap();
            let mut request = request(revision, &bytes);
            if revision == 1 {
                request.blob_inventory = Some(&empty);
                assert_eq!(
                    model
                        .commit(&mut memory, request, &mut clock(1), &NeverCancel)
                        .unwrap_err(),
                    TransactionError::InvalidRequest
                );
                request.blob_inventory = None;
            }
            if revision == inventory_at {
                request.blob_inventory = Some(&inventory);
            }
            model
                .commit(
                    &mut memory,
                    request,
                    &mut clock(u64::from(revision)),
                    &NeverCancel,
                )
                .unwrap();
        }
        drop(model);
        let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
        let recovery = open_origin(&mut fs, 2_550_000);
        assert!(origin_recover(&mut fs, recovery, origin_limits()).is_err());
        let recovery = open_origin(&mut fs, 2_570_000);
        no_origin_roots(&recovery, &mut fs, u64::from(inventory_at) + 1);
    }
}
