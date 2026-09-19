use super::*;
use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
use uste_storage::journal::StorageError;
use uste_storage::{BlobReference, FileSystem, IndexGetLimits, IndexRunReadLimits, PageCache};
use uste_txn::{
    AuthenticatedIndexRecovery, CoordinatorDiskBase, DiskRecoveryDomain,
    PrimaryMetadataRecoveryLimits, PrimaryMetadataRecoveryReport, RecoveredFrontierTransaction,
};

type Fs = FaultFileSystem<MemoryFileSystem>;
type Recovery = AuthenticatedIndexRecovery<Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
type Disk =
    uste_txn::DiskCommitCoordinator<Anchored, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;

#[derive(Clone)]
struct Anchored {
    state: CounterState,
    certificate: [u8; 32],
}
impl TransactionState for Anchored {
    type Prepared = CounterState;
    type Snapshot = CounterState;
    fn prepare(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<CounterState, ApplyError> {
        self.state.prepare(request, inventory, revision)
    }
    fn result_digest(prepared: &CounterState) -> [u8; 32] {
        CounterState::result_digest(prepared)
    }
    fn publish(&mut self, prepared: CounterState) {
        self.state = prepared;
    }
    fn snapshot(&self) -> CounterState {
        self.state.clone()
    }
}
impl uste_txn::ExternallyPreparedTransactionState for Anchored {
    fn validate_external_prepared(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &CounterState,
    ) -> Result<(), ApplyError> {
        self.state
            .validate_external_prepared(request, inventory, revision, prepared)
    }
}
impl uste_txn::JournalAnchoredTransactionState for Anchored {
    fn journal_base_anchor(&self) -> Result<(NamespaceRef, CommitRevision, [u8; 32]), ApplyError> {
        Ok((
            self.state.scope,
            self.state.revision.ok_or(ApplyError::Conflict)?,
            self.certificate,
        ))
    }
}
impl uste_txn::DiskCoordinatorState for Anchored {
    fn metadata_publication_input(
        &self,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<IndexRootInput, ApplyError> {
        if anchor.1 != self.certificate {
            return Err(ApplyError::Conflict);
        }
        self.state.metadata_publication_input(anchor)
    }
    fn validate_metadata_base(
        &self,
        root: &uste_storage::RecoveredIndexRoot,
    ) -> Result<(), ApplyError> {
        if root.certificate_digest() != &self.certificate {
            return Err(ApplyError::Conflict);
        }
        self.state.validate_metadata_base(root)
    }
}
#[derive(Default)]
struct Domain {
    advanced: u64,
    finished: bool,
}
impl DiskRecoveryDomain<Anchored, Fs, TestEnvelope, CounterEntropy, CounterEntropy> for Domain {
    fn admit(&mut self, revisions: u64) -> Result<(), StorageError> {
        if revisions > 3 {
            return Err(StorageError::ResourceLimit);
        }
        Ok(())
    }
    fn prepare(
        &mut self,
        _: &Recovery,
        _: &mut Fs,
        state: &Anchored,
        transaction: &RecoveredFrontierTransaction,
        _: &mut PageCache,
    ) -> Result<CounterState, StorageError> {
        state
            .prepare(
                transaction.canonical_request(),
                transaction.blob_inventory(),
                transaction.revision(),
            )
            .map_err(|_| StorageError::IntegrityFailure)
    }
    fn advance(
        &mut self,
        _: &mut Recovery,
        _: &mut Fs,
        state: &mut Anchored,
        transaction: &RecoveredFrontierTransaction,
        _: &mut PageCache,
    ) -> Result<(), StorageError> {
        state.certificate = *transaction.certificate_digest();
        self.advanced += 1;
        Ok(())
    }
    fn finish(
        &mut self,
        _: &mut Recovery,
        _: &mut Fs,
        _: &mut Anchored,
        _: (CommitRevision, [u8; 32]),
        _: &mut PageCache,
    ) -> Result<(), StorageError> {
        self.finished = true;
        Ok(())
    }
}
fn lookup() -> IndexGetLimits {
    IndexGetLimits::new(32, 136).unwrap()
}
fn cache() -> PageCache {
    PageCache::new(64 * 1024).unwrap()
}
fn limits() -> PrimaryMetadataRecoveryLimits {
    PrimaryMetadataRecoveryLimits {
        maximum_revisions: 3,
        maximum_blob_owners: 2,
        maximum_inventory_references: 2,
        merge: metadata_rebase_limits().merge,
    }
}
fn fixture() -> (Fs, EntryName, Anchored, [BlobReference; 2]) {
    fixture_mode(true)
}
fn fixture_mode(initial_owner: bool) -> (Fs, EntryName, Anchored, [BlobReference; 2]) {
    let name = EntryName::new("streamed-primary-owners").unwrap();
    let mut fs = Fs::new(MemoryFileSystem::default(), FaultPlan::default());
    let vault = KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(7_000_000),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(7_010_000),
        CounterState::new(scope()),
    )
    .unwrap();
    let mut references = Vec::new();
    for content in [b"first".as_slice(), b"second".as_slice()] {
        let mut upload = coordinator.start_blob_upload(scope()).unwrap();
        coordinator
            .write_blob_upload(&mut fs, &mut upload, content)
            .unwrap();
        references.push(
            coordinator
                .finish_blob_upload(&mut fs, &mut upload)
                .unwrap(),
        );
    }
    let mut base = None;
    for revision in 1..=4 {
        let inventory = BlobInventory::new(
            scope(),
            if revision == 1 {
                references[..1].to_vec()
            } else if revision == 4 {
                vec![]
            } else {
                references.clone()
            },
        )
        .unwrap();
        coordinator
            .commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: (revision != 4 && (revision != 1 || initial_owner))
                        .then_some(&inventory),
                    ..request(revision, &(revision as u64).to_be_bytes())
                },
                &mut TestClock(20),
                &NeverCancel,
            )
            .unwrap();
        if revision == 1 {
            publish_coordinator_metadata_root(&mut coordinator, &mut fs).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut fs).unwrap();
            base = Some(Anchored {
                state: coordinator.read_view().unwrap().state().clone(),
                certificate: coordinator.checkpoint_anchor().unwrap().unwrap().1,
            });
        }
    }
    drop(coordinator);
    fs.restart().unwrap();
    (fs, name, base.unwrap(), references.try_into().unwrap())
}
fn admit(
    fs: &mut Fs,
    name: &EntryName,
    revision: CommitRevision,
) -> (Recovery, CoordinatorDiskBase) {
    admit_mode(fs, name, revision, false)
}
fn admit_mode(
    fs: &mut Fs,
    name: &EntryName,
    revision: CommitRevision,
    first: bool,
) -> (Recovery, CoordinatorDiskBase) {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(8_000_000);
    let entropy = ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let (recovery, _, _) = Recovery::open_with_disk_blob_metadata(
        fs,
        name,
        scope(),
        CounterEntropy::new(entropy),
        CounterEntropy::new(entropy + 5_000),
        &mut TestKeyAdapter,
        disk_inventory_writes::storage_limits(),
        &mut cache(),
    )
    .unwrap();
    let root = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|r| r.revision() == revision)
        .unwrap();
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        fs,
        root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(32, 32, 8192).unwrap(),
            lookup: lookup(),
            maximum_groups: revision.get(),
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache(),
    )
    .unwrap();
    let candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<CounterState, _, _, _, _>(
            &recovery, fs,
        )
        .unwrap()
        .into_iter()
        .find(|r| r.revision() == revision)
        .unwrap();
    let admission = uste_txn::CoordinatorDiskAdmissionLimits {
        metadata: CoordinatorMetadataLoadLimits::new(
            revision.get(),
            2,
            revision.get() + 3,
            32,
            8192,
        )
        .unwrap(),
        lookup: lookup(),
        maximum_total_journal_groups: revision.get() * 3,
        maximum_encoded_bytes_per_pass: 1_000_000,
    };
    let base = if first {
        let root = recovery
            .load_index_root_manifests(fs, uste_txn::COORDINATOR_FIRST_REFERENCE_PROFILE_V1)
            .unwrap()
            .into_iter()
            .find(|r| r.revision() == revision)
            .unwrap();
        uste_txn::admit_coordinator_disk_base_with_first_references(
            &recovery,
            fs,
            candidate,
            transactions,
            root,
            IndexRunReadLimits::new(32, 32, 8192).unwrap(),
            admission,
            &mut cache(),
        )
    } else {
        uste_txn::admit_coordinator_disk_base(
            &recovery,
            fs,
            candidate,
            transactions,
            admission,
            &mut cache(),
        )
    }
    .unwrap();
    (recovery, base)
}
fn recover(
    fs: &mut Fs,
    recovery: Recovery,
    base: CoordinatorDiskBase,
    state: Anchored,
    limits: PrimaryMetadataRecoveryLimits,
    domain: &mut Domain,
) -> Result<(Disk, PrimaryMetadataRecoveryReport), TransactionError> {
    Disk::recover_with_primary_metadata_streaming_domain(
        recovery,
        fs,
        base,
        state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            lookup: lookup(),
            maximum_encoded_bytes: 1_000_000,
        },
        limits,
        &mut cache(),
        domain,
    )
}
fn assert_terminal(disk: &Disk, fs: &mut Fs, references: [BlobReference; 2]) {
    assert_terminal_owners(disk, fs, references, [1, 2]);
}
fn assert_terminal_owners(
    disk: &Disk,
    fs: &mut Fs,
    references: [BlobReference; 2],
    principals: [u8; 2],
) {
    assert_eq!(disk.state().unwrap().state.value, 10);
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.certificate_anchor_residency(), (false, 0));
    assert_eq!(disk.blob_metadata_residency(), (false, 0, 0, 0));
    for (reference, principal) in references.into_iter().zip(principals) {
        assert_eq!(
            disk.committed_blob_owner(fs, reference, lookup(), &mut cache())
                .unwrap(),
            Some(PrincipalDigest::from_bytes([principal; 32]))
        );
    }
    for revision in 1..=4 {
        let expected = disk
            .outcome(
                fs,
                PrincipalDigest::from_bytes([revision; 32]),
                IdempotencyKey::from_bytes([revision; 16]),
                UtcInstant::new(20, 0).unwrap(),
                lookup(),
                &mut cache(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(expected.revision.get(), u64::from(revision));
        assert_eq!(
            disk.transaction_outcome(
                fs,
                PrincipalDigest::from_bytes([revision; 32]),
                TransactionId::from_bytes([revision + 32; 16]),
                UtcInstant::new(20, 0).unwrap(),
                lookup(),
                &mut cache()
            )
            .unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn primary_streaming_preserves_first_owners_without_suffix_maps_and_reopens() {
    let (mut fs, name, state, references) = fixture();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    let mut domain = Domain::default();
    let (mut disk, report) =
        recover(&mut fs, recovery, base, state, limits(), &mut domain).unwrap();
    assert_eq!((domain.advanced, domain.finished), (3, true));
    assert_eq!(report.revisions, 3);
    assert_eq!(report.staged_runs, 12);
    assert_eq!(report.output_entries, 27);
    assert_eq!(report.output_logical_bytes, 3513);
    assert_terminal(&disk, &mut fs, references);
    assert!(disk.rebase_required());
    disk.rebase_metadata(&mut fs, metadata_rebase_limits())
        .unwrap();
    assert!(!disk.rebase_required());
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::new(4).unwrap());
    let (disk, report) = recover(
        &mut fs,
        recovery,
        base,
        state,
        limits(),
        &mut Domain::default(),
    )
    .unwrap();
    assert_eq!(report, PrimaryMetadataRecoveryReport::default());
    assert_terminal(&disk, &mut fs, references);
}

#[test]
fn primary_streaming_adds_the_first_owner_family_and_preserves_it_without_inventory() {
    let (mut fs, name, state, references) = fixture_mode(false);
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    let (mut disk, report) = recover(
        &mut fs,
        recovery,
        base,
        state,
        limits(),
        &mut Domain::default(),
    )
    .unwrap();
    assert_eq!(report.staged_runs, 12);
    assert_terminal_owners(&disk, &mut fs, references, [2, 2]);
    disk.rebase_metadata(&mut fs, metadata_rebase_limits())
        .unwrap();
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::new(4).unwrap());
    let (disk, _) = recover(
        &mut fs,
        recovery,
        base,
        state,
        limits(),
        &mut Domain::default(),
    )
    .unwrap();
    assert_terminal_owners(&disk, &mut fs, references, [2, 2]);
}

#[test]
fn primary_streaming_late_certificate_corruption_discards_private_owner_stages() {
    let (mut fs, name, state, references) = fixture();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let file = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 4 * 4161 + 100;
    let mut byte = [0];
    assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    let mut domain = Domain::default();
    assert!(
        recover(
            &mut fs,
            recovery,
            base,
            state.clone(),
            limits(),
            &mut domain
        )
        .is_err()
    );
    assert!(!domain.finished);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    fs.restart().unwrap();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    for profile in [
        COORDINATOR_METADATA_PROFILE_V1,
        uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
    ] {
        assert!(
            recovery
                .load_index_root_manifests(&mut fs, profile)
                .unwrap()
                .iter()
                .all(|root| root.revision() == CommitRevision::FIRST)
        );
    }
    let (disk, _) = recover(
        &mut fs,
        recovery,
        base,
        state,
        limits(),
        &mut Domain::default(),
    )
    .unwrap();
    assert_terminal(&disk, &mut fs, references);
}

#[test]
fn primary_streaming_owner_bounds_precede_domain_advancement() {
    for (owners, references) in [(0, 2), (1, 2), (2, 1)] {
        let (mut fs, name, state, _) = fixture();
        let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
        fs.arm(FaultPlan::default()).unwrap();
        let mut domain = Domain::default();
        let bounded = PrimaryMetadataRecoveryLimits {
            maximum_blob_owners: owners,
            maximum_inventory_references: references,
            ..limits()
        };
        assert!(matches!(
            recover(&mut fs, recovery, base, state, bounded, &mut domain),
            Err(TransactionError::ResourceLimit)
        ));
        assert_eq!((domain.advanced, domain.finished), (0, false));
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        if owners == 0 {
            assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        }
    }
}

#[test]
fn primary_streaming_refuses_attached_first_reference_projection_before_io() {
    let (mut fs, name, disk, state, _) = first_reference_rebase::fixture(true);
    drop(disk);
    fs.restart().unwrap();
    let (recovery, base) = admit_mode(&mut fs, &name, CommitRevision::FIRST, true);
    let roots = recovery
        .load_index_root_manifests(&mut fs, COORDINATOR_METADATA_PROFILE_V1)
        .unwrap();
    let certificate = *roots
        .iter()
        .find(|r| r.revision() == CommitRevision::FIRST)
        .unwrap()
        .certificate_digest();
    fs.arm(FaultPlan::default()).unwrap();
    let mut domain = Domain::default();
    assert!(matches!(
        recover(
            &mut fs,
            recovery,
            base,
            Anchored { state, certificate },
            limits(),
            &mut domain
        ),
        Err(TransactionError::InvalidRequest)
    ));
    assert_eq!((domain.advanced, domain.finished), (0, false));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn primary_streaming_owner_faults_never_publish_partial_metadata() {
    let (mut fs, name, state, _) = fixture();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    fs.arm(FaultPlan::default()).unwrap();
    drop(
        recover(
            &mut fs,
            recovery,
            base,
            state,
            limits(),
            &mut Domain::default(),
        )
        .unwrap(),
    );
    let mut attempts = 0;
    let mut optional_no_crash = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::RenameNoReplace,
        Operation::RemoveFile,
        Operation::SyncData,
    ] {
        for occurrence in 1..=fs.operation_count(operation) {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut trial, name, state, references) = fixture();
                let (recovery, base) = admit(&mut trial, &name, CommitRevision::FIRST);
                trial
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                let mut domain = Domain::default();
                let result = recover(
                    &mut trial,
                    recovery,
                    base,
                    state.clone(),
                    limits(),
                    &mut domain,
                );
                if action == FaultAction::CrashAfter && !trial.is_crashed() {
                    assert!(matches!(
                        operation,
                        Operation::OpenExisting | Operation::RemoveFile
                    ));
                    assert_terminal(&result.unwrap().0, &mut trial, references);
                    optional_no_crash += 1;
                } else {
                    assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                }
                assert_eq!(trial.pending_faults(), 0);
                trial.restart().unwrap();
                let (recovery, base) = admit(&mut trial, &name, CommitRevision::FIRST);
                for profile in [
                    COORDINATOR_METADATA_PROFILE_V1,
                    uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                ] {
                    assert!(
                        recovery
                            .load_index_root_manifests(&mut trial, profile)
                            .unwrap()
                            .iter()
                            .all(|root| root.revision() == CommitRevision::FIRST)
                    );
                }
                let (disk, _) = recover(
                    &mut trial,
                    recovery,
                    base,
                    state,
                    limits(),
                    &mut Domain::default(),
                )
                .unwrap();
                assert_terminal(&disk, &mut trial, references);
                attempts += 1;
            }
        }
    }
    eprintln!("primary owner streaming faults={attempts} optional_no_crash={optional_no_crash}");
    assert!(attempts > 0);
}
