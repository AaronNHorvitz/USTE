use super::*;
use sha2::{Digest, Sha256};
use uste_graph::{PackedGraphAdmissionLimits, admit_packed_graph_base};
use uste_storage::{
    journal::{CertifiedPackedRoot, PackedRootDiscoveryLimits},
    packed_tree_validation::TreeValidationLimits,
};

fn claim_digest(
    root: &CertifiedPackedRoot,
    families: &[uste_storage::packed_root_manifest::PackedRootFamily],
) -> [u8; 32] {
    let claims = root.manifest().claims();
    let mut hash = Sha256::new();
    hash.update(b"USTE-GRAPH-ORDERED-STATE-V1\0");
    hash.update(scope().database().as_bytes());
    hash.update(scope().namespace().as_bytes());
    hash.update(claims.revision.get().to_be_bytes());
    hash.update(claims.reducer_profile);
    hash.update(GRAPH_PACKED_PROFILE_V1);
    for family in families {
        hash.update([family.family]);
        hash.update(family.commitment.entries().to_be_bytes());
        hash.update(family.commitment.logical_bytes().to_be_bytes());
        hash.update(family.commitment.digest());
    }
    hash.finalize().into()
}

fn cold_limits() -> PackedGraphAdmissionLimits {
    PackedGraphAdmissionLimits {
        canonical: TreeValidationLimits {
            maximum_path_branches: 512,
            maximum_nodes: 10000,
            maximum_logical_bytes: 16 * 1024 * 1024,
            maximum_pages: 100000,
            maximum_encoded_bytes: 100000 * 20545,
        },
        semantic: admission_limits(),
        scan: export_limits(),
        lookup: preparation_limits().lookup,
        maximum_lookup_encoded_bytes: 100000 * 20545,
    }
}
struct Cold {
    fs: Fs,
    recovery: Recovery,
    transaction: RecoveredFrontierTransaction,
    root: CertifiedPackedRoot,
    v1_digest: [u8; 32],
}
fn reopen(mut fs: Fs, v1_digest: [u8; 32]) -> Cold {
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let name = EntryName::new("packed-graph-bridge").unwrap();
    let (recovery, _, transaction) =
        AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
            &mut fs,
            &name,
            scope(),
            CounterEntropy(1_210_000),
            CounterEntropy(1_220_000),
            &mut TestKeyAdapter,
            limits(2).certificates,
        )
        .unwrap();
    let transaction = transaction.unwrap();
    let (roots, _) = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            transaction.revision(),
            limits(2).certificates,
            PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap(),
        )
        .unwrap();
    assert_eq!(roots.len(), 1);
    Cold {
        fs,
        recovery,
        transaction,
        root: roots.into_iter().next().unwrap(),
        v1_digest,
    }
}
fn cold(request: Option<&GraphTransaction>) -> Cold {
    if let Some(request) = request {
        let mut input = input(request);
        let (base, _) = stage_packed_graph_delta(
            &mut input.recovery,
            &mut input.fs,
            &input.base,
            &input.target,
            &input.plan,
            stage_limits(2),
        )
        .unwrap();
        input
            .recovery
            .publish_recovered_packed_root(
                &mut input.fs,
                GRAPH_PACKED_PROFILE_V1,
                base.publication_claims(),
                &base.families(),
                8,
            )
            .unwrap();
        drop(input.recovery);
        reopen(input.fs, input.new_digest)
    } else {
        let (memory, name, v1_digest) = fixture();
        let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
        let (mut recovery, source, transaction) = open(&mut fs, &name, 1_190_000);
        let (base, _) =
            bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
                .unwrap();
        recovery
            .publish_recovered_packed_root(
                &mut fs,
                GRAPH_PACKED_PROFILE_V1,
                base.publication_claims(),
                &base.families(),
                8,
            )
            .unwrap();
        drop(recovery);
        reopen(fs, v1_digest)
    }
}

#[test]
fn packed_cold_graph_admission_reopens_all_families_and_matches_frozen_reference() {
    let variants = cases();
    for request in [
        None,
        Some(&variants[0]),
        Some(&variants[6]),
        Some(&variants[18]),
        Some(&variants[20]),
    ] {
        let mut input = cold(request);
        let maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
            .unwrap();
        input.fs.arm(FaultPlan::default()).unwrap();
        let (base, report) =
            admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, cold_limits())
                .unwrap();
        assert_eq!(base.source_v1_digest(), Some(&input.v1_digest));
        assert_eq!(
            base.anchor(),
            (
                input.transaction.revision(),
                *input.transaction.certificate_digest()
            )
        );
        assert_eq!(
            base.publication_claims().state_digest,
            input.root.manifest().claims().state_digest
        );
        assert_eq!(report.semantic.scan.runs, 8);
        assert_eq!(
            report.semantic.scan.entries,
            report.canonical.iter().map(|r| r.entries).sum()
        );
        assert!(report.semantic.predecessor_lookups > 0);
        assert!(report.semantic.exact_lookups > 0);
        assert_eq!(input.fs.operation_count(FsOp::WriteAt), 0);
        assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
    }
}

#[test]
fn packed_cold_graph_admission_exact_and_independent_minus_one_limits() {
    let mut input = cold(None);
    let maintenance = input
        .recovery
        .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
        .unwrap();
    let (_, report) =
        admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, cold_limits()).unwrap();
    let semantic = &report.semantic;
    let mut exact = cold_limits();
    exact.canonical.maximum_nodes = report.canonical.iter().map(|r| r.nodes).sum();
    exact.canonical.maximum_logical_bytes = report.canonical.iter().map(|r| r.logical_bytes).sum();
    exact.canonical.maximum_pages = report.canonical.iter().map(|r| r.pages).sum();
    exact.canonical.maximum_encoded_bytes = report.canonical.iter().map(|r| r.encoded_bytes).sum();
    exact.scan.maximum_candidates = report.scan_candidates;
    exact.scan.maximum_returned_bytes = semantic.scan.logical_bytes;
    exact.scan.maximum_pages = semantic.scan.pages_read;
    exact.scan.maximum_encoded_bytes = report.scan_encoded_bytes;
    exact.maximum_lookup_encoded_bytes = report.lookup_encoded_bytes;
    let semantic_limits = |minus: usize| {
        let values = [
            report.canonical[1].entries,
            report.canonical[2].entries,
            1,
            semantic.scan.entries,
            semantic.scan.pages_read,
            semantic.scan.logical_bytes,
            2,
            semantic.peak_history_group_logical_bytes,
            semantic.semantic_reference_visits,
            semantic.exact_lookups + semantic.predecessor_lookups,
            semantic.lookup_page_visits,
            semantic.lookup_result_bytes,
        ];
        let v = values
            .into_iter()
            .enumerate()
            .map(|(i, v)| v - u64::from(i == minus))
            .collect::<Vec<_>>();
        GraphDiskBaseAdmissionLimits::new(
            GraphStateLoadLimits::new(v[0], v[1], v[2], v[3], v[4], v[5]).unwrap(),
            v[6],
            v[7],
            v[8],
            v[9],
            v[10],
            v[11],
            IndexPredecessorLimits::new(1000, 16 * 1024 * 1024).unwrap(),
        )
        .unwrap()
    };
    exact.semantic = semantic_limits(usize::MAX);
    let (base, measured) =
        admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, exact).unwrap();
    assert_eq!(base.source_v1_digest(), Some(&input.v1_digest));
    assert_eq!(measured, report);
    for field in 0..12 {
        let mut narrow = exact;
        narrow.semantic = semantic_limits(field);
        assert!(
            admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, narrow).is_err(),
            "semantic {field}"
        );
    }
    for field in 0..12 {
        let mut narrow = exact;
        match field {
            0 => narrow.canonical.maximum_nodes -= 1,
            1 => narrow.canonical.maximum_logical_bytes -= 1,
            2 => narrow.canonical.maximum_pages -= 1,
            3 => narrow.canonical.maximum_encoded_bytes -= 1,
            4 => narrow.scan.maximum_candidates -= 1,
            5 => narrow.scan.maximum_returned_bytes -= 1,
            6 => narrow.scan.maximum_pages -= 1,
            7 => narrow.scan.maximum_encoded_bytes -= 1,
            8 => narrow.maximum_lookup_encoded_bytes -= 1,
            9 => narrow.canonical.maximum_path_branches = 0,
            10 => narrow.scan.maximum_path_branches = 0,
            11 => narrow.lookup.maximum_path_branches = 0,
            _ => unreachable!(),
        }
        assert!(
            admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, narrow).is_err(),
            "packed {field}"
        );
    }
}

#[test]
fn packed_cold_graph_admission_every_read_fault_restarts_without_writes() {
    let mut observed = cold(None);
    let maintenance = observed
        .recovery
        .packed_indexes_with_io(
            &mut observed.fs,
            &observed.transaction,
            limits(2).certificates,
        )
        .unwrap();
    observed.fs.arm(FaultPlan::default()).unwrap();
    admit_packed_graph_base(
        &maintenance,
        &mut observed.fs,
        &observed.root,
        cold_limits(),
    )
    .unwrap();
    let counts = [FsOp::OpenExisting, FsOp::Metadata, FsOp::ReadAt]
        .map(|op| (op, observed.fs.operation_count(op)));
    let mut checked = 0;
    for (operation, count) in counts {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut input = cold(None);
                let maintenance = input
                    .recovery
                    .packed_indexes_with_io(
                        &mut input.fs,
                        &input.transaction,
                        limits(2).certificates,
                    )
                    .unwrap();
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
                assert!(
                    admit_packed_graph_base(
                        &maintenance,
                        &mut input.fs,
                        &input.root,
                        cold_limits()
                    )
                    .is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(input.fs.pending_faults(), 0);
                assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
                assert_eq!(input.fs.operation_count(FsOp::WriteAt), 0);
                drop(input.recovery);
                let mut input = reopen(input.fs, input.v1_digest);
                let maintenance = input
                    .recovery
                    .packed_indexes_with_io(
                        &mut input.fs,
                        &input.transaction,
                        limits(2).certificates,
                    )
                    .unwrap();
                let (base, _) = admit_packed_graph_base(
                    &maintenance,
                    &mut input.fs,
                    &input.root,
                    cold_limits(),
                )
                .unwrap();
                assert_eq!(base.source_v1_digest(), Some(&input.v1_digest));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 1377);
}

#[test]
fn packed_cold_graph_admission_rejects_authenticated_semantic_inconsistency() {
    for variant in 0..8 {
        let tx = cases()[20].clone();
        let mut input = cold(Some(&tx));
        let mut maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
            .unwrap();
        let family = match variant {
            0 => 2,
            1 | 2 => 3,
            3 => 4,
            4 => 7,
            5 | 6 => 8,
            7 => 1,
            _ => unreachable!(),
        };
        let tree = maintenance
            .admit(&mut input.fs, &input.root, family, cold_limits().canonical)
            .unwrap()
            .0;
        let mut cursor = maintenance
            .cursor(&tree, b"", None, export_limits())
            .unwrap();
        let mut entries = Vec::new();
        while let Some(entry) = maintenance.next(&mut input.fs, &mut cursor).unwrap() {
            entries.push((entry.key().to_vec(), entry.value().to_vec()));
        }
        let selected = match variant {
            2 => entries
                .iter()
                .position(|(key, _)| key[..16] == *record(4).record().as_bytes())
                .unwrap(),
            6 => 1,
            _ => 0,
        };
        let (key, before) = &entries[selected];
        let mut after = before.clone();
        match variant {
            0..=2 => {
                let mut record = uste_graph::decode_stored_record(before).unwrap();
                match &mut record {
                    Record::Entity(entity) if variant == 0 => entity.properties = Value::Bool(true),
                    Record::Entity(entity) if variant == 1 => {
                        entity.version = uste_graph::RecordVersion::new(2).unwrap()
                    }
                    Record::Relationship(relationship) if variant == 2 => {
                        relationship.from = self::record(99)
                    }
                    _ => unreachable!(),
                }
                after = encode_stored_record(&record).unwrap();
            }
            3 => after = record(3).record().as_bytes().to_vec(),
            4 => after[1] ^= 1,
            5 => after = entries[1].1.clone(), // Current policy regresses to historical v1.
            6 => after = entries[0].1.clone(), // First history becomes v2; successor duplicates v2.
            7 => after[16..24].copy_from_slice(&3_u64.to_be_bytes()),
            _ => unreachable!(),
        }
        assert_ne!(&after, before);
        let staged = maintenance
            .stage(
                &mut input.fs,
                GRAPH_PACKED_PROFILE_V1,
                family,
                Some(&tree),
                &[
                    uste_storage::IndexDelta::new(key.clone(), Some(before.clone()), Some(after))
                        .unwrap(),
                ],
                limits(2).batch,
            )
            .unwrap();
        let mut families = input.root.manifest().families().to_vec();
        families[usize::from(family - 1)] = staged.tree().family_descriptor();
        let mut claims = input.root.manifest().claims();
        claims.state_digest = claim_digest(&input.root, &families);
        let root = input
            .recovery
            .publish_recovered_packed_root(
                &mut input.fs,
                GRAPH_PACKED_PROFILE_V1,
                claims,
                &families,
                8,
            )
            .unwrap();
        let maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
            .unwrap();
        // Storage authentication and full canonical structure really pass for the changed family.
        maintenance
            .admit(&mut input.fs, &root, family, cold_limits().canonical)
            .unwrap();
        input.fs.arm(FaultPlan::default()).unwrap();
        assert!(
            admit_packed_graph_base(&maintenance, &mut input.fs, &root, cold_limits()).is_err(),
            "semantic variant {variant}"
        );
        assert_eq!(input.fs.operation_count(FsOp::WriteAt), 0);
        assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
    }
}

#[test]
fn packed_cold_graph_admission_foreign_owner_false_claims_and_late_corruption_fail_closed() {
    let mut input = cold(None);
    let foreign = cold(None);
    let maintenance = input
        .recovery
        .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
        .unwrap();
    input.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        admit_packed_graph_base(&maintenance, &mut input.fs, &foreign.root, cold_limits()).is_err()
    );
    assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
    for variant in 0..3 {
        let mut claims = input.root.manifest().claims();
        match variant {
            0 => claims.state_digest[0] ^= 1,
            1 => claims.reducer_profile[0] ^= 1,
            2 => claims.state_commitment_profile[0] ^= 1,
            _ => unreachable!(),
        }
        let root = input
            .recovery
            .publish_recovered_packed_root(
                &mut input.fs,
                GRAPH_PACKED_PROFILE_V1,
                claims,
                input.root.manifest().families(),
                8,
            )
            .unwrap();
        let maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
            .unwrap();
        input.fs.arm(FaultPlan::default()).unwrap();
        assert!(
            admit_packed_graph_base(&maintenance, &mut input.fs, &root, cold_limits()).is_err()
        );
        assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
    }
    use uste_storage::FileSystem;
    let physical = input.root.manifest().families()[7]
        .root
        .unwrap()
        .resolve(
            scope(),
            GRAPH_PACKED_PROFILE_V1,
            8,
            input.transaction.revision(),
        )
        .unwrap();
    let name = EntryName::new(format!(
        "pack-{}",
        physical
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
    .unwrap();
    let directory = input
        .fs
        .open_directory(
            &input.fs.root(),
            &EntryName::new("packed-graph-bridge").unwrap(),
        )
        .unwrap();
    let file = input.fs.open_existing(&directory, &name).unwrap();
    let offset = physical.page * 20545 + 137;
    let mut byte = [0];
    assert_eq!(input.fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(input.fs.write_at(&file, offset, &byte).unwrap(), 1);
    let maintenance = input
        .recovery
        .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
        .unwrap();
    input.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, cold_limits()).is_err()
    );
    assert!(input.fs.operation_count(FsOp::ReadAt) > 7);
    byte[0] ^= 1;
    assert_eq!(input.fs.write_at(&file, offset, &byte).unwrap(), 1);
    assert_eq!(
        admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, cold_limits())
            .unwrap()
            .0
            .source_v1_digest(),
        Some(&input.v1_digest)
    );
}

#[test]
fn packed_cold_graph_admission_explicit_empty_families_and_absent_policy() {
    for policy_only in [false, true] {
        let mut memory = MemoryFileSystem::new(16 * 1024 * 1024);
        let name = EntryName::new("packed-graph-bridge").unwrap();
        let mut coordinator = CommitCoordinator::create(
            &mut memory,
            scope(),
            RetentionDays::new(30).unwrap(),
            name.clone(),
            create_vault(scope().database(), 1_230_000),
            CounterEntropy(1_240_000),
            GraphState::new(scope()),
        )
        .unwrap();
        let transaction = if policy_only {
            GraphTransaction::with_policy_mutation(
                scope(),
                vec![],
                DurablePolicyMutation::Install {
                    policy: NamespacePolicy::new(
                        scope(),
                        PolicyVersion::new(1).unwrap(),
                        QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                    ),
                },
            )
        } else {
            GraphTransaction::new(
                scope(),
                vec![Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: record(1),
                        entity_type: text("empty-indexes"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                }],
            )
        };
        commit(&mut coordinator, &mut memory, 1, transaction);
        let snapshot = coordinator.read_view().unwrap().state().clone();
        let digest = GraphState::logical_state_digest(&snapshot).unwrap();
        publish_graph_state_root(&mut coordinator, &mut memory, &snapshot).unwrap();
        drop(coordinator);
        memory.restart().unwrap();
        let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
        let (mut recovery, source, transaction) = open(&mut fs, &name, 1_250_000);
        let (base, _) =
            bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
                .unwrap();
        recovery
            .publish_recovered_packed_root(
                &mut fs,
                GRAPH_PACKED_PROFILE_V1,
                base.publication_claims(),
                &base.families(),
                8,
            )
            .unwrap();
        drop(recovery);
        let mut input = reopen(fs, digest);
        let maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.transaction, limits(2).certificates)
            .unwrap();
        let (base, report) =
            admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, cold_limits())
                .unwrap();
        assert_eq!(base.source_v1_digest(), Some(&digest));
        assert_eq!(base.namespace_policy().is_some(), policy_only);
        assert_eq!(report.semantic.exact_lookups, u64::from(!policy_only));
        assert_eq!(report.semantic.predecessor_lookups, 0);
        let mut exact = cold_limits();
        exact.canonical.maximum_nodes = report.canonical.iter().map(|r| r.nodes).sum();
        exact.canonical.maximum_pages = report.canonical.iter().map(|r| r.pages).sum();
        exact.canonical.maximum_encoded_bytes =
            report.canonical.iter().map(|r| r.encoded_bytes).sum();
        exact.canonical.maximum_logical_bytes =
            report.canonical.iter().map(|r| r.logical_bytes).sum();
        exact.scan.maximum_candidates = report.scan_candidates;
        exact.scan.maximum_pages = report.semantic.scan.pages_read;
        exact.scan.maximum_encoded_bytes = report.scan_encoded_bytes;
        exact.scan.maximum_returned_bytes = report.semantic.scan.logical_bytes;
        assert_eq!(
            admit_packed_graph_base(&maintenance, &mut input.fs, &input.root, exact)
                .unwrap()
                .1,
            report
        );
        let state =
            uste_graph::GraphPackedLiveState::from_published(&maintenance, base, input.root)
                .unwrap();
        assert!(!state.needs_repair());
        assert_eq!(
            uste_txn::AuthorizedDiskPolicyState::current_durable_policy(&state).is_ok(),
            policy_only
        );
    }
}
