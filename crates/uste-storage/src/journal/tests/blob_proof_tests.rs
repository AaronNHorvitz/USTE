use super::*;

const PAYLOAD: &[u8] = b"synthetic committed blob proof";

struct Fixture {
    fs: FaultFileSystem<MemoryFileSystem>,
    store: FaultStore,
    commit: DurableCommit,
    inventory: BlobInventory,
    reference: BlobReference,
    empty: BlobReference,
    orphan: BlobReference,
}

fn limits() -> BlobReferenceProofLimits {
    BlobReferenceProofLimits::new(2, 2 * SMALL_ENVELOPE_BYTES).unwrap()
}

impl Fixture {
    fn new() -> Self {
        let database = DatabaseId::from_bytes([0x96; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([1; 16]));
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut fs,
            options(database, "blob-proof"),
            create_vault(database, 960_000),
            CounterEntropy::new(961_000),
        )
        .unwrap();
        let mut upload = |bytes: &[u8]| {
            let mut handle = store.start_blob_upload(scope).unwrap();
            store
                .write_blob_upload(&mut fs, &mut handle, bytes)
                .unwrap();
            store.finish_blob_upload(&mut fs, &mut handle).unwrap()
        };
        let reference = upload(PAYLOAD);
        let empty = upload(b"");
        let orphan = upload(PAYLOAD);
        let inventory = BlobInventory::new(scope, [reference, empty]).unwrap();
        let commit = store
            .append_group_with_inventory(
                &mut fs,
                CommitInput {
                    encoded_group: b"committed inventory",
                    logical_event_digest: [1; 32],
                },
                &inventory,
            )
            .unwrap();
        store
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: b"empty inventory",
                    logical_event_digest: [2; 32],
                },
            )
            .unwrap();
        Self {
            fs,
            store,
            commit,
            inventory,
            reference,
            empty,
            orphan,
        }
    }

    fn certificate(&mut self) -> CertificateAnchorProof {
        self.store
            .authenticate_certificate_anchor(
                &mut self.fs,
                self.commit.revision,
                self.commit.certificate_digest,
                CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
            )
            .unwrap()
    }

    fn clear_resident_metadata(&mut self) {
        // This isolates the read capability, not a claim of map-free blob recovery.
        self.store.certificate_anchors.clear();
        self.store.committed_blobs.clear();
        self.store.committed_blob_inventories.clear();
        self.store.committed_blob_bytes.clear();
    }
}

#[test]
fn blob_proof_reads_exact_committed_bytes_without_resident_maps_and_survives_append() {
    let mut f = Fixture::new();
    let certificate = f.certificate();
    f.clear_resident_metadata();
    let proof = f
        .store
        .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
        .unwrap();
    assert_eq!(proof.reference(), f.reference);
    assert_eq!(
        proof.report(),
        BlobReferenceProofReport {
            inventory_references: 2,
            encoded_bytes: 2 * SMALL_ENVELOPE_BYTES,
        }
    );
    assert!(format!("{proof:?}").contains("[REDACTED]"));
    let mut output = [0; 64];
    assert!(
        f.store
            .read_blob_range(&mut f.fs, f.reference, 0, &mut output)
            .is_err()
    );
    let count = f
        .store
        .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
        .unwrap();
    assert_eq!(&output[..count], PAYLOAD);
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"later",
                logical_event_digest: [3; 32],
            },
        )
        .unwrap();
    let count = f
        .store
        .read_proven_blob_range(&mut f.fs, &proof, 10, &mut output)
        .unwrap();
    assert_eq!(&output[..count], &PAYLOAD[10..]);
    let empty = f
        .store
        .authenticate_blob_reference(&mut f.fs, &certificate, f.empty, limits())
        .unwrap();
    assert_eq!(
        f.store
            .read_proven_blob_range(&mut f.fs, &empty, 0, &mut output)
            .unwrap(),
        0
    );
    assert!(f.store.committed_blobs.is_empty());
    assert!(f.store.committed_blob_inventories.is_empty());
    assert!(f.store.committed_blob_bytes.is_empty());
}

#[test]
fn blob_proof_limits_uncommitted_and_changed_references_fail_closed() {
    let mut f = Fixture::new();
    let certificate = f.certificate();
    for bad in [
        f.orphan,
        BlobReference::new(
            f.reference.scope(),
            f.reference.id(),
            f.reference.byte_len() + 1,
            f.reference.chunk_count(),
            f.reference.content_digest(),
        )
        .unwrap(),
        BlobReference::new(
            f.reference.scope(),
            f.reference.id(),
            f.reference.byte_len(),
            f.reference.chunk_count(),
            [9; 32],
        )
        .unwrap(),
        BlobReference::new(
            NamespaceRef::new(
                f.store.database,
                uste_types::NamespaceId::from_bytes([2; 16]),
            ),
            f.reference.id(),
            f.reference.byte_len(),
            f.reference.chunk_count(),
            f.reference.content_digest(),
        )
        .unwrap(),
    ] {
        assert!(matches!(
            f.store
                .authenticate_blob_reference(&mut f.fs, &certificate, bad, limits()),
            Err(StorageError::InvalidState)
        ));
    }
    for (allowance, reads) in [
        (
            BlobReferenceProofLimits::new(1, 2 * SMALL_ENVELOPE_BYTES).unwrap(),
            2,
        ),
        (
            BlobReferenceProofLimits::new(2, 2 * SMALL_ENVELOPE_BYTES - 1).unwrap(),
            1,
        ),
        (
            BlobReferenceProofLimits::new(2, SMALL_ENVELOPE_BYTES - 1).unwrap(),
            0,
        ),
    ] {
        f.fs.arm(FaultPlan::default()).unwrap();
        assert!(matches!(
            f.store
                .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, allowance),
            Err(StorageError::ResourceLimit)
        ));
        assert_eq!(f.fs.operation_count(Operation::ReadAt), reads);
    }
    let frontier = f.store.checkpoint_anchor().unwrap();
    let empty_certificate = f
        .store
        .authenticate_certificate_anchor(
            &mut f.fs,
            frontier.0,
            frontier.1,
            CertificateAnchorReadLimits::new(1, SMALL_ENVELOPE_BYTES).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.store
            .authenticate_blob_reference(&mut f.fs, &empty_certificate, f.reference, limits()),
        Err(StorageError::InvalidState)
    ));
    assert!(BlobReferenceProofLimits::new(0, 1).is_err());
    assert!(BlobReferenceProofLimits::new(crate::blob::MAX_BLOBS_PER_INVENTORY + 1, 1).is_err());
    assert!(BlobReferenceProofLimits::new(1, 0).is_err());
    assert!(
        BlobReferenceProofLimits::new(
            1,
            SMALL_ENVELOPE_BYTES + MAX_ENCODED_BLOB_INVENTORY_BYTES + 1
        )
        .is_err()
    );
}

#[test]
fn blob_proof_owner_poison_and_foreign_database_refuse_before_io() {
    let mut f = Fixture::new();
    let certificate = f.certificate();
    let proof = f
        .store
        .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
        .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    let mut output = [0x55; 64];
    f.store.poisoned = true;
    assert!(
        f.store
            .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
            .is_err()
    );
    assert!(
        f.store
            .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
            .is_err()
    );
    f.store.poisoned = false;
    let foreign = BlobReference::new(
        NamespaceRef::new(
            DatabaseId::from_bytes([9; 16]),
            f.reference.scope().namespace(),
        ),
        f.reference.id(),
        f.reference.byte_len(),
        f.reference.chunk_count(),
        f.reference.content_digest(),
    )
    .unwrap();
    assert!(
        f.store
            .authenticate_blob_reference(&mut f.fs, &certificate, foreign, limits())
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(output, [0x55; 64]);
    let mut other = Fixture::new();
    assert_eq!(other.store.checkpoint_anchor(), f.store.checkpoint_anchor());
    other.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        other
            .store
            .read_proven_blob_range(&mut other.fs, &proof, 0, &mut output)
            .is_err()
    );
    assert_eq!(other.fs.operation_count(Operation::ReadAt), 0);
    drop(f.store);
    f.store = reopen_fault_store(
        &mut f.fs,
        f.reference.scope().database(),
        "blob-proof",
        962_000,
    );
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        f.store
            .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
            .is_err()
    );
    assert!(
        f.store
            .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let fresh = f.certificate();
    let proof = f
        .store
        .authenticate_blob_reference(&mut f.fs, &fresh, f.reference, limits())
        .unwrap();
    assert_eq!(
        f.store
            .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
            .unwrap(),
        PAYLOAD.len()
    );
}

#[test]
fn blob_proof_every_metadata_io_failure_yields_no_proof_and_restart_recovers() {
    let mut baseline = Fixture::new();
    let certificate = baseline.certificate();
    baseline.fs.arm(FaultPlan::default()).unwrap();
    baseline
        .store
        .authenticate_blob_reference(&mut baseline.fs, &certificate, baseline.reference, limits())
        .unwrap();
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        let count = baseline.fs.operation_count(operation);
        assert!(count > 0);
        eprintln!(
            "blob_reference_proof operation={operation:?} boundaries={count} fault_cases={}",
            count * 3
        );
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new();
                let certificate = f.certificate();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    f.store
                        .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                drop(f.store);
                f.fs.restart().unwrap();
                f.store = reopen_fault_store(
                    &mut f.fs,
                    f.reference.scope().database(),
                    "blob-proof",
                    962_000,
                );
                let certificate = f.certificate();
                f.clear_resident_metadata();
                let proof = f
                    .store
                    .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
                    .unwrap();
                let mut output = [0; 64];
                let count = f
                    .store
                    .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
                    .unwrap();
                assert_eq!(&output[..count], PAYLOAD);
            }
        }
    }
}

#[test]
fn blob_proof_rechecks_certificate_inventory_and_requested_chunk_corruption() {
    for target in 0..3 {
        let mut f = Fixture::new();
        let certificate = f.certificate();
        let proof = f
            .store
            .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits())
            .unwrap();
        let (file, offset) = match target {
            0 => (f.store.certificate_file, SMALL_ENVELOPE_BYTES),
            1 => (
                f.fs.open_existing(
                    &f.store.database_directory,
                    &inventory_name(
                        &f.store.vault,
                        f.store.database,
                        f.store.epoch,
                        f.store.writer,
                        f.inventory.digest(),
                    )
                    .unwrap(),
                )
                .unwrap(),
                0,
            ),
            _ => (
                f.fs.open_existing(
                    &f.store.database_directory,
                    &crate::blob::final_chunk_name(f.reference.id(), 0).unwrap(),
                )
                .unwrap(),
                0,
            ),
        };
        let mut byte = [0];
        read_exact_at(&mut f.fs, &file, offset + 100, &mut byte).unwrap();
        byte[0] ^= 1;
        write_all_at(&mut f.fs, &file, offset + 100, &byte).unwrap();
        if target < 2 {
            assert!(matches!(
                f.store
                    .authenticate_blob_reference(&mut f.fs, &certificate, f.reference, limits()),
                Err(StorageError::IntegrityFailure)
            ));
        } else {
            let mut output = [0x55; 64];
            assert!(
                f.store
                    .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
                    .is_err()
            );
            assert_eq!(output, [0x55; 64]);
        }
    }
}
