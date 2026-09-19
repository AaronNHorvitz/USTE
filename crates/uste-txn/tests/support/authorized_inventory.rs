use super::*;
use uste_policy::{
    Action, AuthenticatedPrincipal, AuthenticationError, AuthorizationRequirement,
    AuthorizationRequirements, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, QuotaLimits, Target, TrustedPrincipalAdapter,
};
use uste_storage::journal::{
    BlobCatalogRecovery, BlobMetadataAdmissionLimits, BlobMetadataRebuildLimits,
    BlobRecoveryLimits, CertificateAnchorReadLimits, DiskBlobAppendLimits,
};
use uste_storage::{
    BlobReference, IndexGetLimits, IndexRootInput, IndexRunMergeLimits, IndexRunReadLimits,
    PageCache,
};
use uste_txn::{
    AuthorizedDiskInventoryError, AuthorizedDiskPolicyState, AuthorizedDiskUploads,
    AuthorizedError, AuthorizedTransactionRequest, AuthorizedTransactionState, CheckpointState,
    CheckpointStateError, CoordinatorDiskAdmissionLimits, CoordinatorMetadataLoadLimits,
    CoordinatorRecoveryLimits, CoordinatorTransactionAdmissionLimits, DiskBlobAccountingLimits,
    DiskCommitCoordinator, DiskCoordinatorRecoveryLimits, DiskCoordinatorState,
    DurablePolicyChange,
};

#[path = "authorized_inventory/fixtures.rs"]
mod fixtures;
#[path = "authorized_inventory/usage.rs"]
mod usage;
use fixtures::*;

fn raw<'a>(
    key: u8,
    bytes: &'a [u8],
    inventory: Option<&'a BlobInventory>,
) -> AuthorizedTransactionRequest<'a> {
    AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([key; 16]),
        transaction_id: TransactionId::from_bytes([key; 16]),
        canonical_request: bytes,
        blob_inventory: inventory,
    }
}

fn finish(
    uploads: &mut AuthorizedDiskUploads<
        '_,
        PolicyCounter,
        Fs,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    >,
    fs: &mut Fs,
    principal: &AuthenticatedPrincipal,
    bytes: &[u8],
) -> BlobReference {
    let mut upload = uploads.start_blob_upload(principal).unwrap();
    uploads
        .write_blob_upload(fs, principal, &mut upload, bytes)
        .unwrap();
    uploads
        .finish_blob_upload(fs, principal, &mut upload)
        .unwrap()
}

#[test]
fn authorized_disk_inventory_exact_quota_transfer_retry_and_cold_reconciliation() {
    exact_quota_transfer_case(false);
}

#[test]
fn authorized_indexed_inventory_exact_quota_transfer_retry_and_cold_reconciliation() {
    exact_quota_transfer_case(true);
}

fn exact_quota_transfer_case(indexed: bool) {
    let (mut fs, name, mut disk, base, kernel, alice, bob) = fixture();
    if indexed {
        usage::bootstrap(&mut fs, &mut disk);
    }
    let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let mut upload = uploads.start_blob_upload(&alice).unwrap();
    let token = upload.token();
    uploads
        .write_blob_upload(&mut fs, &alice, &mut upload, b"abc")
        .unwrap();
    let reference = uploads
        .finish_blob_upload(&mut fs, &alice, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let bytes = mutation(0, 1);
    let outcome = uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &bytes, Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(outcome.revision.get(), 2);
    let usage = uploads.quota_usage(&mut fs, &alice).unwrap().known_usage;
    assert_eq!(
        (usage.namespace_staged_bytes, usage.principal_staged_bytes),
        (0, 0)
    );
    assert_eq!(
        (
            usage.namespace_committed_bytes,
            usage.principal_committed_bytes
        ),
        (3, 3)
    );
    assert_eq!(
        uploads
            .commit_inventory(
                &mut fs,
                &alice,
                raw(2, &bytes, Some(&inventory)),
                &mut clock(1),
                &NeverCancel
            )
            .unwrap(),
        outcome
    );
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &bob,
            raw(3, &mutation(1, 1), Some(&inventory)),
            &mut clock(1),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::Unauthorized
        ))
    );
    let extra = finish(&mut uploads, &mut fs, &bob, b"x");
    let excess = BlobInventory::new(scope(), [extra]).unwrap();
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &bob,
            raw(3, &mutation(1, 1), Some(&excess)),
            &mut clock(1),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::ResourceLimit
        ))
    );
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &bob)
            .unwrap()
            .known_usage
            .principal_staged_bytes,
        1
    );
    drop(uploads);
    assert_eq!(disk.overlay_counts(), (1, 1));
    assert_eq!(disk.blob_metadata_residency(), (false, 0, 0, 0));
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen(&mut fs, &name, base);
    let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
    assert!(
        !uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .staging_complete
    );
    // Bob's finalized but uncommitted blob remains unresolved. Do not claim the consumer outbox
    // complete or reopen new-upload admission merely because Alice committed.
    assert_eq!(
        uploads
            .commit_inventory(
                &mut fs,
                &alice,
                raw(2, &bytes, Some(&inventory)),
                &mut clock(1),
                &NeverCancel
            )
            .unwrap(),
        outcome
    );
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_committed_bytes,
        3
    );
    let mut resumed = uploads.resume_blob_upload(&mut fs, &alice, token).unwrap();
    assert_eq!(
        uploads
            .finish_blob_upload(&mut fs, &alice, &mut resumed)
            .unwrap(),
        reference
    );
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_staged_bytes,
        3
    );
    uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &bytes, Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_staged_bytes,
        0
    );
}

#[test]
fn authorized_disk_inventory_denial_targets_foreign_identity_and_policy_changes_precede_io() {
    let (mut fs, _, mut disk, _, kernel, alice, _) = fixture();
    let denied = authenticated(&kernel, 3);
    let mut foreign_kernel = PolicyKernel::new();
    foreign_kernel.install_initial_policy(policy(1)).unwrap();
    let foreign = authenticated(&foreign_kernel, 1);
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    let bytes = mutation(0, 1);
    for principal in [&denied, &foreign] {
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            uploads.commit_inventory(
                &mut fs,
                principal,
                raw(2, &bytes, None),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskInventoryError::Authorization(
                AuthorizedError::Unauthorized
            ))
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    }
    for leading in [0xFF, 0xFE] {
        let mut guarded = bytes;
        guarded[0] = leading;
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            uploads.commit_inventory(
                &mut fs,
                &alice,
                raw(2, &guarded, None),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskInventoryError::Authorization(
                AuthorizedError::Unauthorized
            ))
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    }
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &alice,
            raw(2, b"revoke", None),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::Transaction(TransactionError::InvalidRequest)
        ))
    );
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &alice,
            raw(2, &[0; 17], None),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::ResourceLimit
        ))
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn authorized_disk_inventory_uncertainty_retains_charge_until_recovered_commit() {
    uncertainty_case(false);
}

#[test]
fn authorized_indexed_inventory_uncertainty_retains_charge_until_recovered_commit() {
    uncertainty_case(true);
}

fn uncertainty_case(indexed: bool) {
    for occurrence in [1, 2] {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let (mut fs, name, mut disk, base, kernel, alice, _) = fixture();
            if indexed {
                usage::bootstrap(&mut fs, &mut disk);
            }
            let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
            uploads
                .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
                .unwrap();
            let mut upload = uploads.start_blob_upload(&alice).unwrap();
            let token = upload.token();
            uploads
                .write_blob_upload(&mut fs, &alice, &mut upload, b"abc")
                .unwrap();
            let reference = uploads
                .finish_blob_upload(&mut fs, &alice, &mut upload)
                .unwrap();
            let inventory = BlobInventory::new(scope(), [reference]).unwrap();
            let bytes = mutation(0, 1);
            fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::SyncData,
                    occurrence,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                uploads.commit_inventory(
                    &mut fs,
                    &alice,
                    raw(2, &bytes, Some(&inventory)),
                    &mut clock(1),
                    &NeverCancel
                ),
                Err(AuthorizedDiskInventoryError::Authorization(
                    AuthorizedError::Transaction(TransactionError::OutcomeUnknown)
                ))
            );
            assert_eq!(fs.pending_faults(), 0);
            assert_eq!(
                uploads.quota_usage(&mut fs, &alice).err(),
                Some(AuthorizedError::Transaction(
                    TransactionError::OutcomeUnknown
                ))
            );
            drop(uploads);
            drop(disk);
            fs.restart().unwrap();
            let mut disk = reopen(&mut fs, &name, base);
            let committed = occurrence == 2 && action == FaultAction::CrashAfter;
            assert_eq!(
                PolicyCounter::checkpoint_revision(disk.state().unwrap())
                    .unwrap()
                    .get(),
                if committed { 2 } else { 1 }
            );
            let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
            assert!(
                !uploads
                    .quota_usage(&mut fs, &alice)
                    .unwrap()
                    .staging_complete
            );
            let mut resumed = uploads.resume_blob_upload(&mut fs, &alice, token).unwrap();
            assert_eq!(
                uploads
                    .finish_blob_upload(&mut fs, &alice, &mut resumed)
                    .unwrap(),
                reference
            );
            assert_eq!(
                uploads
                    .quota_usage(&mut fs, &alice)
                    .unwrap()
                    .known_usage
                    .principal_staged_bytes,
                3
            );
            assert_eq!(
                uploads
                    .commit_inventory(
                        &mut fs,
                        &alice,
                        raw(2, &bytes, Some(&inventory)),
                        &mut clock(1),
                        &NeverCancel
                    )
                    .unwrap()
                    .revision
                    .get(),
                2
            );
            let usage = uploads.quota_usage(&mut fs, &alice).unwrap().known_usage;
            assert_eq!(
                (
                    usage.principal_committed_bytes,
                    usage.principal_staged_bytes
                ),
                (3, 0)
            );
            uploads
                .complete_recovered_upload_reconciliation(&mut fs, &alice, &[token])
                .unwrap();
            assert!(
                uploads
                    .quota_usage(&mut fs, &alice)
                    .unwrap()
                    .staging_complete
            );
        }
    }
}

#[test]
fn authorized_disk_inventory_current_revocation_denies_old_exact_retry() {
    revocation_case(false);
}

#[test]
fn authorized_indexed_inventory_current_revocation_denies_old_exact_retry() {
    revocation_case(true);
}

fn revocation_case(indexed: bool) {
    let (mut fs, _, mut disk, _, mut kernel, alice, _) = fixture();
    if indexed {
        usage::bootstrap(&mut fs, &mut disk);
    }
    let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"abc");
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let bytes = mutation(0, 1);
    uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &bytes, Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(uploads);
    // Privileged synthetic domain mutation represents the separately authorized policy writer.
    disk.commit(
        &mut fs,
        request(3, 3, b"revoke"),
        &mut clock(1),
        &NeverCancel,
        lookup(),
        &mut cache(),
    )
    .unwrap();
    kernel
        .replace_namespace_policy(&alice, PolicyVersion::new(1).unwrap(), policy(2))
        .unwrap();
    let mut uploads = usage::inventory_facade(&mut disk, &kernel, indexed);
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &alice,
            raw(2, &bytes, Some(&inventory)),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::Unauthorized
        ))
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn authorized_disk_inventory_zero_byte_commit_releases_a_reservation_slot() {
    let (mut fs, _, mut disk, _, kernel, alice, _) = fixture();
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let references = (0..uste_txn::MAX_STAGED_UPLOAD_RESERVATIONS)
        .map(|_| finish(&mut uploads, &mut fs, &alice, b""))
        .collect::<Vec<_>>();
    assert!(matches!(
        uploads.start_blob_upload(&alice),
        Err(AuthorizedError::ResourceLimit)
    ));
    let inventory = BlobInventory::new(scope(), [references[0]]).unwrap();
    uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    let usage = uploads.quota_usage(&mut fs, &alice).unwrap().known_usage;
    assert_eq!(
        (
            usage.principal_committed_bytes,
            usage.principal_staged_bytes
        ),
        (0, 0)
    );
    assert!(uploads.start_blob_upload(&alice).is_ok());
}

#[test]
fn authorized_disk_inventory_accounting_admission_and_staging_only_mode_keep_reservations() {
    let (mut fs, _, mut disk, _, kernel, alice, _) = fixture();
    let mut uploads = AuthorizedDiskUploads::new(&mut disk, &kernel, accounting()).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), None),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::Transaction(TransactionError::InvalidRequest)
        ))
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    drop(uploads);
    let bounded = DiskBlobAccountingLimits {
        maximum_total_owners: 0,
        ..accounting()
    };
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        bounded,
        append_limits(),
    )
    .unwrap();
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"abc");
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uploads.commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), Some(&inventory)),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::ResourceLimit
        ))
    );
    let usage = uploads.quota_usage(&mut fs, &alice).unwrap().known_usage;
    assert_eq!(
        (
            usage.principal_staged_bytes,
            usage.principal_committed_bytes
        ),
        (3, 0)
    );
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn authorized_disk_inventory_changed_unknown_and_foreign_references_share_denial() {
    let (mut fs, _, mut disk, _, kernel, alice, _) = fixture();
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"abc");
    let foreign = NamespaceRef::new(DatabaseId::from_bytes([0xFF; 16]), scope().namespace());
    for changed in [
        BlobReference::new(
            scope(),
            reference.id(),
            reference.byte_len(),
            reference.chunk_count(),
            [0xEE; 32],
        )
        .unwrap(),
        BlobReference::new(
            scope(),
            uste_storage::BlobId::from_bytes([0xEE; 16]),
            reference.byte_len(),
            reference.chunk_count(),
            reference.content_digest(),
        )
        .unwrap(),
        BlobReference::new(
            foreign,
            reference.id(),
            reference.byte_len(),
            reference.chunk_count(),
            reference.content_digest(),
        )
        .unwrap(),
    ] {
        let inventory = BlobInventory::new(changed.scope(), [changed]).unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            uploads.commit_inventory(
                &mut fs,
                &alice,
                raw(2, &mutation(0, 1), Some(&inventory)),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskInventoryError::Authorization(
                AuthorizedError::Unauthorized
            ))
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    }
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_staged_bytes,
        3
    );
}

#[test]
fn authorized_disk_inventory_known_commit_policy_drift_never_claims_rollback() {
    let (mut fs, _, mut disk, _, mut kernel, alice, _) = fixture();
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"abc");
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let error = uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, b"drift", Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap_err();
    let AuthorizedDiskInventoryError::CommittedPolicy { outcome, error } = error else {
        panic!("known outcome required")
    };
    assert_eq!(outcome.revision.get(), 2);
    assert_eq!(error, AuthorizedError::InvalidPolicy);
    assert_eq!(
        uploads.quota_usage(&mut fs, &alice).err(),
        Some(AuthorizedError::InvalidPolicy)
    );
    drop(uploads);
    assert_eq!(
        disk.outcome(
            &mut fs,
            alice.digest(),
            IdempotencyKey::from_bytes([2; 16]),
            instant(1),
            lookup(),
            &mut cache()
        )
        .unwrap(),
        Some(outcome)
    );
    kernel
        .replace_namespace_policy(&alice, PolicyVersion::new(1).unwrap(), policy(2))
        .unwrap();
    let uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    let usage = uploads.quota_usage(&mut fs, &alice).unwrap().known_usage;
    assert_eq!(
        (
            usage.principal_committed_bytes,
            usage.principal_staged_bytes
        ),
        (3, 0)
    );
}
