use super::*;
use uste_txn::{
    AuthenticatedIndexRecovery, COORDINATOR_BLOB_USAGE_PROFILE_V1, CoordinatorDiskBase,
};

type Recovery = AuthenticatedIndexRecovery<Fs, TestEnvelope, CounterEntropy, CounterEntropy>;

fn open(
    fs: &mut Fs,
    name: &EntryName,
) -> (
    Recovery,
    CoordinatorDiskBase,
    uste_storage::RecoveredIndexRoot,
) {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(30_000_000);
    let entropy = ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let recovery = Recovery::open_with_disk_certificate_anchors(
        fs,
        name,
        scope(),
        CounterEntropy::new(entropy),
        CounterEntropy::new(entropy + 5_000),
        &mut TestKeyAdapter,
        uste_storage::journal::CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
    )
    .unwrap()
    .0;
    let candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<CounterState, _, _, _, _>(
            &recovery, fs,
        )
        .unwrap()
        .into_iter()
        .find(|root| root.revision().get() == 2)
        .unwrap();
    let transaction = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.revision().get() == 2)
        .unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let transaction = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        fs,
        transaction,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: limits().run,
            lookup: limits().lookup,
            maximum_groups: 2,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let base = uste_txn::admit_coordinator_disk_base(
        &recovery,
        fs,
        candidate,
        transaction,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(2, 1, 4, 32, 4096).unwrap(),
            lookup: limits().lookup,
            maximum_total_journal_groups: 4,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let root = recovery
        .load_index_root_manifests(fs, COORDINATOR_BLOB_USAGE_PROFILE_V1)
        .unwrap()
        .into_iter()
        .max_by_key(|root| root.generation())
        .unwrap();
    (recovery, base, root)
}

#[test]
fn indexed_blob_usage_admission_rejects_authenticated_false_totals_and_owners() {
    for mutation in 0..13 {
        let (mut fs, name, mut disk, _, _) = fixture(false);
        rebase(&mut fs, &mut disk).unwrap();
        drop(disk);
        fs.restart().unwrap();
        // Test-only legacy writer makes fully authenticated but semantically false cache roots.
        let (mut writer, _) = CommitCoordinator::open(
            &mut fs,
            &name,
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy::new(40_000_000),
            CounterEntropy::new(40_005_000),
            &mut TestKeyAdapter,
            CounterState::new(scope()),
        )
        .unwrap();
        let root = writer
            .load_index_root_manifests(&mut fs, COORDINATOR_BLOB_USAGE_PROFILE_V1)
            .unwrap()
            .remove(0);
        let mut families: [Vec<IndexEntry>; 3] = Default::default();
        for family in 1..=3 {
            writer
                .visit_index_run(&mut fs, &root, family, limits().run, &mut |key, value| {
                    families[usize::from(family - 1)].push(IndexEntry {
                        key: key.to_vec(),
                        value: value.to_vec(),
                    });
                    Ok(())
                })
                .unwrap();
        }
        match mutation {
            0 => {}
            1 => families[0][0].value[7] = 0, // missing owner count
            2 => families[0][0].value[15] ^= 1, // false namespace bytes
            3 => families[0][0].value[23] = 2, // invented principal
            4 => families[1][0].value[7] = 0, // zero aggregate count
            5 => families[1][0].value[15] ^= 1, // false principal bytes
            6 => families[1][0].key[0] ^= 1,  // missing principal lookup
            7 => families[2][0].key[0] ^= 1,  // principal/key mismatch
            8 => families[2][0].key[47] ^= 1, // missing primary owner
            9 => families[2][0].value[11] ^= 1, // changed reference length
            10 => {
                families[2][0].key[0] ^= 1;
                families[2][0].value[48] ^= 1;
            } // coherent foreign owner
            11 => {
                families[0][0].key.push(0);
            }
            12 => {
                families[2][0].value.push(0);
            }
            _ => unreachable!(),
        }
        let mut runs = Vec::new();
        for (slot, entries) in families.into_iter().enumerate() {
            runs.push(
                writer
                    .publish_index_run(
                        &mut fs,
                        root.revision(),
                        COORDINATOR_BLOB_USAGE_PROFILE_V1,
                        (slot + 1) as u8,
                        entries,
                    )
                    .unwrap(),
            );
        }
        writer
            .publish_index_root(
                &mut fs,
                IndexRootInput {
                    scope: scope(),
                    revision: root.revision(),
                    certificate_digest: *root.certificate_digest(),
                    reducer_profile: *root.reducer_profile(),
                    logical_state_digest: *root.logical_state_digest(),
                    index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
                },
                &runs,
            )
            .unwrap();
        drop(writer);
        fs.restart().unwrap();
        let (recovery, mut base, root) = open(&mut fs, &name);
        assert_eq!(
            base.admit_blob_usage_index(
                &recovery,
                &mut fs,
                root,
                limits(),
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_ok(),
            mutation == 0,
            "mutation {mutation}"
        );
        assert_eq!(base.has_blob_usage_index(), mutation == 0);
    }
}

#[test]
fn indexed_blob_usage_admission_every_read_fault_is_atomic_and_bounds_precede_io() {
    let (mut fs, name, mut disk, _, _) = fixture(false);
    rebase(&mut fs, &mut disk).unwrap();
    drop(disk);
    fs.restart().unwrap();
    let (recovery, mut base, root) = open(&mut fs, &name);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        base.admit_blob_usage_index(
            &recovery,
            &mut fs,
            root.clone(),
            uste_txn::CoordinatorBlobUsageLimits {
                maximum_owners: 0,
                ..limits()
            },
            &mut PageCache::new(64 * 1024).unwrap()
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert!(!base.has_blob_usage_index());
    let exact = uste_txn::CoordinatorBlobUsageLimits {
        run: IndexRunReadLimits::new(1, 1, 128).unwrap(),
        // Binary search and selected-page access each consume a visit, even on cache hits.
        lookup: IndexGetLimits::new(2, 80).unwrap(),
        maximum_owners: 1,
    };
    assert!(
        base.admit_blob_usage_index(
            &recovery,
            &mut fs,
            root.clone(),
            uste_txn::CoordinatorBlobUsageLimits {
                run: IndexRunReadLimits::new(1, 1, 127).unwrap(),
                ..exact
            },
            &mut PageCache::new(64 * 1024).unwrap()
        )
        .is_err()
    );
    assert!(!base.has_blob_usage_index());
    for lookup in [
        IndexGetLimits::new(1, 80).unwrap(),
        IndexGetLimits::new(2, 79).unwrap(),
    ] {
        assert!(
            base.admit_blob_usage_index(
                &recovery,
                &mut fs,
                root.clone(),
                uste_txn::CoordinatorBlobUsageLimits { lookup, ..exact },
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        assert!(!base.has_blob_usage_index());
    }
    base.admit_blob_usage_index(
        &recovery,
        &mut fs,
        root.clone(),
        exact,
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    base.admit_blob_usage_index(
        &recovery,
        &mut fs,
        root.clone(),
        limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 0);
    for occurrence in 1..=reads {
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        assert!(
            base.admit_blob_usage_index(
                &recovery,
                &mut fs,
                root.clone(),
                limits(),
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        assert_eq!(fs.pending_faults(), 0);
        assert!(base.has_blob_usage_index()); // prior independently admitted projection survives
    }
    fs.arm(FaultPlan::default()).unwrap();
    base.admit_blob_usage_index(
        &recovery,
        &mut fs,
        root,
        limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    eprintln!("quota cold admission: {reads} read faults");
}
