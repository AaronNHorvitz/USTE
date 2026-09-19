use super::*;
use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
use uste_storage::{IndexGetLimits, IndexRunReadLimits, PageCache};
use uste_txn::{COORDINATOR_FIRST_REFERENCE_PROFILE_V1, CoordinatorFirstReferenceLimits};

#[test]
fn first_reference_claims_require_an_actual_earliest_match_and_all_reads_succeed() {
    // The first owner appears at revision two. Claiming revision one must not pass merely
    // because it precedes every reference; the terminal exact first-match count proves existence.
    for claimed in [0_u64, 1, 2, 3, 4, 5] {
        let scope = scope();
        let name = EntryName::new("first-reference-proof").unwrap();
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let vault = KeyVault::create(
            scope.database(),
            &mut TestKeyAdapter,
            CounterEntropy::new(900_000),
        )
        .unwrap();
        let mut coordinator = CommitCoordinator::create(
            &mut fs,
            scope,
            RetentionDays::new(30).unwrap(),
            name.clone(),
            vault,
            CounterEntropy::new(910_000),
            CounterState::new(scope),
        )
        .unwrap();
        let mut clock = TestClock(20);
        coordinator
            .commit(
                &mut fs,
                request(1, &1_u64.to_be_bytes()),
                &mut clock,
                &NeverCancel,
            )
            .unwrap();
        let mut upload = coordinator.start_blob_upload(scope).unwrap();
        coordinator
            .write_blob_upload(&mut fs, &mut upload, b"owner begins at revision two")
            .unwrap();
        let reference = coordinator
            .finish_blob_upload(&mut fs, &mut upload)
            .unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        coordinator
            .commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: Some(&inventory),
                    ..request(2, &2_u64.to_be_bytes())
                },
                &mut clock,
                &NeverCancel,
            )
            .unwrap();
        publish_coordinator_metadata_root(&mut coordinator, &mut fs).unwrap();
        uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut fs).unwrap();
        uste_txn::publish_coordinator_first_reference_index(
            &mut coordinator,
            &mut fs,
            CoordinatorFirstReferenceLimits {
                maximum_owners: 1,
                maximum_groups: 2,
                maximum_encoded_bytes: 1_000_000,
            },
        )
        .unwrap();
        let root = coordinator
            .load_index_root_manifests(&mut fs, COORDINATOR_FIRST_REFERENCE_PROFILE_V1)
            .unwrap()
            .remove(0);
        let run = coordinator
            .publish_index_run(
                &mut fs,
                root.revision(),
                COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
                1,
                [IndexEntry {
                    key: if claimed == 4 {
                        vec![0xee; 16]
                    } else {
                        reference.id().as_bytes().to_vec()
                    },
                    value: if claimed == 5 {
                        vec![0; 7]
                    } else if claimed == 4 {
                        2_u64.to_be_bytes().to_vec()
                    } else {
                        claimed.to_be_bytes().to_vec()
                    },
                }],
            )
            .unwrap();
        let publication = coordinator
            .publish_index_root(
                &mut fs,
                IndexRootInput {
                    scope,
                    revision: root.revision(),
                    certificate_digest: *root.certificate_digest(),
                    reducer_profile: *root.reducer_profile(),
                    logical_state_digest: *root.logical_state_digest(),
                    index_profile: COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
                },
                &[run],
            )
            .unwrap();
        drop(coordinator);
        fs.restart().unwrap();
        let (recovery, _) = uste_txn::AuthenticatedIndexRecovery::open(
            &mut fs,
            &name,
            scope,
            CounterEntropy::new(920_000),
            CounterEntropy::new(930_000),
            &mut TestKeyAdapter,
        )
        .unwrap();
        let lookup = IndexGetLimits::new(16, 136).unwrap();
        let run_limits = IndexRunReadLimits::new(16, 10, 4096).unwrap();
        let mut reads = 0;
        let mut occurrence = 0;
        loop {
            let mut cache = PageCache::new(64 * 1024).unwrap();
            let candidate = uste_txn::load_coordinator_metadata_candidates_for_recovery::<
                CounterState,
                _,
                _,
                _,
                _,
            >(&recovery, &mut fs)
            .unwrap()
            .remove(0);
            let transaction_root = recovery
                .load_index_root_manifests(&mut fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
                .unwrap()
                .remove(0);
            let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
                &recovery,
                &mut fs,
                transaction_root,
                uste_txn::CoordinatorTransactionAdmissionLimits {
                    run: run_limits,
                    lookup,
                    maximum_groups: 2,
                    maximum_encoded_bytes: 1_000_000,
                },
                &mut cache,
            )
            .unwrap();
            let proof = recovery
                .load_index_root_manifests(&mut fs, COORDINATOR_FIRST_REFERENCE_PROFILE_V1)
                .unwrap()
                .into_iter()
                .find(|root| root.generation() == publication.generation)
                .unwrap();
            cache = PageCache::new(64 * 1024).unwrap();
            let plan = if occurrence == 0 {
                FaultPlan::default()
            } else {
                FaultPlan::new([FaultPoint {
                    operation: Operation::ReadAt,
                    occurrence,
                    action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                }])
                .unwrap()
            };
            fs.arm(plan).unwrap();
            let result = uste_txn::admit_coordinator_disk_base_with_first_references(
                &recovery,
                &mut fs,
                candidate,
                transactions,
                proof,
                run_limits,
                uste_txn::CoordinatorDiskAdmissionLimits {
                    metadata: CoordinatorMetadataLoadLimits::new(2, 1, 4, 32, 4096).unwrap(),
                    lookup,
                    maximum_total_journal_groups: 2,
                    maximum_encoded_bytes_per_pass: 1_000_000,
                },
                &mut cache,
            );
            assert_eq!(result.is_ok(), claimed == 2 && occurrence == 0);
            assert_eq!(fs.pending_faults(), 0);
            if occurrence == 0 {
                reads = fs.operation_count(Operation::ReadAt);
            }
            if claimed != 2 || occurrence == reads {
                break;
            }
            occurrence += 1;
        }
        if claimed == 2 {
            assert!(reads > 10);
        }
    }
}
