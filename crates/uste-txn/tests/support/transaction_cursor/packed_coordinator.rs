use super::*;
#[path = "packed_coordinator_admission.rs"]
mod admission;
#[path = "packed_quota.rs"]
mod quota;
use uste_txn::{
    COORDINATOR_PACKED_PROFILE_V1, PackedCoordinatorLimits, PackedCoordinatorPrefix,
    stage_packed_coordinator_prefix,
};

fn limits() -> PackedCoordinatorLimits {
    PackedCoordinatorLimits {
        certificates: CertificateAnchorReadLimits::new(64, 64 * 4161).unwrap(),
        lookup: reads(),
        batch: batches(),
        maximum_references: 4,
        maximum_owners: 4,
    }
}
fn transactions(
    fs: &mut Fs,
    name: &EntryName,
    count: u64,
) -> (Recovery, Vec<uste_txn::RecoveredFrontierTransaction>) {
    transactions_with_entropy(fs, name, count, 1402)
}
fn transactions_with_entropy(
    fs: &mut Fs,
    name: &EntryName,
    count: u64,
    entropy: u64,
) -> (Recovery, Vec<uste_txn::RecoveredFrontierTransaction>) {
    let (recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        fs,
        name,
        scope(),
        CounterEntropy(1401),
        CounterEntropy(entropy),
        &mut TestKeyAdapter,
        limits().certificates,
    )
    .unwrap();
    let mut cursor = recovery
        .open_transaction_cursor_with_certificate_window(
            CommitRevision::FIRST,
            CommitRevision::new(count).unwrap(),
            count,
            2_000_000,
            64,
        )
        .unwrap();
    let mut transactions = Vec::new();
    while let Some(transaction) = recovery
        .next_recovered_transaction(fs, &mut cursor)
        .unwrap()
    {
        transactions.push(transaction);
    }
    recovery.finish_transaction_cursor(cursor).unwrap();
    (recovery, transactions)
}
fn principal(index: u8) -> PrincipalDigest {
    PrincipalDigest::from_bytes([3 + index % 2; 32])
}
fn populated(
    count: u8,
) -> (
    Fs,
    EntryName,
    Vec<TransactionOutcome>,
    [uste_storage::BlobReference; 2],
) {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("packed-prefix").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 1301),
        CounterEntropy(1302),
        CounterState::default(),
    )
    .unwrap();
    let references = [b"first".as_slice(), b"second".as_slice()].map(|bytes| {
        let mut upload = coordinator.start_blob_upload(scope()).unwrap();
        coordinator
            .write_blob_upload(&mut fs, &mut upload, bytes)
            .unwrap();
        coordinator
            .finish_blob_upload(&mut fs, &mut upload)
            .unwrap()
    });
    let mut outcomes = Vec::new();
    for index in 0..count {
        let inventory = BlobInventory::new(
            scope(),
            if index == 0 {
                references[..1].to_vec()
            } else {
                references.to_vec()
            },
        )
        .unwrap();
        let bytes = mutation(i64::from(index), 1);
        outcomes.push(
            coordinator
                .commit(
                    &mut fs,
                    TransactionRequest {
                        principal: principal(index),
                        blob_inventory: Some(&inventory),
                        ..request(index + 1, index + 11, &bytes)
                    },
                    &mut clock(i64::from(index)),
                    &NeverCancel,
                )
                .unwrap(),
        );
    }
    drop(coordinator);
    (fs, name, outcomes, references)
}

#[test]
fn packed_prefix_matches_reference_retries_transactions_and_first_owners() {
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(b"USTE coordinator-packed-v1")),
        COORDINATOR_PACKED_PROFILE_V1
    );
    let (mut fs, name, outcomes, references) = populated(32);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 32);
    let mut prefix: Option<PackedCoordinatorPrefix> = None;
    for (index, transaction) in transactions.iter().enumerate() {
        let (next, report) = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            prefix.as_ref(),
            transaction,
            limits(),
        )
        .unwrap();
        assert_eq!(next.anchor().0, transaction.revision());
        assert_eq!(report.new_owners, u64::from(index < 2));
        assert_eq!(next.owner_count(), if index == 0 { 1 } else { 2 });
        assert_eq!(next.families()[0].commitment.entries(), index as u64 + 1);
        assert_eq!(next.families()[1].commitment.entries(), index as u64 + 1);
        if index > 0 {
            assert_eq!(
                report.batches[2].written_nodes,
                if index == 1 { 2 } else { 0 }
            );
        }
        prefix = Some(next);
    }
    let prefix = prefix.unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, transactions.last().unwrap(), limits().certificates)
        .unwrap();
    for (index, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            prefix
                .retry(
                    &maintenance,
                    &mut fs,
                    principal(index as u8),
                    IdempotencyKey::from_bytes([index as u8 + 1; 16]),
                    reads()
                )
                .unwrap()
                .0,
            Some(*outcome)
        );
        assert_eq!(
            prefix
                .transaction(&maintenance, &mut fs, outcome.transaction_id, reads())
                .unwrap()
                .0,
            Some((principal(index as u8), *outcome))
        );
    }
    for (index, reference) in references.iter().enumerate() {
        assert_eq!(
            prefix
                .owner(&maintenance, &mut fs, reference.id(), reads())
                .unwrap()
                .0,
            Some((*reference, principal(index as u8)))
        );
        assert_eq!(
            prefix
                .first_revision(&maintenance, &mut fs, reference.id(), reads())
                .unwrap()
                .0,
            Some(CommitRevision::new(index as u64 + 1).unwrap())
        );
    }
    assert!(
        prefix
            .retry(
                &maintenance,
                &mut fs,
                principal(0),
                IdempotencyKey::from_bytes([99; 16]),
                reads()
            )
            .unwrap()
            .0
            .is_none()
    );
    assert!(
        prefix
            .transaction(
                &maintenance,
                &mut fs,
                TransactionId::from_bytes([99; 16]),
                reads()
            )
            .unwrap()
            .0
            .is_none()
    );
}

#[test]
fn packed_prefix_limits_gaps_and_foreign_owners_cannot_advance_a_prefix() {
    let (mut fs, name, _, _) = populated(3);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 3);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[1], limits())
            .is_err()
    );
    for selected in [
        PackedCoordinatorLimits {
            maximum_references: 0,
            ..limits()
        },
        PackedCoordinatorLimits {
            maximum_references: 513,
            ..limits()
        },
    ] {
        assert!(
            stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                None,
                &transactions[0],
                selected
            )
            .is_err()
        );
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (prefix, _) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], limits())
            .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            Some(&prefix),
            &transactions[2],
            limits()
        )
        .is_err()
    );
    assert!(
        stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            Some(&prefix),
            &transactions[1],
            PackedCoordinatorLimits {
                maximum_owners: 1,
                ..limits()
            }
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (next, _) = stage_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        Some(&prefix),
        &transactions[1],
        limits(),
    )
    .unwrap();
    assert_eq!(next.owner_count(), 2);
    drop(recovery);
    let (mut recovery, fresh) = self::transactions(&mut fs, &name, 3);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, Some(&prefix), &fresh[1], limits())
            .is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn packed_prefix_rejects_authenticated_retry_and_transaction_collisions() {
    for retry_collision in [true, false] {
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let name = EntryName::new("packed-collision").unwrap();
        let mut journal = JournalStore::create(
            &mut fs,
            CreationOptions {
                database: scope().database(),
                final_name: name.clone(),
            },
            create_vault(scope().database(), 1501),
            CounterEntropy(1502),
        )
        .unwrap();
        let mut group: Vec<u8> = include_str!("../../../../../acceptance/r1/txn-group-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        journal
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: &group,
                    logical_event_digest: Sha256::digest(&group).into(),
                },
            )
            .unwrap();
        if retry_collision {
            group[72..88].fill(6);
        } else {
            group[56..72].fill(6);
            group[24..56].fill(4); // Transaction IDs collide across principals too.
        }
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
        let (mut recovery, transactions) = transactions(&mut fs, &name, 2);
        let (base, _) = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            None,
            &transactions[0],
            limits(),
        )
        .unwrap();
        assert!(
            stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                Some(&base),
                &transactions[1],
                limits()
            )
            .is_err()
        );
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[1], limits().certificates)
            .unwrap();
        assert_eq!(
            base.retry(
                &maintenance,
                &mut fs,
                PrincipalDigest::from_bytes([3; 32]),
                IdempotencyKey::from_bytes([4; 16]),
                reads()
            )
            .unwrap()
            .0
            .unwrap()
            .revision,
            CommitRevision::FIRST
        );
    }
}

#[test]
fn packed_prefix_every_staging_fault_preserves_prior_metadata_and_restart() {
    let (mut observed_fs, name, _, _) = populated(3);
    let (mut observed, transactions) = transactions(&mut observed_fs, &name, 3);
    let (base, _) = stage_packed_coordinator_prefix(
        &mut observed,
        &mut observed_fs,
        None,
        &transactions[0],
        limits(),
    )
    .unwrap();
    observed_fs.arm(FaultPlan::default()).unwrap();
    let (expected, _) = stage_packed_coordinator_prefix(
        &mut observed,
        &mut observed_fs,
        Some(&base),
        &transactions[1],
        limits(),
    )
    .unwrap();
    let boundaries = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ]
    .map(|operation| (operation, observed_fs.operation_count(operation)));
    let mut cases = 0;
    for (operation, count) in boundaries {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, outcomes, references) = populated(3);
                let (mut recovery, transactions) = self::transactions(&mut fs, &name, 3);
                let (base, _) = stage_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    None,
                    &transactions[0],
                    limits(),
                )
                .unwrap();
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
                    stage_packed_coordinator_prefix(
                        &mut recovery,
                        &mut fs,
                        Some(&base),
                        &transactions[1],
                        limits()
                    )
                    .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                if matches!(action, FaultAction::Error(_)) {
                    fs.arm(FaultPlan::default()).unwrap();
                    let maintenance = recovery
                        .packed_indexes_with_io(&mut fs, &transactions[1], limits().certificates)
                        .unwrap();
                    assert_eq!(
                        base.retry(
                            &maintenance,
                            &mut fs,
                            principal(0),
                            IdempotencyKey::from_bytes([1; 16]),
                            reads()
                        )
                        .unwrap()
                        .0,
                        Some(outcomes[0])
                    );
                    assert_eq!(base.owner_count(), 1);
                }
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, transactions) =
                    transactions_with_entropy(&mut fs, &name, 3, 1602);
                assert!(
                    recovery
                        .discover_packed_roots_at_revision(
                            &mut fs,
                            COORDINATOR_PACKED_PROFILE_V1,
                            transactions[2].revision(),
                            limits().certificates,
                            uste_storage::journal::PackedRootDiscoveryLimits::new(2, 8354).unwrap()
                        )
                        .unwrap()
                        .0
                        .is_empty()
                );
                let (first, _) = stage_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    None,
                    &transactions[0],
                    limits(),
                )
                .unwrap();
                let (second, _) = stage_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    Some(&first),
                    &transactions[1],
                    limits(),
                )
                .unwrap();
                assert_eq!(
                    second.families().map(|f| f.commitment),
                    expected.families().map(|f| f.commitment)
                );
                let maintenance = recovery
                    .packed_indexes_with_io(&mut fs, &transactions[1], limits().certificates)
                    .unwrap();
                assert_eq!(
                    second
                        .owner(&maintenance, &mut fs, references[0].id(), reads())
                        .unwrap()
                        .0,
                    Some((references[0], principal(0)))
                );
                assert_eq!(
                    second
                        .owner(&maintenance, &mut fs, references[1].id(), reads())
                        .unwrap()
                        .0,
                    Some((references[1], principal(1)))
                );
                assert_eq!(
                    second
                        .transaction(&maintenance, &mut fs, outcomes[1].transaction_id, reads())
                        .unwrap()
                        .0,
                    Some((principal(1), outcomes[1]))
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 123);
}

#[test]
fn packed_prefix_corrupt_owner_refuses_before_output_and_rebuilds_from_journal() {
    let (mut fs, name, _, references) = populated(3);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 3);
    let (base, _) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], limits())
            .unwrap();
    let location = base.families()[2]
        .root
        .unwrap()
        .resolve(
            scope(),
            COORDINATOR_PACKED_PROFILE_V1,
            3,
            CommitRevision::FIRST,
        )
        .unwrap();
    let object = location
        .object
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let file = fs
        .open_existing(
            &directory,
            &EntryName::new(format!("pack-{object}")).unwrap(),
        )
        .unwrap();
    let offset = location.page * 20545 + 137;
    let mut byte = [0];
    fs.read_at(&file, offset, &mut byte).unwrap();
    fs.write_at(&file, offset, &[byte[0] ^ 1]).unwrap();
    fs.sync_all(&file).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            Some(&base),
            &transactions[1],
            limits()
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    drop(recovery);
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 3, 1702);
    let (first, _) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], limits())
            .unwrap();
    let (second, _) = stage_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        Some(&first),
        &transactions[1],
        limits(),
    )
    .unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[1], limits().certificates)
        .unwrap();
    assert_eq!(
        second
            .owner(&maintenance, &mut fs, references[0].id(), reads())
            .unwrap()
            .0,
        Some((references[0], principal(0)))
    );
    assert_eq!(
        second
            .first_revision(&maintenance, &mut fs, references[0].id(), reads())
            .unwrap()
            .0,
        Some(CommitRevision::FIRST)
    );
}

#[test]
fn packed_prefix_exact_maximum_inventory_and_first_overage_are_explicit() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("packed-maximum").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 1801),
        CounterEntropy(1802),
        CounterState::default(),
    )
    .unwrap();
    let mut references = Vec::new();
    for index in 0_u64..513 {
        let mut upload = coordinator.start_blob_upload(scope()).unwrap();
        coordinator
            .write_blob_upload(&mut fs, &mut upload, &index.to_be_bytes())
            .unwrap();
        references.push(
            coordinator
                .finish_blob_upload(&mut fs, &mut upload)
                .unwrap(),
        );
    }
    let inventory = BlobInventory::new(scope(), references[..512].iter().copied()).unwrap();
    coordinator
        .commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(1, 11, &mutation(0, 1))
            },
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    let inventory = BlobInventory::new(scope(), references.iter().copied()).unwrap();
    coordinator
        .commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(2, 12, &mutation(1, 1))
            },
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 2);
    let mut selected = limits();
    selected.maximum_references = 512;
    selected.maximum_owners = 513;
    selected.batch.maximum_deltas = 512;
    selected.batch.maximum_dirty_nodes = 4096;
    selected.batch.pack = PackWriteLimits {
        maximum_pages: 128,
        maximum_records: 4096,
        maximum_payload_bytes: 1_000_000,
    };
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            None,
            &transactions[0],
            PackedCoordinatorLimits {
                maximum_references: 511,
                ..selected
            }
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (prefix, report) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], selected)
            .unwrap();
    assert_eq!(prefix.owner_count(), 512);
    assert_eq!(report.new_owners, 512);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            Some(&prefix),
            &transactions[1],
            selected
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[1], limits().certificates)
        .unwrap();
    for reference in &references[..512] {
        assert_eq!(
            prefix
                .owner(&maintenance, &mut fs, reference.id(), reads())
                .unwrap()
                .0,
            Some((*reference, principal(0)))
        );
        assert_eq!(
            prefix
                .first_revision(&maintenance, &mut fs, reference.id(), reads())
                .unwrap()
                .0,
            Some(CommitRevision::FIRST)
        );
    }
    assert!(
        prefix
            .owner(&maintenance, &mut fs, references[512].id(), reads())
            .unwrap()
            .0
            .is_none()
    );
}
