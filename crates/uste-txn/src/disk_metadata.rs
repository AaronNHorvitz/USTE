//! Certificate-anchored coordinator retry and blob-owner metadata.
//!
//! These roots are optional recovery caches. The journal remains authoritative, and seeded open
//! independently replays and compares the complete prefix metadata at the named certificate.

use std::collections::BTreeMap;

mod admission;
mod packed;
pub use packed::{
    COORDINATOR_PACKED_PROFILE_V1, PackedCoordinatorAdmissionLimits,
    PackedCoordinatorAdmissionReport, PackedCoordinatorLimits, PackedCoordinatorPrefix,
    PackedCoordinatorReport, admit_packed_coordinator_prefix, stage_packed_coordinator_prefix,
};
mod first_reference;
mod genesis;
mod rebase;
mod recovery_step;
mod usage;
pub use admission::{
    CoordinatorDiskAdmissionLimits, CoordinatorDiskBase, admit_coordinator_disk_base,
    admit_coordinator_disk_base_with_first_references,
};
pub use first_reference::{
    COORDINATOR_FIRST_REFERENCE_PROFILE_V1, CoordinatorFirstReferenceLimits,
    publish_coordinator_first_reference_index,
};
pub use genesis::{
    stage_genesis_first_references, stage_inventory_free_genesis_metadata,
    stage_primary_genesis_metadata,
};
pub use rebase::CoordinatorMetadataRebaseLimits;
pub(crate) use rebase::publish_overlay_base;
pub(crate) use recovery_step::stage_primary_metadata_step;
pub use usage::{
    COORDINATOR_BLOB_USAGE_PROFILE_V1, CoordinatorBlobUsageLimits,
    CoordinatorBlobUsageRebuildLimits, CoordinatorBlobUsageRebuildReport,
    MAX_BLOB_USAGE_REBUILD_BATCH_OWNERS, stage_genesis_blob_usage,
};
pub(crate) use usage::{bootstrap_empty_usage, rebuild_usage};

use uste_crypto::EntropySource;
use uste_storage::{
    BlobId, BlobReference, DurableIndexRoot, IndexEntry, IndexRootAnchor, IndexRootInput,
    IndexRunReadLimits, IndexRunReadReport, MAX_COMMITTED_BLOBS_PER_JOURNAL,
    MAX_INDEX_PAGES_PER_RUN, MAX_INDEX_RUN_LOGICAL_BYTES, OwnershipFileSystem, RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, StorageError},
};
use uste_types::{CommitRevision, IdempotencyKey, TransactionId, UtcInstant};

use super::{
    AuthenticatedIndexRecovery, CheckpointState, CheckpointStateError, CommitCoordinator,
    CoordinatorRecoverySeed, MAX_OUTCOMES_PER_NAMESPACE, PrincipalDigest, RetryKey,
    TransactionError, TransactionOutcome, TransactionState,
};

pub const COORDINATOR_METADATA_PROFILE_V1: [u8; 32] = [
    0x2f, 0x97, 0xf5, 0xf5, 0x23, 0x87, 0xec, 0x06, 0x46, 0x07, 0x70, 0x64, 0xc3, 0x19, 0x24, 0xab,
    0x17, 0xc1, 0xdd, 0x99, 0xe4, 0x69, 0x57, 0x3f, 0xec, 0x8b, 0xb1, 0x55, 0xe8, 0x59, 0x61, 0x36,
];

const FAMILY_METADATA: u8 = 1;
const FAMILY_OUTCOME: u8 = 2;
const FAMILY_BLOB_OWNER: u8 = 3;
const METADATA_KEY: &[u8] = b"coordinator-meta-v1";
const METADATA_VALUE_BYTES: usize = 48;
const OUTCOME_KEY_BYTES: usize = 48;
const OUTCOME_VALUE_BYTES: usize = 104;
const OWNER_KEY_BYTES: usize = 16;
const OWNER_VALUE_BYTES: usize = 80;
const METADATA_LOGICAL_BYTES: u64 = METADATA_KEY.len() as u64 + METADATA_VALUE_BYTES as u64;
const OUTCOME_LOGICAL_BYTES: u64 = OUTCOME_KEY_BYTES as u64 + OUTCOME_VALUE_BYTES as u64;
const OWNER_LOGICAL_BYTES: u64 = OWNER_KEY_BYTES as u64 + OWNER_VALUE_BYTES as u64;

/// SHA-256 of `USTE coordinator-transaction-v1`; a separate optional index profile.
pub const COORDINATOR_TRANSACTION_PROFILE_V1: [u8; 32] = [
    0xd7, 0x8e, 0x78, 0xaf, 0x45, 0xef, 0xd9, 0xc5, 0xa3, 0xa0, 0x61, 0x23, 0x95, 0x47, 0xcf, 0xfd,
    0x8b, 0xfb, 0xe9, 0xc6, 0xc1, 0x84, 0x62, 0x88, 0x7a, 0x51, 0x3b, 0xee, 0x63, 0x0d, 0x79, 0xce,
];

/// Privileged transaction lookup index validated against coordinator metadata or the journal.
/// This is not a consumer authorization capability or admission of a complete coordinator base.
#[derive(Debug)]
pub struct CoordinatorTransactionIndex {
    root: RecoveredIndexRoot,
}

/// Independent bounds for full-run authentication and journal-to-index correspondence.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorTransactionAdmissionLimits {
    pub run: IndexRunReadLimits,
    pub lookup: uste_storage::IndexGetLimits,
    pub maximum_groups: u64,
    pub maximum_encoded_bytes: u64,
}

/// Admit a transaction-ID root directly against the authenticated journal, without constructing
/// retry or transaction maps. Every journal revision must have exactly one matching index entry;
/// the full run is authenticated first so counts cannot hide extra or missing entries. This
/// validates transaction correspondence only: graph state, retry keys and first owners still
/// require independent admission. Storage's journal anchor/blob maps remain memory-resident.
pub fn admit_coordinator_transaction_index_for_recovery<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    root: RecoveredIndexRoot,
    limits: CoordinatorTransactionAdmissionLimits,
    cache: &mut uste_storage::PageCache,
) -> Result<CoordinatorTransactionIndex, TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if root.scope() != recovery.scope()
        || root.index_profile() != &COORDINATOR_TRANSACTION_PROFILE_V1
        || root.runs().len() != 1
        || root
            .runs()
            .next()
            .is_none_or(|run| run.family() != 1 || run.entry_count() != root.revision().get())
    {
        return Err(TransactionError::IntegrityFailure);
    }
    if root.revision().get() > limits.maximum_groups {
        return Err(TransactionError::ResourceLimit);
    }
    recovery.visit_index_run(filesystem, &root, 1, limits.run, &mut |key, value| {
        if key.len() != 16 || value.len() != 136 {
            return Err(StorageError::IntegrityFailure);
        }
        let mut retry_key = [0; OUTCOME_KEY_BYTES];
        retry_key[..32].copy_from_slice(&value[..32]);
        let (_, _, outcome) = decode_outcome(&retry_key, &value[32..], root.revision())?;
        if outcome.transaction_id.as_bytes() != key {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(())
    })?;
    recovery.visit_transactions_reverse(
        filesystem,
        CommitRevision::FIRST,
        root.revision(),
        limits.maximum_groups,
        limits.maximum_encoded_bytes,
        |filesystem, transaction| {
            if transaction.revision == root.revision()
                && &transaction.certificate_digest != root.certificate_digest()
            {
                return Err(StorageError::IntegrityFailure);
            }
            let expected = encode_outcome(
                transaction.principal,
                transaction.idempotency_key,
                transaction.outcome,
            )?;
            let (value, _) = recovery
                .index_get_bounded(
                    filesystem,
                    &root,
                    1,
                    transaction.outcome.transaction_id.as_bytes(),
                    limits.lookup,
                    cache,
                )
                .map_err(|error| match error {
                    TransactionError::Storage(error) => error,
                    TransactionError::ResourceLimit => StorageError::ResourceLimit,
                    _ => StorageError::IntegrityFailure,
                })?;
            let value = value.ok_or(StorageError::IntegrityFailure)?;
            if value.len() != 136
                || value[..32] != transaction.principal.as_bytes()
                || value[32..] != expected.value
            {
                return Err(StorageError::IntegrityFailure);
            }
            Ok(())
        },
    )?;
    Ok(CoordinatorTransactionIndex { root })
}

impl CoordinatorTransactionIndex {
    pub fn anchor(&self) -> IndexRootAnchor {
        self.root.anchor()
    }

    /// Exact raw lookup with explicit I/O limits. Expiry and principal authorization remain the
    /// responsibility of the trusted coordinator caller. A changed frontier fails before I/O.
    pub fn lookup<S, F, W, E, I>(
        &self,
        coordinator: &CommitCoordinator<S, F, W, E, I>,
        filesystem: &mut F,
        transaction_id: TransactionId,
        limits: uste_storage::IndexGetLimits,
        cache: &mut uste_storage::PageCache,
    ) -> Result<Option<(PrincipalDigest, TransactionOutcome)>, TransactionError>
    where
        S: TransactionState,
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if coordinator.scope != self.root.scope()
            || coordinator.checkpoint_anchor()?
                != Some((self.root.revision(), *self.root.certificate_digest()))
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let (value, _) = coordinator.index_get_bounded(
            filesystem,
            &self.root,
            1,
            transaction_id.as_bytes(),
            limits,
            cache,
        )?;
        value
            .map(|value| {
                if value.len() != 136 {
                    return Err(TransactionError::IntegrityFailure);
                }
                let mut retry_key = [0_u8; OUTCOME_KEY_BYTES];
                retry_key[..32].copy_from_slice(&value[..32]);
                let (principal, _, outcome) =
                    decode_outcome(&retry_key, &value[32..], self.root.revision())
                        .map_err(TransactionError::Storage)?;
                if outcome.transaction_id != transaction_id {
                    return Err(TransactionError::IntegrityFailure);
                }
                Ok((principal, outcome))
            })
            .transpose()
    }
}

/// Stream the transaction-ID ordering into a separate certificate-paired immutable index.
pub fn publish_coordinator_transaction_index<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
) -> Result<CoordinatorTransactionIndex, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let (revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(TransactionError::InvalidRequest)?;
    if coordinator.state.current_checkpoint_scope() != coordinator.scope
        || coordinator.state.current_checkpoint_revision() != Some(revision)
        || coordinator.outcomes.is_empty()
        || coordinator.outcomes.len() != coordinator.transactions.len()
        || coordinator.outcomes.iter().any(|(key, outcome)| {
            outcome.revision > revision
                || coordinator.transactions.get(&outcome.transaction_id)
                    != Some(&(key.principal, *outcome))
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let logical_state_digest = coordinator
        .state
        .current_logical_state_digest()
        .map_err(checkpoint_error)?;
    let entries = coordinator
        .transactions
        .iter()
        .map(|(id, (principal, outcome))| {
            let encoded =
                encode_outcome(*principal, IdempotencyKey::from_bytes([0; 16]), *outcome)?;
            let mut value = Vec::with_capacity(136);
            value.extend_from_slice(&principal.as_bytes());
            value.extend_from_slice(&encoded.value);
            Ok(IndexEntry {
                key: id.as_bytes().to_vec(),
                value,
            })
        });
    let run = coordinator
        .journal
        .publish_index_run_fallible(
            filesystem,
            coordinator.scope,
            revision,
            COORDINATOR_TRANSACTION_PROFILE_V1,
            1,
            entries,
        )
        .map_err(TransactionError::Storage)?;
    let root = coordinator
        .journal
        .publish_index_root_recovered(
            filesystem,
            IndexRootInput {
                scope: coordinator.scope,
                revision,
                certificate_digest,
                reducer_profile: S::REDUCER_PROFILE,
                logical_state_digest,
                index_profile: COORDINATOR_TRANSACTION_PROFILE_V1,
            },
            &[run],
        )
        .map_err(TransactionError::Storage)?;
    Ok(CoordinatorTransactionIndex { root })
}

/// Re-admit current transaction indexes against independently recovered coordinator metadata.
/// The run scan is bounded and retains one entry, but the comparator is still memory-resident.
pub fn load_coordinator_transaction_indexes<S, F, W, E, I>(
    coordinator: &CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    limits: IndexRunReadLimits,
) -> Result<Vec<CoordinatorTransactionIndex>, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let anchor = coordinator
        .checkpoint_anchor()?
        .ok_or(TransactionError::InvalidRequest)?;
    let digest = coordinator
        .state
        .current_logical_state_digest()
        .map_err(checkpoint_error)?;
    let mut admitted = Vec::new();
    for root in
        coordinator.load_index_root_manifests(filesystem, COORDINATOR_TRANSACTION_PROFILE_V1)?
    {
        if (root.revision(), *root.certificate_digest()) != anchor
            || root.reducer_profile() != &S::REDUCER_PROFILE
            || root.logical_state_digest() != &digest
        {
            continue;
        }
        if root.runs().len() != 1
            || root.runs().next().is_none_or(|run| {
                run.family() != 1 || run.entry_count() != coordinator.transactions.len() as u64
            })
        {
            return Err(TransactionError::IntegrityFailure);
        }
        coordinator.visit_index_run(filesystem, &root, 1, limits, &mut |key, value| {
            if key.len() != 16 || value.len() != 136 {
                return Err(StorageError::IntegrityFailure);
            }
            let id = TransactionId::from_bytes(read_array(key)?);
            let mut retry_key = [0; OUTCOME_KEY_BYTES];
            retry_key[..32].copy_from_slice(&value[..32]);
            let (principal, _, outcome) =
                decode_outcome(&retry_key, &value[32..], root.revision())?;
            if outcome.transaction_id != id
                || coordinator.transactions.get(&id) != Some(&(principal, outcome))
            {
                return Err(StorageError::IntegrityFailure);
            }
            Ok(())
        })?;
        admitted.push(CoordinatorTransactionIndex { root });
    }
    Ok(admitted)
}

pub struct CoordinatorMetadataCandidate {
    root: RecoveredIndexRoot,
}

impl core::fmt::Debug for CoordinatorMetadataCandidate {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CoordinatorMetadataCandidate")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl CoordinatorMetadataCandidate {
    #[must_use]
    pub const fn anchor(&self) -> IndexRootAnchor {
        self.root.anchor()
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoordinatorMetadataLoadLimits {
    maximum_outcomes: u64,
    maximum_blob_owners: u64,
    maximum_total_entries: u64,
    maximum_total_pages: u64,
    maximum_logical_bytes: u64,
}

impl CoordinatorMetadataLoadLimits {
    pub const fn new(
        maximum_outcomes: u64,
        maximum_blob_owners: u64,
        maximum_total_entries: u64,
        maximum_total_pages: u64,
        maximum_logical_bytes: u64,
    ) -> Result<Self, TransactionError> {
        if maximum_outcomes > MAX_OUTCOMES_PER_NAMESPACE as u64
            || maximum_blob_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
            || maximum_total_entries == 0
            || maximum_total_entries
                > 1 + MAX_OUTCOMES_PER_NAMESPACE as u64 + MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
            || maximum_total_pages == 0
            || maximum_total_pages > MAX_INDEX_PAGES_PER_RUN * 3
            || maximum_logical_bytes == 0
            || maximum_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES * 3
        {
            return Err(TransactionError::ResourceLimit);
        }
        Ok(Self {
            maximum_outcomes,
            maximum_blob_owners,
            maximum_total_entries,
            maximum_total_pages,
            maximum_logical_bytes,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoordinatorMetadataLoadReport {
    pub runs: u64,
    pub entries: u64,
    pub pages_read: u64,
    pub logical_bytes: u64,
}

trait MetadataIndexReader<F>
where
    F: OwnershipFileSystem,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef;

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError>;

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut uste_storage::IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError>;
}

impl<S, F, W, E, I> MetadataIndexReader<F> for CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef {
        self.scope()
    }

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_root_manifests(filesystem, profile)
    }

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut uste_storage::IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.visit_index_run(filesystem, root, family, limits, visitor)
    }
}

impl<F, W, E, I> MetadataIndexReader<F> for AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef {
        self.scope()
    }

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_root_manifests(filesystem, profile)
    }

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut uste_storage::IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.visit_index_run(filesystem, root, family, limits, visitor)
    }
}

pub fn publish_coordinator_metadata_root<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
) -> Result<DurableIndexRoot, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if coordinator.uncertain {
        return Err(TransactionError::OutcomeUnknown);
    }
    let revision = coordinator
        .state
        .current_checkpoint_revision()
        .ok_or(TransactionError::InvalidRequest)?;
    let (anchor_revision, certificate_digest) = coordinator
        .journal
        .checkpoint_anchor()
        .ok_or(TransactionError::InvalidRequest)?;
    if revision != anchor_revision
        || coordinator.state.current_checkpoint_scope() != coordinator.scope
        || coordinator.outcomes.len() > MAX_OUTCOMES_PER_NAMESPACE
        || coordinator.committed_blob_owners.len() > MAX_COMMITTED_BLOBS_PER_JOURNAL
        || coordinator.outcomes.len() != coordinator.transactions.len()
        || coordinator.outcomes.iter().any(|(key, outcome)| {
            outcome.revision > revision
                || coordinator.transactions.get(&outcome.transaction_id)
                    != Some(&(key.principal, *outcome))
        })
        || coordinator
            .committed_blob_owners
            .iter()
            .any(|((scope, id), (reference, _))| {
                *scope != coordinator.scope
                    || reference.scope() != coordinator.scope
                    || *id != reference.id()
            })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let logical_state_digest = coordinator
        .state
        .current_logical_state_digest()
        .map_err(checkpoint_error)?;
    let outcome_count =
        u64::try_from(coordinator.outcomes.len()).map_err(|_| TransactionError::ResourceLimit)?;
    let owner_count = u64::try_from(coordinator.committed_blob_owners.len())
        .map_err(|_| TransactionError::ResourceLimit)?;

    let metadata = coordinator
        .journal
        .publish_index_run(
            filesystem,
            coordinator.scope,
            revision,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_METADATA,
            [IndexEntry {
                key: METADATA_KEY.to_vec(),
                value: metadata_value(revision, outcome_count, owner_count),
            }],
        )
        .map_err(TransactionError::Storage)?;
    if metadata.entry_count() != 1 {
        return Err(TransactionError::IntegrityFailure);
    }
    let mut runs = Vec::with_capacity(3);
    runs.push(metadata);

    if outcome_count != 0 {
        let entries = coordinator
            .outcomes
            .iter()
            .map(|(key, outcome)| encode_outcome(key.principal, key.key, *outcome));
        let run = coordinator
            .journal
            .publish_index_run_fallible(
                filesystem,
                coordinator.scope,
                revision,
                COORDINATOR_METADATA_PROFILE_V1,
                FAMILY_OUTCOME,
                entries,
            )
            .map_err(TransactionError::Storage)?;
        if run.entry_count() != outcome_count {
            return Err(TransactionError::IntegrityFailure);
        }
        runs.push(run);
    }
    if owner_count != 0 {
        let entries = coordinator
            .committed_blob_owners
            .values()
            .map(|(reference, principal)| encode_owner(*reference, *principal));
        let run = coordinator
            .journal
            .publish_index_run_fallible(
                filesystem,
                coordinator.scope,
                revision,
                COORDINATOR_METADATA_PROFILE_V1,
                FAMILY_BLOB_OWNER,
                entries,
            )
            .map_err(TransactionError::Storage)?;
        if run.entry_count() != owner_count {
            return Err(TransactionError::IntegrityFailure);
        }
        runs.push(run);
    }
    coordinator
        .journal
        .publish_index_root(
            filesystem,
            IndexRootInput {
                scope: coordinator.scope,
                revision,
                certificate_digest,
                reducer_profile: S::REDUCER_PROFILE,
                logical_state_digest,
                index_profile: COORDINATOR_METADATA_PROFILE_V1,
            },
            &runs,
        )
        .map_err(TransactionError::Storage)
}

/// Discover provisional certificate-bound manifests without scanning metadata runs.
/// Reconstruction authenticates every run under caller-selected limits before returning a seed.
pub fn load_coordinator_metadata_candidates<S, F, W, E, I>(
    coordinator: &CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
) -> Result<Vec<CoordinatorMetadataCandidate>, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    load_candidates::<S, F, _>(coordinator, filesystem)
}

/// Recovery-owner form of bounded provisional metadata-manifest discovery.
pub fn load_coordinator_metadata_candidates_for_recovery<S, F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
) -> Result<Vec<CoordinatorMetadataCandidate>, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    load_candidates::<S, F, _>(recovery, filesystem)
}

fn load_candidates<S, F, R>(
    reader: &R,
    filesystem: &mut F,
) -> Result<Vec<CoordinatorMetadataCandidate>, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    R: MetadataIndexReader<F>,
{
    Ok(reader
        .reader_load_roots(filesystem, COORDINATOR_METADATA_PROFILE_V1)?
        .into_iter()
        .filter(|root| root.reducer_profile() == &S::REDUCER_PROFILE)
        .map(|root| CoordinatorMetadataCandidate { root })
        .collect())
}

pub fn reconstruct_coordinator_metadata_seed<S, F, W, E, I>(
    coordinator: &CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    candidate: &CoordinatorMetadataCandidate,
    state: S,
    limits: CoordinatorMetadataLoadLimits,
) -> Result<(CoordinatorRecoverySeed<S>, CoordinatorMetadataLoadReport), TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    reconstruct_seed_with_reader(coordinator, filesystem, candidate, state, limits)
}

pub fn reconstruct_coordinator_metadata_seed_for_recovery<S, F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: &CoordinatorMetadataCandidate,
    state: S,
    limits: CoordinatorMetadataLoadLimits,
) -> Result<(CoordinatorRecoverySeed<S>, CoordinatorMetadataLoadReport), TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    reconstruct_seed_with_reader(recovery, filesystem, candidate, state, limits)
}

fn reconstruct_seed_with_reader<S, F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &CoordinatorMetadataCandidate,
    state: S,
    limits: CoordinatorMetadataLoadLimits,
) -> Result<(CoordinatorRecoverySeed<S>, CoordinatorMetadataLoadReport), TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    R: MetadataIndexReader<F>,
{
    if candidate.root.scope() != reader.reader_scope()
        || candidate.root.reducer_profile() != &S::REDUCER_PROFILE
        || candidate.root.index_profile() != &COORDINATOR_METADATA_PROFILE_V1
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let mut budget = LoadBudget::new(limits);
    let mut metadata = None;
    visit_family(
        reader,
        filesystem,
        candidate,
        FAMILY_METADATA,
        1,
        &mut budget,
        &mut |key, value| {
            if key != METADATA_KEY || metadata.is_some() {
                return Err(StorageError::IntegrityFailure);
            }
            metadata = Some(parse_metadata(value, candidate.revision())?);
            Ok(())
        },
    )?;
    let metadata = metadata.ok_or(TransactionError::IntegrityFailure)?;
    validate_shape(candidate, metadata, limits)?;
    let outcome_count = metadata.outcomes;
    let owner_count = metadata.owners;

    let mut outcomes = BTreeMap::new();
    let mut transactions = BTreeMap::new();
    if outcome_count != 0 {
        visit_family(
            reader,
            filesystem,
            candidate,
            FAMILY_OUTCOME,
            outcome_count,
            &mut budget,
            &mut |key, value| {
                let (principal, idempotency_key, outcome) =
                    decode_outcome(key, value, candidate.revision())?;
                if outcomes
                    .insert(
                        RetryKey {
                            principal,
                            key: idempotency_key,
                        },
                        outcome,
                    )
                    .is_some()
                    || transactions
                        .insert(outcome.transaction_id, (principal, outcome))
                        .is_some()
                {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok(())
            },
        )?;
    }

    let mut owners = BTreeMap::new();
    if owner_count != 0 {
        visit_family(
            reader,
            filesystem,
            candidate,
            FAMILY_BLOB_OWNER,
            owner_count,
            &mut budget,
            &mut |key, value| {
                let (reference, principal) = decode_owner(candidate.root.scope(), key, value)?;
                if owners
                    .insert((reference.scope(), reference.id()), (reference, principal))
                    .is_some()
                {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok(())
            },
        )?;
    }
    let seed = CoordinatorRecoverySeed::from_authenticated_index_root_parts(
        &candidate.root,
        state,
        outcomes,
        transactions,
        owners,
    )?;
    Ok((seed, budget.report))
}

fn metadata_value(revision: CommitRevision, outcomes: u64, owners: u64) -> Vec<u8> {
    let mut value = Vec::with_capacity(METADATA_VALUE_BYTES);
    value.extend_from_slice(b"UCMD");
    value.extend_from_slice(&[1, 0, 0, 0]);
    value.extend_from_slice(&revision.get().to_be_bytes());
    value.extend_from_slice(&outcomes.to_be_bytes());
    value.extend_from_slice(&owners.to_be_bytes());
    value.extend_from_slice(&(outcomes * OUTCOME_LOGICAL_BYTES).to_be_bytes());
    value.extend_from_slice(&(owners * OWNER_LOGICAL_BYTES).to_be_bytes());
    value
}

#[derive(Clone, Copy)]
struct Metadata {
    outcomes: u64,
    owners: u64,
    outcome_logical_bytes: u64,
    owner_logical_bytes: u64,
}

fn parse_metadata(value: &[u8], revision: CommitRevision) -> Result<Metadata, StorageError> {
    if value.len() != METADATA_VALUE_BYTES
        || &value[..4] != b"UCMD"
        || value[4..8] != [1, 0, 0, 0]
        || read_u64(&value[8..16])? != revision.get()
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(Metadata {
        outcomes: read_u64(&value[16..24])?,
        owners: read_u64(&value[24..32])?,
        outcome_logical_bytes: read_u64(&value[32..40])?,
        owner_logical_bytes: read_u64(&value[40..48])?,
    })
}

fn encode_outcome(
    principal: PrincipalDigest,
    key: IdempotencyKey,
    outcome: TransactionOutcome,
) -> Result<IndexEntry, StorageError> {
    let mut encoded_key = Vec::new();
    encoded_key
        .try_reserve_exact(OUTCOME_KEY_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    encoded_key.extend_from_slice(&principal.as_bytes());
    encoded_key.extend_from_slice(key.as_bytes());
    let mut value = Vec::new();
    value
        .try_reserve_exact(OUTCOME_VALUE_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    value.extend_from_slice(outcome.transaction_id.as_bytes());
    value.extend_from_slice(&outcome.revision.get().to_be_bytes());
    value.extend_from_slice(&outcome.request_digest);
    value.extend_from_slice(&outcome.result_digest);
    value.extend_from_slice(&outcome.expires_at.seconds().to_be_bytes());
    value.extend_from_slice(&outcome.expires_at.nanoseconds().to_be_bytes());
    value.extend_from_slice(&[0; 4]);
    Ok(IndexEntry {
        key: encoded_key,
        value,
    })
}

fn decode_outcome(
    key: &[u8],
    value: &[u8],
    maximum_revision: CommitRevision,
) -> Result<(PrincipalDigest, IdempotencyKey, TransactionOutcome), StorageError> {
    if key.len() != OUTCOME_KEY_BYTES || value.len() != OUTCOME_VALUE_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let principal = PrincipalDigest::from_bytes(read_array(&key[..32])?);
    let idempotency_key = IdempotencyKey::from_bytes(read_array(&key[32..])?);
    let revision = CommitRevision::new(read_u64(&value[16..24])?)
        .map_err(|_| StorageError::IntegrityFailure)?;
    if revision > maximum_revision {
        return Err(StorageError::IntegrityFailure);
    }
    if value[100..] != [0; 4] {
        return Err(StorageError::IntegrityFailure);
    }
    let expires_at = UtcInstant::new(read_i64(&value[88..96])?, read_u32(&value[96..100])?)
        .map_err(|_| StorageError::IntegrityFailure)?;
    Ok((
        principal,
        idempotency_key,
        TransactionOutcome {
            transaction_id: TransactionId::from_bytes(read_array(&value[..16])?),
            revision,
            request_digest: read_array(&value[24..56])?,
            result_digest: read_array(&value[56..88])?,
            expires_at,
        },
    ))
}

fn encode_owner(
    reference: BlobReference,
    principal: PrincipalDigest,
) -> Result<IndexEntry, StorageError> {
    let mut value = Vec::new();
    value
        .try_reserve_exact(OWNER_VALUE_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    value.extend_from_slice(&[1, 0, 0, 0]);
    value.extend_from_slice(&reference.byte_len().to_be_bytes());
    value.extend_from_slice(&reference.chunk_count().to_be_bytes());
    value.extend_from_slice(&reference.content_digest());
    value.extend_from_slice(&principal.as_bytes());
    Ok(IndexEntry {
        key: reference.id().as_bytes().to_vec(),
        value,
    })
}

fn decode_owner(
    scope: uste_types::NamespaceRef,
    key: &[u8],
    value: &[u8],
) -> Result<(BlobReference, PrincipalDigest), StorageError> {
    if key.len() != OWNER_KEY_BYTES || value.len() != OWNER_VALUE_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    if value[..4] != [1, 0, 0, 0] {
        return Err(StorageError::IntegrityFailure);
    }
    let reference = BlobReference::new(
        scope,
        BlobId::from_bytes(read_array(key)?),
        read_u64(&value[4..12])?,
        read_u32(&value[12..16])?,
        read_array(&value[16..48])?,
    )
    .map_err(|_| StorageError::IntegrityFailure)?;
    Ok((
        reference,
        PrincipalDigest::from_bytes(read_array(&value[48..])?),
    ))
}

fn validate_shape(
    candidate: &CoordinatorMetadataCandidate,
    metadata: Metadata,
    limits: CoordinatorMetadataLoadLimits,
) -> Result<(), TransactionError> {
    let outcomes = metadata.outcomes;
    let owners = metadata.owners;
    let total_entries = 1_u64
        .checked_add(outcomes)
        .and_then(|total| total.checked_add(owners))
        .ok_or(TransactionError::ResourceLimit)?;
    let outcome_logical_bytes = outcomes
        .checked_mul(OUTCOME_LOGICAL_BYTES)
        .ok_or(TransactionError::ResourceLimit)?;
    let owner_logical_bytes = owners
        .checked_mul(OWNER_LOGICAL_BYTES)
        .ok_or(TransactionError::ResourceLimit)?;
    let total_logical_bytes = METADATA_LOGICAL_BYTES
        .checked_add(outcome_logical_bytes)
        .and_then(|total| total.checked_add(owner_logical_bytes))
        .ok_or(TransactionError::ResourceLimit)?;
    if outcomes > limits.maximum_outcomes
        || owners > limits.maximum_blob_owners
        || total_entries > limits.maximum_total_entries
        || total_logical_bytes > limits.maximum_logical_bytes
    {
        return Err(TransactionError::ResourceLimit);
    }
    if metadata.outcome_logical_bytes != outcome_logical_bytes
        || metadata.owner_logical_bytes != owner_logical_bytes
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let expected = [
        (FAMILY_METADATA, 1),
        (FAMILY_OUTCOME, outcomes),
        (FAMILY_BLOB_OWNER, owners),
    ];
    let mut seen = [false; 3];
    let mut total_pages = 0_u64;
    for run in candidate.root.runs() {
        let slot = usize::from(
            run.family()
                .checked_sub(1)
                .ok_or(TransactionError::IntegrityFailure)?,
        );
        if slot >= seen.len()
            || seen[slot]
            || expected[slot].1 == 0
            || run.entry_count() != expected[slot].1
        {
            return Err(TransactionError::IntegrityFailure);
        }
        seen[slot] = true;
        total_pages = total_pages
            .checked_add(run.page_count())
            .ok_or(TransactionError::ResourceLimit)?;
    }
    if expected
        .iter()
        .enumerate()
        .any(|(slot, (_, count))| seen[slot] != (*count != 0))
    {
        return Err(TransactionError::IntegrityFailure);
    }
    if total_pages > limits.maximum_total_pages {
        return Err(TransactionError::ResourceLimit);
    }
    Ok(())
}

fn visit_family<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &CoordinatorMetadataCandidate,
    family: u8,
    expected_entries: u64,
    budget: &mut LoadBudget,
    visitor: &mut uste_storage::IndexRunVisitor<'_>,
) -> Result<(), TransactionError>
where
    F: OwnershipFileSystem,
    R: MetadataIndexReader<F>,
{
    let limits = IndexRunReadLimits::new(
        budget.remaining_pages()?.min(MAX_INDEX_PAGES_PER_RUN),
        expected_entries,
        budget
            .remaining_logical_bytes()?
            .min(MAX_INDEX_RUN_LOGICAL_BYTES),
    )
    .map_err(TransactionError::Storage)?;
    let report = reader.reader_visit_run(filesystem, &candidate.root, family, limits, visitor)?;
    if report.entries != expected_entries {
        return Err(TransactionError::IntegrityFailure);
    }
    budget.add(&report)
}

struct LoadBudget {
    limits: CoordinatorMetadataLoadLimits,
    report: CoordinatorMetadataLoadReport,
}

impl LoadBudget {
    const fn new(limits: CoordinatorMetadataLoadLimits) -> Self {
        Self {
            limits,
            report: CoordinatorMetadataLoadReport {
                runs: 0,
                entries: 0,
                pages_read: 0,
                logical_bytes: 0,
            },
        }
    }

    fn remaining_pages(&self) -> Result<u64, TransactionError> {
        self.limits
            .maximum_total_pages
            .checked_sub(self.report.pages_read)
            .filter(|remaining| *remaining != 0)
            .ok_or(TransactionError::ResourceLimit)
    }

    fn remaining_logical_bytes(&self) -> Result<u64, TransactionError> {
        self.limits
            .maximum_logical_bytes
            .checked_sub(self.report.logical_bytes)
            .filter(|remaining| *remaining != 0)
            .ok_or(TransactionError::ResourceLimit)
    }

    fn add(&mut self, report: &IndexRunReadReport) -> Result<(), TransactionError> {
        self.report.runs = self
            .report
            .runs
            .checked_add(1)
            .ok_or(TransactionError::ResourceLimit)?;
        self.report.entries = self
            .report
            .entries
            .checked_add(report.entries)
            .ok_or(TransactionError::ResourceLimit)?;
        self.report.pages_read = self
            .report
            .pages_read
            .checked_add(report.stats.pages_read)
            .ok_or(TransactionError::ResourceLimit)?;
        self.report.logical_bytes = self
            .report
            .logical_bytes
            .checked_add(report.logical_bytes)
            .ok_or(TransactionError::ResourceLimit)?;
        if self.report.pages_read > self.limits.maximum_total_pages
            || self.report.logical_bytes > self.limits.maximum_logical_bytes
        {
            return Err(TransactionError::ResourceLimit);
        }
        Ok(())
    }
}

fn checkpoint_error(error: CheckpointStateError) -> TransactionError {
    match error {
        CheckpointStateError::ResourceLimit => TransactionError::ResourceLimit,
        CheckpointStateError::Invalid | CheckpointStateError::UnsupportedProfile => {
            TransactionError::IntegrityFailure
        }
    }
}

fn read_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], StorageError> {
    bytes.try_into().map_err(|_| StorageError::IntegrityFailure)
}

fn read_u64(bytes: &[u8]) -> Result<u64, StorageError> {
    Ok(u64::from_be_bytes(read_array(bytes)?))
}

fn read_i64(bytes: &[u8]) -> Result<i64, StorageError> {
    Ok(i64::from_be_bytes(read_array(bytes)?))
}

fn read_u32(bytes: &[u8]) -> Result<u32, StorageError> {
    Ok(u32::from_be_bytes(read_array(bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

    #[test]
    fn frozen_metadata_outcome_and_owner_layouts_are_exact() {
        assert_eq!(
            COORDINATOR_METADATA_PROFILE_V1,
            crate::sha256(b"coordinator-meta-v1")
        );
        let revision = CommitRevision::new(7).unwrap();
        let metadata = metadata_value(revision, 2, 3);
        assert_eq!(metadata.len(), METADATA_VALUE_BYTES);
        assert_eq!(&metadata[..8], b"UCMD\x01\0\0\0");
        assert_eq!(read_u64(&metadata[8..16]).unwrap(), 7);
        assert_eq!(read_u64(&metadata[16..24]).unwrap(), 2);
        assert_eq!(read_u64(&metadata[24..32]).unwrap(), 3);
        assert_eq!(read_u64(&metadata[32..40]).unwrap(), 304);
        assert_eq!(read_u64(&metadata[40..48]).unwrap(), 288);

        let principal = PrincipalDigest::from_bytes([0x11; 32]);
        let key = IdempotencyKey::from_bytes([0x22; 16]);
        let outcome = TransactionOutcome {
            transaction_id: TransactionId::from_bytes([0x33; 16]),
            revision,
            request_digest: [0x44; 32],
            result_digest: [0x55; 32],
            expires_at: UtcInstant::new(-9, 123).unwrap(),
        };
        let encoded = encode_outcome(principal, key, outcome).unwrap();
        assert_eq!(encoded.key.len(), OUTCOME_KEY_BYTES);
        assert_eq!(encoded.value.len(), OUTCOME_VALUE_BYTES);
        assert_eq!(&encoded.key[..32], &[0x11; 32]);
        assert_eq!(&encoded.key[32..], &[0x22; 16]);
        assert_eq!(&encoded.value[..16], &[0x33; 16]);
        assert_eq!(&encoded.value[100..], &[0; 4]);
        assert_eq!(
            decode_outcome(&encoded.key, &encoded.value, revision).unwrap(),
            (principal, key, outcome)
        );

        let scope = NamespaceRef::new(
            DatabaseId::from_bytes([0x66; 16]),
            NamespaceId::from_bytes([0x77; 16]),
        );
        let reference =
            BlobReference::new(scope, BlobId::from_bytes([0x88; 16]), 5, 1, [0x99; 32]).unwrap();
        let owner = encode_owner(reference, principal).unwrap();
        assert_eq!(owner.key, vec![0x88; OWNER_KEY_BYTES]);
        assert_eq!(owner.value.len(), OWNER_VALUE_BYTES);
        assert_eq!(&owner.value[..4], &[1, 0, 0, 0]);
        assert_eq!(
            decode_owner(scope, &owner.key, &owner.value).unwrap(),
            (reference, principal)
        );

        let mut noncanonical = encoded.value;
        noncanonical[100] = 1;
        assert_eq!(
            decode_outcome(&encoded.key, &noncanonical, revision),
            Err(StorageError::IntegrityFailure)
        );
    }
}
