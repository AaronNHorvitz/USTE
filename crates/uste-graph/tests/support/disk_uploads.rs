use super::*;
use uste_policy::AuthenticatedPrincipal;
use uste_txn::{AuthorizedDiskUploads, AuthorizedError, DiskBlobAccountingLimits};

type Fs = FaultFileSystem<MemoryFileSystem>;
type Disk = uste_txn::DiskCommitCoordinator<
    GraphDiskLiveState,
    Fs,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

pub(super) fn verify(
    disk: &mut Disk,
    fs: &mut Fs,
    kernel: &PolicyKernel,
    admin: &AuthenticatedPrincipal,
    bob: &AuthenticatedPrincipal,
) {
    let accounting = DiskBlobAccountingLimits {
        base: IndexRunReadLimits::new(100, 1000, 1024 * 1024).unwrap(),
        maximum_total_owners: 100,
    };
    let mut uploads = AuthorizedDiskUploads::new(disk, kernel, accounting).unwrap();
    assert!(!uploads.quota_usage(fs, admin).unwrap().staging_complete);
    assert!(matches!(
        uploads.start_blob_upload(admin),
        Err(AuthorizedError::ResourceLimit)
    ));
    assert!(matches!(
        uploads.start_blob_upload(bob),
        Err(AuthorizedError::Unauthorized)
    ));
    assert!(matches!(
        uploads.quota_usage(fs, bob),
        Err(AuthorizedError::Unauthorized)
    ));
    uploads
        .complete_recovered_upload_reconciliation(fs, admin, &[])
        .unwrap();
    assert!(uploads.quota_usage(fs, admin).unwrap().staging_complete);
    let mut upload = uploads.start_blob_upload(admin).unwrap();
    assert!(matches!(
        uploads.write_blob_upload(fs, bob, &mut upload, b"denied"),
        Err(AuthorizedError::Unauthorized)
    ));
    uploads
        .write_blob_upload(fs, admin, &mut upload, &vec![42; 1024 * 1024])
        .unwrap();
    assert_eq!(
        uploads
            .quota_usage(fs, admin)
            .unwrap()
            .known_usage
            .namespace_staged_bytes,
        1024 * 1024
    );
    assert!(matches!(
        uploads.write_blob_upload(fs, admin, &mut upload, b"x"),
        Err(AuthorizedError::ResourceLimit)
    ));
    let token = upload.token();
    drop(upload);
    drop(uploads);
    let mut uploads = AuthorizedDiskUploads::new(disk, kernel, accounting).unwrap();
    fs.arm(
        FaultPlan::new(vec![FaultPoint {
            operation: FaultOperation::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        uploads.resume_blob_upload(fs, bob, token),
        Err(AuthorizedError::Unauthorized)
    ));
    assert!(matches!(
        uploads.complete_recovered_upload_reconciliation(fs, admin, &[token, token]),
        Err(AuthorizedError::IntegrityFailure)
    ));
    assert_eq!(fs.pending_faults(), 1);
    assert!(
        uploads
            .complete_recovered_upload_reconciliation(fs, admin, &[token])
            .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
    assert!(matches!(
        uploads.start_blob_upload(admin),
        Err(AuthorizedError::ResourceLimit)
    ));
    assert!(matches!(
        uploads.complete_recovered_upload_reconciliation(fs, admin, &[token]),
        Err(AuthorizedError::ResourceLimit)
    ));
    let mut upload = uploads.resume_blob_upload(fs, admin, token).unwrap();
    assert_eq!(
        uploads
            .quota_usage(fs, admin)
            .unwrap()
            .known_usage
            .principal_live_uploads,
        1
    );
    uploads.abort_blob_upload(fs, admin, &mut upload).unwrap();
    uploads
        .complete_recovered_upload_reconciliation(fs, admin, &[token])
        .unwrap();
    assert_eq!(
        uploads
            .quota_usage(fs, admin)
            .unwrap()
            .known_usage
            .namespace_staged_bytes,
        0
    );
    let mut live = Vec::new();
    for _ in 0..8 {
        live.push(uploads.start_blob_upload(admin).unwrap());
    }
    assert!(matches!(
        uploads.start_blob_upload(admin),
        Err(AuthorizedError::ResourceLimit)
    ));
    for upload in &mut live {
        uploads.abort_blob_upload(fs, admin, upload).unwrap();
    }
    drop(live);
    let mut handles = Vec::new();
    for _ in 0..32 {
        let upload = uploads.start_blob_upload(admin).unwrap();
        handles.push(upload.token());
        drop(upload);
    }
    assert!(matches!(
        uploads.start_blob_upload(admin),
        Err(AuthorizedError::ResourceLimit)
    ));
    for token in handles {
        let mut upload = uploads.resume_blob_upload(fs, admin, token).unwrap();
        uploads.abort_blob_upload(fs, admin, &mut upload).unwrap();
    }
    let mut upload = uploads.start_blob_upload(admin).unwrap();
    uploads
        .write_blob_upload(fs, admin, &mut upload, b"finalized")
        .unwrap();
    assert_eq!(
        uploads
            .finish_blob_upload(fs, admin, &mut upload)
            .unwrap()
            .byte_len(),
        9
    );
    assert!(uploads.abort_blob_upload(fs, admin, &mut upload).is_err());
    assert_eq!(
        uploads
            .quota_usage(fs, admin)
            .unwrap()
            .known_usage
            .namespace_staged_bytes,
        9
    );
    drop(uploads);
    let uploads = AuthorizedDiskUploads::new(disk, kernel, accounting).unwrap();
    assert!(!uploads.quota_usage(fs, admin).unwrap().staging_complete);
}
