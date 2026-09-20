use super::*;
mod trees;
use crate::{
    ordered_commitment::{self, CommitmentContext},
    packed_index_pack::PackWriteLimits,
    packed_index_page::{ENCODED_PAGE_BYTES, PackedPageContext},
    packed_root_manifest::*,
    packed_tree_batch::{TreeBatchLimits, stage_batch},
    packed_tree_lookup::{TreeLookupLimits, TreeReadContext, lookup},
};
const PROFILE: [u8; 32] = [44; 32];
const FILE_BYTES: u64 = 4177;
fn scope(f: &Fixture) -> NamespaceRef {
    NamespaceRef::new(
        f.store.database,
        uste_types::NamespaceId::from_bytes([7; 16]),
    )
}
fn claims(f: &Fixture) -> PackedRootClaims {
    let (revision, certificate_digest) = f.store.checkpoint_anchor().unwrap();
    PackedRootClaims {
        revision,
        certificate_digest,
        generation: 1,
        reducer_profile: [2; 32],
        state_commitment_profile: [3; 32],
        state_digest: [4; 32],
    }
}
fn families(f: &Fixture) -> Vec<PackedRootFamily> {
    vec![PackedRootFamily {
        family: 1,
        root: None,
        commitment: ordered_commitment::empty_commitment(
            CommitmentContext::new(scope(f), PROFILE, 1).unwrap(),
        ),
    }]
}
fn publish(f: &mut Fixture, attempts: u8) -> Result<CertifiedPackedRoot, StorageError> {
    let scope = scope(f);
    let claims = claims(f);
    let families = families(f);
    f.store
        .publish_packed_root(&mut f.fs, scope, PROFILE, claims, &families, attempts)
}
fn limits(attempts: u8) -> PackedRootDiscoveryLimits {
    PackedRootDiscoveryLimits::new(attempts, attempts as u64 * FILE_BYTES).unwrap()
}
fn discover(
    f: &mut Fixture,
    proof: &CertificateAnchorProof,
    limits: PackedRootDiscoveryLimits,
) -> Result<(Vec<CertifiedPackedRoot>, PackedRootDiscoveryReport), StorageError> {
    let scope = scope(f);
    f.store
        .discover_packed_roots_proven(&mut f.fs, scope, PROFILE, proof, limits)
}
fn reopen(mut f: Fixture) -> Fixture {
    let database = f.store.database;
    drop(f.store);
    f.fs.restart().unwrap();
    let (store, _) = JournalStore::open_with_disk_certificate_anchors(
        &mut f.fs,
        &entry("certificate-proof"),
        database,
        CounterEntropy::new(945_000),
        CounterEntropy::new(946_000),
        &mut TestKeyAdapter,
        CertificateAnchorReadLimits::new(4, 4 * SMALL_ENVELOPE_BYTES).unwrap(),
        |_| Ok(()),
    )
    .unwrap();
    assert!(store.certificate_anchors.is_empty());
    Fixture {
        fs: f.fs,
        store,
        commits: f.commits,
    }
}

#[test]
fn packed_roots_append_only_attempts_and_disk_only_cold_discovery() {
    let mut f = Fixture::new(false);
    let before = f.store.checkpoint_anchor();
    f.fs.arm(FaultPlan::default()).unwrap();
    let first = publish(&mut f, 2).unwrap();
    assert_eq!(first.manifest().claims().generation, 1);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::RemoveFile), 0);
    let second = publish(&mut f, 2).unwrap();
    assert_eq!(second.manifest().claims().generation, 2);
    assert_eq!(publish(&mut f, 2).err(), Some(StorageError::ResourceLimit));
    assert_eq!(f.store.checkpoint_anchor(), before);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::RemoveFile), 0);
    let mut f = reopen(f);
    assert!(f.store.validate_packed_root_certificate(&first).is_err());
    let proof = f.proof(2).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    let (roots, report) = discover(&mut f, &proof, limits(2)).unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(
        report,
        PackedRootDiscoveryReport {
            slots: 2,
            manifest_files: 2,
            encoded_bytes: 2 * FILE_BYTES
        }
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 2);
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 2);
    for root in &roots {
        f.store.validate_packed_root_certificate(root).unwrap();
    }
    assert_eq!(
        discover(
            &mut f,
            &proof,
            PackedRootDiscoveryLimits::new(2, 2 * FILE_BYTES - 1).unwrap()
        )
        .err(),
        Some(StorageError::ResourceLimit)
    );
    // Later publication preserves both older revision slots; the older handle stays bound to this owner.
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"four",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    f.store.validate_packed_root_certificate(&roots[0]).unwrap();
    assert!(discover(&mut f, &proof, limits(2)).is_err());
    publish(&mut f, 1).unwrap();
    let historical = f
        .store
        .authenticate_certificate_revision(
            &mut f.fs,
            CommitRevision::new(3).unwrap(),
            CertificateAnchorReadLimits::new(2, 2 * SMALL_ENVELOPE_BYTES).unwrap(),
        )
        .unwrap();
    assert_eq!(discover(&mut f, &historical, limits(2)).unwrap().0.len(), 2);
}

#[test]
fn packed_roots_binding_limits_and_poison_refuse_before_output_or_reads() {
    let mut f = Fixture::new(false);
    let proof = f.proof(2).unwrap();
    let foreign = Fixture::new(false);
    let input = claims(&f);
    let scoped = scope(&f);
    let family = families(&f);
    f.fs.arm(FaultPlan::default()).unwrap();
    for changed in [
        PackedRootClaims {
            revision: CommitRevision::FIRST,
            ..input
        },
        PackedRootClaims {
            certificate_digest: [0; 32],
            ..input
        },
    ] {
        assert!(
            f.store
                .publish_packed_root(&mut f.fs, scoped, PROFILE, changed, &family, 2)
                .is_err()
        );
    }
    for attempts in [0, MAX_PACKED_ROOT_ATTEMPTS + 1] {
        assert!(publish(&mut f, attempts).is_err());
        assert!(PackedRootDiscoveryLimits::new(attempts, FILE_BYTES).is_err());
    }
    assert!(PackedRootDiscoveryLimits::new(1, 0).is_err());
    assert!(PackedRootDiscoveryLimits::new(1, FILE_BYTES + 1).is_err());
    assert!(
        f.store
            .publish_packed_root(&mut f.fs, scoped, PROFILE, input, &[], 2)
            .is_err()
    );
    let wrong = NamespaceRef::new(DatabaseId::from_bytes([0; 16]), scoped.namespace());
    assert!(
        f.store
            .publish_packed_root(&mut f.fs, wrong, PROFILE, input, &family, 2)
            .is_err()
    );
    assert!(
        f.store
            .discover_packed_roots_proven(&mut f.fs, wrong, PROFILE, &proof, limits(2))
            .is_err()
    );
    assert!(
        foreign
            .store
            .discover_packed_roots_proven(&mut f.fs, scoped, PROFILE, &proof, limits(2))
            .is_err()
    );
    let nonce_report = f.store.vault_nonce_report().unwrap();
    assert_eq!(nonce_report, f.store.vault.nonce_report());
    f.store.poisoned = true;
    assert_eq!(
        f.store.vault_nonce_report(),
        Err(StorageError::NeedsRecovery)
    );
    assert!(publish(&mut f, 2).is_err());
    assert!(discover(&mut f, &proof, limits(2)).is_err());
    f.store.poisoned = false;
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    f.store.vault.lock();
    assert_eq!(f.store.vault_nonce_report().unwrap(), nonce_report);
    assert!(publish(&mut f, 2).is_err());
    assert!(discover(&mut f, &proof, limits(2)).is_err());
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
}

#[test]
fn packed_roots_write_faults_keep_existing_fallback_and_retry_in_new_slot() {
    let mut observed = Fixture::new(false);
    publish(&mut observed, 3).unwrap();
    observed.fs.arm(FaultPlan::default()).unwrap();
    publish(&mut observed, 3).unwrap();
    let mut cases = 0;
    for operation in [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        for occurrence in 1..=observed.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                publish(&mut f, 3).unwrap();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = publish(&mut f, 3);
                if operation == Operation::CreateNew
                    && occurrence == 1
                    && action == FaultAction::CrashAfter
                {
                    // The first slot already exists. The harness injects CrashAfter only after
                    // successful adapter operations; this boundary is explicitly a no-op.
                    assert_eq!(result.unwrap().manifest().claims().generation, 2);
                    assert!(!f.fs.is_crashed());
                } else {
                    assert!(result.is_err(), "{operation:?} {occurrence} {action:?}");
                }
                assert_eq!(f.fs.pending_faults(), 0);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                let candidates = discover(&mut f, &proof, limits(3)).unwrap().0;
                assert!(
                    candidates
                        .iter()
                        .any(|root| root.manifest().claims().generation == 1)
                );
                let retried = publish(&mut f, 3).unwrap();
                f.store.validate_packed_root_certificate(&retried).unwrap();
                assert_eq!(
                    f.store.checkpoint_anchor(),
                    Some((f.commits[2].revision, f.commits[2].certificate_digest))
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 21);
}

#[test]
fn packed_roots_read_faults_are_operational_errors_not_false_absence() {
    let mut observed = Fixture::new(false);
    publish(&mut observed, 2).unwrap();
    publish(&mut observed, 2).unwrap();
    let proof = observed.proof(2).unwrap();
    observed.fs.arm(FaultPlan::default()).unwrap();
    discover(&mut observed, &proof, limits(2)).unwrap();
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        for occurrence in 1..=observed.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                publish(&mut f, 2).unwrap();
                publish(&mut f, 2).unwrap();
                let proof = f.proof(2).unwrap();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(discover(&mut f, &proof, limits(2)).is_err());
                assert_eq!(f.fs.pending_faults(), 0);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                assert_eq!(discover(&mut f, &proof, limits(2)).unwrap().0.len(), 2);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 18);
}

#[test]
fn packed_roots_nonempty_tree_publishes_without_fallback_reads_and_reopens() {
    let mut f = Fixture::new(false);
    let scope = scope(&f);
    let input = claims(&f);
    let context = TreeReadContext {
        scope,
        profile: PROFILE,
        family: 1,
        revision: input.revision,
    };
    let empty = families(&f)[0].commitment;
    let stage = stage_batch(
        &mut f.fs,
        &f.store.database_directory,
        &mut f.store.vault,
        &mut CounterEntropy::new(950_000),
        context,
        empty,
        None,
        &[IndexDelta::new(b"key".to_vec(), None, Some(vec![9; 20_000])).unwrap()],
        PackedPageContext {
            scope,
            profile: PROFILE,
            family: 1,
            creation_revision: input.revision,
            epoch: f.store.epoch,
            writer: f.store.writer,
            object: [0; 16],
            page: 0,
        },
        TreeBatchLimits {
            maximum_deltas: 1,
            maximum_input_bytes: 32_000,
            maximum_dirty_nodes: 128,
            maximum_path_branches: 128,
            maximum_read_pages: 128,
            maximum_read_bytes: 128 * ENCODED_PAGE_BYTES as u64,
            pack: PackWriteLimits {
                maximum_pages: 8,
                maximum_records: 8,
                maximum_payload_bytes: 32_000,
            },
        },
    )
    .unwrap();
    let families = [PackedRootFamily {
        family: 1,
        commitment: stage.logical_root(),
        root: stage.root().map(|r| r.location),
    }];
    f.fs.arm(FaultPlan::default()).unwrap();
    let published = f
        .store
        .publish_packed_root(&mut f.fs, scope, PROFILE, input, &families, 1)
        .unwrap();
    assert!(published.manifest().families() == families);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let mut f = reopen(f);
    let proof = f.proof(2).unwrap();
    let (roots, _) = discover(&mut f, &proof, limits(1)).unwrap();
    assert_eq!(roots.len(), 1);
    let family = roots[0].manifest().families()[0];
    let result = lookup(
        &mut f.fs,
        &f.store.database_directory,
        &f.store.vault,
        context,
        family.commitment,
        family.root,
        b"key",
        TreeLookupLimits {
            maximum_path_branches: 128,
            maximum_pages: 8,
            maximum_encoded_bytes: 8 * ENCODED_PAGE_BYTES as u64,
            maximum_value_bytes: 32_000,
        },
    )
    .unwrap();
    assert_eq!(result.value.unwrap().as_slice(), vec![9; 20_000]);
}

fn slot_name(f: &Fixture, attempt: u8) -> EntryName {
    // Independent reproduction of the documented name domain, not the production helper.
    let scope = scope(f);
    let mut hash = Sha256::new();
    hash.update(b"USTE-PACKED-ROOT-NAME-V1\0");
    hash.update(scope.namespace().as_bytes());
    hash.update(PROFILE);
    hash.update(claims(f).revision.get().to_be_bytes());
    hash.update([attempt]);
    let input: [u8; 32] = hash.finalize().into();
    let token = f
        .store
        .vault
        .derive_opaque_identifier(
            CryptoContext::new(
                scope.database(),
                Scope::Namespace(scope.namespace()),
                f.store.epoch,
                ObjectRole::IndexName,
                CryptoObjectId::from_bytes([0; 16]),
                attempt as u64,
                f.store.writer,
                2,
                0,
                FrameClass::Small4KiB,
            ),
            &input,
        )
        .unwrap();
    let mut out = String::from("p-");
    use core::fmt::Write as _;
    for byte in token {
        write!(&mut out, "{byte:02x}").unwrap();
    }
    entry(&out)
}

#[test]
fn packed_roots_corrupt_partial_and_context_swapped_candidates_keep_valid_fallback() {
    for variant in 0..7 {
        let mut f = Fixture::new(false);
        let first = publish(&mut f, 2).unwrap();
        let second = publish(&mut f, 2).unwrap();
        let directory = f.store.database_directory;
        let filename = slot_name(&f, 0);
        let file = f.fs.open_existing(&directory, &filename).unwrap();
        let mut bytes = vec![0; FILE_BYTES as usize];
        read_exact_at(&mut f.fs, &file, 0, &mut bytes).unwrap();
        match variant {
            0 => bytes[100] ^= 1,
            1 => bytes.truncate(100),
            2..=4 => {
                let mut c = first.manifest().context();
                let mut claims = first.manifest().claims();
                if variant == 2 {
                    claims.certificate_digest[0] ^= 1;
                }
                if variant == 3 {
                    claims.generation = 2;
                }
                if variant == 4 {
                    c.scope = NamespaceRef::new(
                        c.scope.database(),
                        uste_types::NamespaceId::from_bytes([9; 16]),
                    );
                }
                let encoded = seal_manifest(
                    &mut f.store.vault,
                    c,
                    claims,
                    &[PackedRootFamily {
                        family: 1,
                        root: None,
                        commitment: ordered_commitment::empty_commitment(
                            CommitmentContext::new(c.scope, c.profile, 1).unwrap(),
                        ),
                    }],
                )
                .unwrap();
                bytes[16..].copy_from_slice(&encoded);
            }
            5 => bytes[..16].fill(0),
            _ => {
                let other = f.fs.open_existing(&directory, &slot_name(&f, 1)).unwrap();
                read_exact_at(&mut f.fs, &other, 0, &mut bytes).unwrap();
                assert_eq!(second.manifest().claims().generation, 2);
            }
        }
        write_all_at(&mut f.fs, &file, 0, &bytes).unwrap();
        f.fs.set_len(&file, bytes.len() as u64).unwrap();
        f.fs.sync_all(&file).unwrap();
        let mut f = reopen(f);
        let proof = f.proof(2).unwrap();
        let (candidates, _) = discover(&mut f, &proof, limits(2)).unwrap();
        assert_eq!(candidates.len(), 1, "variant {variant}");
        assert_eq!(candidates[0].manifest().claims().generation, 2);
        assert_eq!(publish(&mut f, 2).err(), Some(StorageError::ResourceLimit));
        assert_eq!(discover(&mut f, &proof, limits(2)).unwrap().0.len(), 1);
    }
}

#[test]
fn packed_roots_maximum_attempt_geometry_and_zero_remaining_read_budget() {
    let mut f = Fixture::new(false);
    for generation in 1..=64 {
        assert_eq!(
            publish(&mut f, 64).unwrap().manifest().claims().generation,
            generation
        );
    }
    assert_eq!(publish(&mut f, 64).err(), Some(StorageError::ResourceLimit));
    let proof = f.proof(2).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    let (roots, report) = discover(&mut f, &proof, limits(64)).unwrap();
    assert_eq!(roots.len(), 64);
    assert_eq!(report.slots, 64);
    assert_eq!(report.manifest_files, 64);
    assert_eq!(report.encoded_bytes, 267_328);
    f.fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        discover(
            &mut f,
            &proof,
            PackedRootDiscoveryLimits::new(1, FILE_BYTES - 1).unwrap()
        )
        .err(),
        Some(StorageError::ResourceLimit)
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 1);
}
