use super::*;

mod bound_roots;
mod retained;
mod reverse;
mod window;

#[test]
fn certificate_revision_lookup_matches_exact_proofs_without_preliminary_reads() {
    let mut f = Fixture::new(false);
    f.store.certificate_anchors.clear();
    for index in 0..3 {
        let expected = f.proof(index).unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        let actual = f
            .store
            .authenticate_certificate_revision(
                &mut f.fs,
                f.commits[index].revision,
                CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
            )
            .unwrap();
        assert_eq!(actual.anchor(), expected.anchor());
        assert_eq!(actual.report(), expected.report());
        assert_eq!(f.fs.operation_count(Operation::ReadAt), 3 - index as u64);
    }
    f.fs.arm(FaultPlan::default()).unwrap();
    for revision in [CommitRevision::FIRST, CommitRevision::new(4).unwrap()] {
        assert!(
            f.store
                .authenticate_certificate_revision(
                    &mut f.fs,
                    revision,
                    CertificateAnchorReadLimits::new(1, SMALL_ENVELOPE_BYTES).unwrap()
                )
                .is_err()
        );
    }
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let mut fork = Fixture::new(true);
    for sequence in 2..=3 {
        let bytes = read_bounded(
            &mut fork.fs,
            &fork.store.certificate_file,
            sequence * SMALL_ENVELOPE_BYTES,
            SMALL_ENVELOPE_BYTES,
        )
        .unwrap();
        write_all_at(
            &mut f.fs,
            &f.store.certificate_file,
            sequence * SMALL_ENVELOPE_BYTES,
            &bytes,
        )
        .unwrap();
    }
    assert!(matches!(
        f.store.authenticate_certificate_revision(
            &mut f.fs,
            CommitRevision::new(2).unwrap(),
            CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap()
        ),
        Err(StorageError::IntegrityFailure)
    ));
}

struct Fixture {
    fs: FaultFileSystem<MemoryFileSystem>,
    store: FaultStore,
    commits: Vec<DurableCommit>,
}

impl Fixture {
    fn new(fork: bool) -> Self {
        let database = DatabaseId::from_bytes([0x94; 16]);
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut fs,
            options(database, "certificate-proof"),
            create_vault(database, 940_000),
            CounterEntropy::new(941_000),
        )
        .unwrap();
        let mut commits = Vec::new();
        for sequence in 1..=3 {
            commits.push(
                store
                    .append_group(
                        &mut fs,
                        CommitInput {
                            encoded_group: if fork && sequence == 2 {
                                b"fork"
                            } else {
                                b"base"
                            },
                            logical_event_digest: [sequence; 32],
                        },
                    )
                    .unwrap(),
            );
        }
        Self { fs, store, commits }
    }
    fn proof(&mut self, first: usize) -> Result<CertificateAnchorProof, StorageError> {
        let target = self.commits[first];
        self.store.authenticate_certificate_anchor(
            &mut self.fs,
            target.revision,
            target.certificate_digest,
            CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
        )
    }
}

#[test]
fn certificate_disk_proof_uses_no_resident_history_and_stages_only_pinned_content() {
    let mut f = Fixture::new(false);
    f.store.certificate_anchors.clear();
    for first in 0..3 {
        f.fs.arm(FaultPlan::default()).unwrap();
        let proof = f.proof(first).unwrap();
        let count = 3 - first as u64;
        assert_eq!(
            proof.report(),
            CertificateAnchorReadReport {
                certificates: count,
                encoded_bytes: count * SMALL_ENVELOPE_BYTES,
            }
        );
        assert_eq!(f.fs.operation_count(Operation::ReadAt), count);
        assert_eq!(
            proof.anchor(),
            (
                f.commits[first].revision,
                f.commits[first].certificate_digest
            )
        );
        assert!(f.store.certificate_anchors.is_empty());
        f.store.validate_certificate_anchor_proof(&proof).unwrap();
        let scope = NamespaceRef::new(
            f.store.database,
            uste_types::NamespaceId::from_bytes([7; 16]),
        );
        let stage = f
            .store
            .open_proven_index_recovery_stage(scope, &proof)
            .unwrap();
        assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
        let run = f
            .store
            .stage_index_merge_visit(
                &mut f.fs,
                &stage,
                [4; 32],
                1,
                None,
                IndexRunMergeLimits::new(
                    IndexRunReadLimits::new(4, 4, 1024).unwrap(),
                    4,
                    1024,
                    4,
                    1024,
                )
                .unwrap(),
                [IndexDelta::new(
                    b"key".to_vec(),
                    None,
                    Some(b"value".to_vec()),
                )],
                &mut |_, _| Ok(()),
            )
            .unwrap()
            .run
            .unwrap();
        let creates = f.fs.operation_count(Operation::CreateNew);
        let root = f
            .store
            .finish_index_recovery_stage(
                stage,
                IndexRootInput {
                    scope,
                    revision: proof.anchor().0,
                    certificate_digest: proof.anchor().1,
                    reducer_profile: [2; 32],
                    logical_state_digest: [3; 32],
                    index_profile: [4; 32],
                },
                &[run],
            )
            .unwrap();
        assert_eq!(root.read_root().generation(), 0);
        assert_eq!(f.fs.operation_count(Operation::CreateNew), creates);
        let mut cache = PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap();
        assert_eq!(
            f.store
                .index_get_proven(
                    &mut f.fs,
                    root.read_root(),
                    &proof,
                    1,
                    b"key",
                    IndexGetLimits::new(4, 1024).unwrap(),
                    &mut cache,
                )
                .unwrap()
                .0,
            Some(b"value".to_vec())
        );
        let mut cursor = f
            .store
            .open_proven_index_run_cursor(
                &mut f.fs,
                root.read_root(),
                proof.clone(),
                1,
                IndexRunReadLimits::new(4, 4, 1024).unwrap(),
            )
            .unwrap();
        assert!(
            f.store
                .finish_proven_index_run_cursor(
                    f.store
                        .open_proven_index_run_cursor(
                            &mut f.fs,
                            root.read_root(),
                            proof.clone(),
                            1,
                            IndexRunReadLimits::new(4, 4, 1024).unwrap()
                        )
                        .unwrap()
                )
                .is_err()
        );
        if first == 2 {
            f.store
                .append_group(
                    &mut f.fs,
                    CommitInput {
                        encoded_group: b"owner continuation",
                        logical_event_digest: [4; 32],
                    },
                )
                .unwrap();
            f.store.certificate_anchors.clear();
            // Historical reads survive this owner's append; scratch-stage admission needs
            // a fresh proof of the exact recovery frontier instead.
            assert!(f.store.validate_certificate_anchor_proof(&proof).is_err());
            assert_eq!(
                f.store
                    .index_get_proven(
                        &mut f.fs,
                        root.read_root(),
                        &proof,
                        1,
                        b"key",
                        IndexGetLimits::new(4, 1024).unwrap(),
                        &mut cache
                    )
                    .unwrap()
                    .0,
                Some(b"value".to_vec())
            );
        }
        assert_eq!(
            f.store
                .next_proven_index_run_entry(&mut f.fs, &mut cursor)
                .unwrap(),
            Some(IndexEntry {
                key: b"key".to_vec(),
                value: b"value".to_vec()
            })
        );
        assert_eq!(
            f.store
                .next_proven_index_run_entry(&mut f.fs, &mut cursor)
                .unwrap(),
            None
        );
        assert_eq!(
            f.store
                .finish_proven_index_run_cursor(cursor)
                .unwrap()
                .entries,
            1
        );
        assert!(f.store.certificate_anchors.is_empty());
    }
}

#[test]
fn certificate_disk_proof_limits_and_context_binding_precede_io() {
    let mut f = Fixture::new(false);
    assert!(CertificateAnchorReadLimits::new(0, 1).is_err());
    assert!(CertificateAnchorReadLimits::new(u64::MAX, u64::MAX).is_err());
    assert!(CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES + 1).is_err());
    let target = f.commits[0];
    f.fs.arm(FaultPlan::default()).unwrap();
    for (groups, bytes) in [
        (2, 2 * SMALL_ENVELOPE_BYTES),
        (3, 3 * SMALL_ENVELOPE_BYTES - 1),
    ] {
        assert!(matches!(
            f.store.authenticate_certificate_anchor(
                &mut f.fs,
                target.revision,
                target.certificate_digest,
                CertificateAnchorReadLimits::new(groups, bytes).unwrap(),
            ),
            Err(StorageError::ResourceLimit)
        ));
    }
    assert!(
        f.store
            .authenticate_certificate_anchor(
                &mut f.fs,
                CommitRevision::new(4).unwrap(),
                target.certificate_digest,
                CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
            )
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let latest = f.store.checkpoint_anchor().unwrap();
    let live_proof = f
        .store
        .authenticate_certificate_anchor(
            &mut f.fs,
            latest.0,
            latest.1,
            CertificateAnchorReadLimits::new(1, SMALL_ENVELOPE_BYTES).unwrap(),
        )
        .unwrap();
    let database = f.store.database;
    drop(f.store);
    // A retained proof must not keep the lock/key alive or validate against a reopened owner.
    let (reopened, _) = JournalStore::open(
        &mut f.fs,
        &entry("certificate-proof"),
        database,
        CounterEntropy::new(942_000),
        CounterEntropy::new(943_000),
        &mut TestKeyAdapter,
        |_| Ok(()),
    )
    .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(reopened.checkpoint_anchor(), Some(latest));
    assert!(
        reopened
            .validate_certificate_anchor_proof(&live_proof)
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    f.store = reopened;
    let proof = f.proof(0).unwrap();
    let mut same_content_other_owner = Fixture::new(false);
    assert_eq!(
        same_content_other_owner.store.checkpoint_anchor(),
        f.store.checkpoint_anchor()
    );
    same_content_other_owner
        .fs
        .arm(FaultPlan::default())
        .unwrap();
    assert!(
        same_content_other_owner
            .store
            .validate_certificate_anchor_proof(&proof)
            .is_err()
    );
    assert_eq!(
        same_content_other_owner
            .fs
            .operation_count(Operation::ReadAt),
        0
    );
    f.fs.arm(FaultPlan::default()).unwrap();
    let epoch = f.store.epoch;
    f.store.epoch = KeyEpoch::new(epoch.get() + 1).unwrap();
    assert!(f.store.validate_certificate_anchor_proof(&proof).is_err());
    f.store.epoch = epoch;
    f.store.poisoned = true;
    assert!(f.store.validate_certificate_anchor_proof(&proof).is_err());
    assert!(f.proof(0).is_err());
    f.store.poisoned = false;
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"later",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(f.store.validate_certificate_anchor_proof(&proof).is_err());
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
}

#[test]
fn certificate_disk_proof_rejects_authenticated_forks_and_every_corrupt_certificate() {
    for entire_suffix in [false, true] {
        let mut f = Fixture::new(false);
        let mut fork = Fixture::new(true);
        assert_eq!(f.commits[0], fork.commits[0]);
        assert_ne!(f.commits[2], fork.commits[2]);
        for sequence in 2..=if entire_suffix { 3 } else { 2 } {
            let bytes = read_bounded(
                &mut fork.fs,
                &fork.store.certificate_file,
                sequence * SMALL_ENVELOPE_BYTES,
                SMALL_ENVELOPE_BYTES,
            )
            .unwrap();
            // Prove this is authentic ciphertext under the same context, not just corruption.
            decode_certificate(
                &f.store.vault,
                f.store.database,
                f.store.epoch,
                f.store.certificate_log_id,
                f.store.writer,
                CommitRevision::new(sequence).unwrap(),
                &bytes,
            )
            .unwrap();
            write_all_at(
                &mut f.fs,
                &f.store.certificate_file,
                sequence * SMALL_ENVELOPE_BYTES,
                &bytes,
            )
            .unwrap();
        }
        assert!(matches!(f.proof(0), Err(StorageError::IntegrityFailure)));
    }
    for sequence in 1..=3 {
        let mut f = Fixture::new(false);
        let offset = sequence * SMALL_ENVELOPE_BYTES;
        let mut bytes = read_bounded(
            &mut f.fs,
            &f.store.certificate_file,
            offset,
            SMALL_ENVELOPE_BYTES,
        )
        .unwrap();
        bytes[100] ^= 1;
        write_all_at(&mut f.fs, &f.store.certificate_file, offset, &bytes).unwrap();
        assert!(f.proof(0).is_err());
    }
    let mut f = Fixture::new(false);
    f.fs.set_len(&f.store.certificate_file, 4 * SMALL_ENVELOPE_BYTES - 1)
        .unwrap();
    assert!(f.proof(0).is_err());
}

#[test]
fn certificate_disk_proof_every_read_failure_returns_no_proof_and_restart_recovers() {
    for occurrence in 1..=3 {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let mut f = Fixture::new(false);
            f.fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::ReadAt,
                    occurrence,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            assert!(f.proof(0).is_err());
            assert_eq!(f.fs.pending_faults(), 0);
            let database = f.store.database;
            drop(f.store);
            f.fs.restart().unwrap();
            let (store, _) = JournalStore::open(
                &mut f.fs,
                &entry("certificate-proof"),
                database,
                CounterEntropy::new(942_000),
                CounterEntropy::new(943_000),
                &mut TestKeyAdapter,
                |_| Ok(()),
            )
            .unwrap();
            f.store = store;
            f.proof(0).unwrap();
        }
    }
}
