//! Inductively journal-validated private coordinator metadata. No root publication.
mod admission;
use super::*;
use crate::{PackedIndexMaintenance, RecoveredFrontierTransaction};
pub use admission::{
    PackedCoordinatorAdmissionLimits, PackedCoordinatorAdmissionReport,
    admit_packed_coordinator_prefix,
};
use uste_storage::{
    IndexDelta,
    journal::{CanonicalPackedTree, CertificateAnchorReadLimits},
    packed_root_manifest::PackedRootFamily,
    packed_tree_batch::{MAX_BATCH_DELTAS, TreeBatchLimits, TreeBatchReport},
    packed_tree_lookup::{TreeLookupLimits, TreeLookupReport},
};
use uste_types::NamespaceRef;

pub const COORDINATOR_PACKED_PROFILE_V1: [u8; 32] = [
    0x67, 0x01, 0x6e, 0xfc, 0x82, 0xcd, 0x60, 0xfd, 0x6d, 0x22, 0xf1, 0xaf, 0xd3, 0xe2, 0xb4, 0xf8,
    0x19, 0x8d, 0x59, 0xb2, 0x64, 0x00, 0x6e, 0x7d, 0x1b, 0x2e, 0xba, 0x96, 0x96, 0xad, 0xbf, 0x7d,
];

/// Not consumer authority or admission of an independently discovered root.
pub struct PackedCoordinatorPrefix {
    scope: NamespaceRef,
    anchor: (CommitRevision, [u8; 32]),
    trees: [CanonicalPackedTree; 4],
}
#[derive(Clone, Copy)]
pub struct PackedCoordinatorLimits {
    pub certificates: CertificateAnchorReadLimits,
    pub lookup: TreeLookupLimits,
    pub batch: TreeBatchLimits,
    pub maximum_references: usize,
    pub maximum_owners: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedCoordinatorReport {
    pub owner_lookup_pages: u64,
    pub owner_lookup_bytes: u64,
    pub new_owners: u64,
    pub batches: [TreeBatchReport; 4],
}
impl PackedCoordinatorPrefix {
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.anchor
    }
    pub fn scope(&self) -> NamespaceRef {
        self.scope
    }
    pub fn owner_count(&self) -> u64 {
        self.trees[2].family_descriptor().commitment.entries()
    }
    pub fn families(&self) -> [PackedRootFamily; 4] {
        core::array::from_fn(|index| self.trees[index].family_descriptor())
    }
    /// Privileged historical metadata read; expiry and consumer policy are caller responsibilities.
    pub fn retry<F, W, E, I>(
        &self,
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        fs: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        limits: TreeLookupLimits,
    ) -> Result<(Option<TransactionOutcome>, TreeLookupReport), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let mut encoded = [0; 48];
        encoded[..32].copy_from_slice(&principal.as_bytes());
        encoded[32..].copy_from_slice(key.as_bytes());
        let result = maintenance.get(fs, &self.trees[0], &encoded, limits)?;
        let outcome = result
            .value
            .map(|value| decode_outcome(&encoded, value.as_slice(), self.anchor.0).map(|v| v.2))
            .transpose()
            .map_err(TransactionError::Storage)?;
        Ok((outcome, result.report))
    }
    pub fn transaction<F, W, E, I>(
        &self,
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        fs: &mut F,
        id: TransactionId,
        limits: TreeLookupLimits,
    ) -> Result<
        (
            Option<(PrincipalDigest, TransactionOutcome)>,
            TreeLookupReport,
        ),
        TransactionError,
    >
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let result = maintenance.get(fs, &self.trees[1], id.as_bytes(), limits)?;
        let outcome = result
            .value
            .map(|value| {
                let value = value.as_slice();
                if value.len() != 136 {
                    return Err(StorageError::IntegrityFailure);
                }
                let mut key = [0; 48];
                key[..32].copy_from_slice(&value[..32]);
                let (principal, _, outcome) = decode_outcome(&key, &value[32..], self.anchor.0)?;
                if outcome.transaction_id != id {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok((principal, outcome))
            })
            .transpose()
            .map_err(TransactionError::Storage)?;
        Ok((outcome, result.report))
    }
    pub fn owner<F, W, E, I>(
        &self,
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        fs: &mut F,
        id: BlobId,
        limits: TreeLookupLimits,
    ) -> Result<(Option<(BlobReference, PrincipalDigest)>, TreeLookupReport), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let result = maintenance.get(fs, &self.trees[2], &id.as_bytes(), limits)?;
        let owner = result
            .value
            .map(|value| decode_owner(self.scope, &id.as_bytes(), value.as_slice()))
            .transpose()
            .map_err(TransactionError::Storage)?;
        Ok((owner, result.report))
    }

    pub fn first_revision<F, W, E, I>(
        &self,
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        fs: &mut F,
        id: BlobId,
        limits: TreeLookupLimits,
    ) -> Result<(Option<CommitRevision>, TreeLookupReport), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let result = maintenance.get(fs, &self.trees[3], &id.as_bytes(), limits)?;
        let revision = result
            .value
            .map(|value| {
                let revision = CommitRevision::new(read_u64(value.as_slice())?)
                    .map_err(|_| StorageError::IntegrityFailure)?;
                if revision > self.anchor.0 {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok(revision)
            })
            .transpose()
            .map_err(TransactionError::Storage)?;
        Ok((revision, result.report))
    }
}

pub fn stage_packed_coordinator_prefix<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    base: Option<&PackedCoordinatorPrefix>,
    transaction: &RecoveredFrontierTransaction,
    limits: PackedCoordinatorLimits,
) -> Result<(PackedCoordinatorPrefix, PackedCoordinatorReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if transaction.scope != recovery.scope()
        || base.is_none() && transaction.revision != CommitRevision::FIRST
        || base.is_some_and(|b| {
            b.scope != recovery.scope()
                || b.anchor.0.checked_next().ok() != Some(transaction.revision)
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let references = transaction
        .blob_inventory
        .as_ref()
        .map_or(&[][..], |i| i.references());
    if limits.maximum_references > MAX_BATCH_DELTAS
        || references.len() > limits.maximum_references
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || transaction.revision.get() > MAX_OUTCOMES_PER_NAMESPACE as u64
        || base.is_some_and(|b| b.owner_count() > limits.maximum_owners)
    {
        return Err(TransactionError::ResourceLimit);
    }
    let mut maintenance = recovery.packed_indexes_with_io(fs, transaction, limits.certificates)?;
    let mut report = PackedCoordinatorReport::default();
    let mut deltas: [Vec<IndexDelta>; 4] = core::array::from_fn(|_| Vec::new());
    for (index, delta) in deltas.iter_mut().enumerate() {
        delta
            .try_reserve_exact(if index < 2 { 1 } else { references.len() })
            .map_err(|_| TransactionError::ResourceLimit)?;
    }
    let retry = encode_outcome(
        transaction.principal,
        transaction.idempotency_key,
        transaction.outcome,
    )
    .map_err(TransactionError::Storage)?;
    let mut value = Vec::new();
    value
        .try_reserve_exact(136)
        .map_err(|_| TransactionError::ResourceLimit)?;
    value.extend_from_slice(&transaction.principal.as_bytes());
    value.extend_from_slice(&retry.value);
    deltas[0].push(
        IndexDelta::new(retry.key, None, Some(retry.value)).map_err(TransactionError::Storage)?,
    );
    deltas[1].push(
        IndexDelta::new(
            transaction.outcome.transaction_id.as_bytes().to_vec(),
            None,
            Some(value),
        )
        .map_err(TransactionError::Storage)?,
    );
    for reference in references {
        let existing = if let Some(base) = base {
            let (existing, work) = base.owner(&maintenance, fs, reference.id(), limits.lookup)?;
            report.owner_lookup_pages = report
                .owner_lookup_pages
                .checked_add(work.pages)
                .ok_or(TransactionError::ResourceLimit)?;
            report.owner_lookup_bytes = report
                .owner_lookup_bytes
                .checked_add(work.encoded_bytes)
                .ok_or(TransactionError::ResourceLimit)?;
            existing
        } else {
            None
        };
        if let Some((previous, _)) = existing {
            if previous != *reference {
                return Err(TransactionError::IntegrityFailure);
            }
            continue;
        }
        report.new_owners += 1;
        if base
            .map_or(0, |b| b.owner_count())
            .checked_add(report.new_owners)
            .is_none_or(|n| n > limits.maximum_owners)
        {
            return Err(TransactionError::ResourceLimit);
        }
        let owner =
            encode_owner(*reference, transaction.principal).map_err(TransactionError::Storage)?;
        deltas[2].push(
            IndexDelta::new(owner.key.clone(), None, Some(owner.value))
                .map_err(TransactionError::Storage)?,
        );
        deltas[3].push(
            IndexDelta::new(
                owner.key,
                None,
                Some(transaction.revision.get().to_be_bytes().to_vec()),
            )
            .map_err(TransactionError::Storage)?,
        );
    }
    let mut trees = Vec::new();
    trees
        .try_reserve_exact(4)
        .map_err(|_| TransactionError::ResourceLimit)?;
    for (index, deltas) in deltas.iter().enumerate() {
        let stage = maintenance.stage(
            fs,
            COORDINATOR_PACKED_PROFILE_V1,
            index as u8 + 1,
            base.map(|b| &b.trees[index]),
            deltas,
            limits.batch,
        )?;
        report.batches[index] = stage.report();
        trees.push(stage.tree().clone());
    }
    let trees: [CanonicalPackedTree; 4] = trees
        .try_into()
        .map_err(|_| TransactionError::IntegrityFailure)?;
    let prefix = PackedCoordinatorPrefix {
        scope: transaction.scope,
        anchor: maintenance.anchor(),
        trees,
    };
    if prefix.trees[0].family_descriptor().commitment.entries() != transaction.revision.get()
        || prefix.trees[1].family_descriptor().commitment.entries() != transaction.revision.get()
        || prefix.trees[3].family_descriptor().commitment.entries() != prefix.owner_count()
    {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok((prefix, report))
}
