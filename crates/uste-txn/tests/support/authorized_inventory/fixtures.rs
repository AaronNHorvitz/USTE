use super::*;

pub(super) type Fs = FaultFileSystem<MemoryFileSystem>;
pub(super) type Disk =
    DiskCommitCoordinator<PolicyCounter, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PolicyCounter {
    value: CounterState,
    revision: Option<CommitRevision>,
    policy: Option<NamespacePolicy>,
}

impl TransactionState for PolicyCounter {
    type Prepared = Self;
    type Snapshot = Self;
    fn prepare(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self, ApplyError> {
        let mut next = self.clone();
        if request == b"policy" {
            if self.policy.is_some() {
                return Err(ApplyError::Conflict);
            }
            next.policy = Some(policy(1));
        } else if request == b"revoke" || request == b"drift" {
            // `drift` deliberately violates the test reducer's policy-change declaration;
            // the facade must report a known commit and fail closed, never claim rollback.
            next.policy = Some(policy(2));
        } else {
            next.value = CounterState(self.value.prepare(request, inventory, revision)?);
        }
        next.revision = Some(revision);
        Ok(next)
    }
    fn result_digest(prepared: &Self) -> [u8; 32] {
        Self::logical_state_digest(prepared).unwrap()
    }
    fn publish(&mut self, prepared: Self) {
        *self = prepared;
    }
    fn snapshot(&self) -> Self {
        self.clone()
    }
}
impl CheckpointState for PolicyCounter {
    const REDUCER_PROFILE: [u8; 32] = [0xA3; 32];
    fn checkpoint_scope(_: &Self) -> NamespaceRef {
        scope()
    }
    fn checkpoint_revision(snapshot: &Self) -> Option<CommitRevision> {
        snapshot.revision
    }
    fn logical_state_digest(snapshot: &Self) -> Result<[u8; 32], CheckpointStateError> {
        Ok(Sha256::digest(Self::encode_checkpoint(snapshot)?).into())
    }
    fn encode_checkpoint(snapshot: &Self) -> Result<Vec<u8>, CheckpointStateError> {
        let mut bytes = snapshot.value.0.to_be_bytes().to_vec();
        bytes.extend_from_slice(
            &snapshot
                .revision
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &snapshot
                .policy
                .as_ref()
                .map_or(0, |p| p.version().get())
                .to_be_bytes(),
        );
        Ok(bytes)
    }
    fn decode_checkpoint(
        expected_scope: NamespaceRef,
        revision: CommitRevision,
        bytes: &[u8],
    ) -> Result<Self, CheckpointStateError> {
        if expected_scope != scope()
            || bytes.len() != 24
            || u64::from_be_bytes(bytes[8..16].try_into().unwrap()) != revision.get()
        {
            return Err(CheckpointStateError::Invalid);
        }
        let version = u64::from_be_bytes(bytes[16..].try_into().unwrap());
        if !(1..=2).contains(&version) {
            return Err(CheckpointStateError::Invalid);
        }
        Ok(Self {
            value: CounterState(i64::from_be_bytes(bytes[..8].try_into().unwrap())),
            revision: Some(revision),
            policy: Some(policy(version)),
        })
    }
}
impl DiskCoordinatorState for PolicyCounter {
    fn validate_metadata_base(
        &self,
        root: &uste_storage::RecoveredIndexRoot,
    ) -> Result<(), ApplyError> {
        if root.scope() != scope()
            || Some(root.revision()) != self.revision
            || *root.reducer_profile() != Self::REDUCER_PROFILE
            || *root.logical_state_digest() != Self::logical_state_digest(self).unwrap()
        {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
    fn metadata_publication_input(
        &self,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<IndexRootInput, ApplyError> {
        if Some(anchor.0) != self.revision {
            return Err(ApplyError::Conflict);
        }
        Ok(IndexRootInput {
            scope: scope(),
            revision: anchor.0,
            certificate_digest: anchor.1,
            reducer_profile: Self::REDUCER_PROFILE,
            logical_state_digest: Self::logical_state_digest(self).unwrap(),
            index_profile: uste_txn::COORDINATOR_METADATA_PROFILE_V1,
        })
    }
}
impl AuthorizedDiskPolicyState for PolicyCounter {
    fn current_durable_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        self.policy.as_ref().ok_or(ApplyError::Conflict)
    }
}
impl AuthorizedTransactionState for PolicyCounter {
    const REQUIRES_DURABLE_POLICY: bool = true;
    fn authorization_requirements(
        request: &[u8],
        _: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        if request.first() == Some(&0xFF) || request.first() == Some(&0xFE) {
            let database = if request[0] == 0xFE {
                DatabaseId::from_bytes([0xFF; 16])
            } else {
                scope().database()
            };
            AuthorizationRequirements::new([AuthorizationRequirement {
                action: Action::ReadRecord,
                target: Target::Record(uste_types::RecordRef::new(
                    database,
                    scope().namespace(),
                    uste_types::RecordId::from_bytes([9; 16]),
                )),
            }])
            .map_err(|_| ApplyError::ResourceLimit)
        } else {
            Ok(AuthorizationRequirements::default())
        }
    }
    fn durable_namespace_policy(snapshot: &Self) -> Option<NamespacePolicy> {
        snapshot.policy.clone()
    }
    fn durable_policy_change(request: &[u8]) -> Result<Option<DurablePolicyChange>, ApplyError> {
        Ok(
            (request == b"policy" || request == b"revoke").then(|| DurablePolicyChange {
                expected: PolicyVersion::new(1).unwrap(),
                next: policy(2),
            }),
        )
    }
}

pub(super) fn policy(version: u64) -> NamespacePolicy {
    let quota = QuotaLimits::new(16, 8, 3, 2, 8).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(version).unwrap(), quota);
    for principal in [1, 2] {
        let mut actions = vec![
            Action::Commit,
            Action::ReadBlob,
            Action::ReadOwnOutcome,
            Action::InspectQuota,
            Action::StartUpload,
            Action::ResumeUpload,
            Action::WriteUpload,
            Action::FinishUpload,
            Action::AbortUpload,
            Action::ManageSchema,
            Action::ManagePolicy,
        ];
        if version == 2 && principal == 1 {
            actions.retain(|action| *action != Action::Commit);
        }
        policy
            .grant(
                PrincipalDigest::from_bytes([principal; 32]),
                NamespaceGrant::new(PermissionSet::from_actions(actions), quota),
            )
            .unwrap();
    }
    policy
}
struct AuthAdapter;
impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = u8;
    fn authenticate(&mut self, credential: &u8) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*credential; 32]))
    }
}
pub(super) fn authenticated(kernel: &PolicyKernel, principal: u8) -> AuthenticatedPrincipal {
    kernel.authenticate(&mut AuthAdapter, &principal).unwrap()
}
pub(super) fn lookup() -> IndexGetLimits {
    IndexGetLimits::new(64, 136).unwrap()
}
pub(super) fn cache() -> PageCache {
    PageCache::new(64 * 1024).unwrap()
}
pub(super) fn run() -> IndexRunReadLimits {
    IndexRunReadLimits::new(32, 32, 8192).unwrap()
}
pub(super) fn accounting() -> DiskBlobAccountingLimits {
    DiskBlobAccountingLimits {
        base: run(),
        maximum_total_owners: 4,
    }
}
pub(super) fn append_limits() -> DiskBlobAppendLimits {
    DiskBlobAppendLimits {
        maximum_pending_blobs: 4,
        maximum_pending_inventories: 4,
        maximum_pending_namespaces: 1,
        maximum_inventory_references: 4,
        maximum_verified_blob_bytes: 8,
        lookup: lookup(),
    }
}

pub(super) fn reopen(fs: &mut Fs, name: &EntryName, base_state: PolicyCounter) -> Disk {
    static SEED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(103_000_000);
    let seed = SEED.fetch_add(100_000, std::sync::atomic::Ordering::Relaxed);
    let limits = BlobRecoveryLimits {
        catalog: BlobMetadataRebuildLimits {
            admission: BlobMetadataAdmissionLimits {
                maximum_blobs: 4,
                maximum_namespaces: 1,
                maximum_inventories: 4,
                maximum_reference_bindings: 16,
                run: run(),
                lookup: lookup(),
                certificates: CertificateAnchorReadLimits::new(8, 8 * 4161).unwrap(),
                maximum_journal_groups: 8,
                maximum_journal_encoded_bytes: 1_000_000,
            },
            merge: IndexRunMergeLimits::new(run(), 32, 8192, 32, 8192).unwrap(),
            maximum_merge_output_bytes: 16_384,
        },
        catalog_recovery: BlobCatalogRecovery::AdmitOrRebuild,
        maximum_verified_blob_bytes_per_pass: 64,
        maximum_uncommitted_segment_tails: 8,
    };
    let (recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        fs,
        name,
        scope(),
        CounterEntropy(seed),
        CounterEntropy(seed + 50_000),
        &mut TestKeyAdapter,
        limits,
        &mut cache(),
    )
    .unwrap();
    let mut cache = cache();
    let root = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.revision() == CommitRevision::FIRST)
        .unwrap();
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        fs,
        root,
        CoordinatorTransactionAdmissionLimits {
            run: run(),
            lookup: lookup(),
            maximum_groups: 1,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let root =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<PolicyCounter, _, _, _, _>(
            &recovery, fs,
        )
        .unwrap()
        .into_iter()
        .find(|root| root.revision() == CommitRevision::FIRST)
        .unwrap();
    let base = uste_txn::admit_coordinator_disk_base(
        &recovery,
        fs,
        root,
        transactions,
        CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(1, 0, 2, 32, 8192).unwrap(),
            lookup: lookup(),
            maximum_total_journal_groups: 1,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    DiskCommitCoordinator::recover_from_admitted_base(
        recovery,
        fs,
        base,
        base_state,
        RetentionDays::new(30).unwrap(),
        DiskCoordinatorRecoveryLimits {
            overlay: CoordinatorRecoveryLimits::new(8, 4).unwrap(),
            lookup: lookup(),
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap()
}

#[allow(clippy::type_complexity)]
pub(super) fn fixture() -> (
    Fs,
    EntryName,
    Disk,
    PolicyCounter,
    PolicyKernel,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
) {
    let name = EntryName::new("authorized-disk-inventory").unwrap();
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 1030),
        CounterEntropy(1031),
        PolicyCounter::default(),
    )
    .unwrap();
    coordinator
        .commit(
            &mut fs,
            request(1, 1, b"policy"),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    uste_txn::publish_coordinator_metadata_root(&mut coordinator, &mut fs).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut fs).unwrap();
    let base = coordinator.read_view().unwrap().state().clone();
    drop(coordinator);
    fs.restart().unwrap();
    let disk = reopen(&mut fs, &name, base.clone());
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy(1)).unwrap();
    let alice = authenticated(&kernel, 1);
    let bob = authenticated(&kernel, 2);
    (fs, name, disk, base, kernel, alice, bob)
}
