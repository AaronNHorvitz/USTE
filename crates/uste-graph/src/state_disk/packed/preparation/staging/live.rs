//! Ready or one-certified-pending graph state; no full-snapshot checkpoint impersonation.
use super::*;
mod authorized_read;
pub use authorized_read::{PackedGraphExpansionLimits, PackedGraphReadLimits};
mod authorized_write;
pub use authorized_write::{PackedGraphWritePreparationLimits, PackedGraphWritePublicationLimits};
mod recovery;
pub use recovery::{
    PackedGraphSuffixRecoveryLimits, PackedGraphSuffixRecoveryReport, recover_packed_graph_suffix,
};
use uste_storage::journal::{CertifiedPackedRoot, JournalStore};
use uste_txn::{
    PackedCommitCoordinator, PackedCoordinatorPublicationState, PackedCoordinatorState,
};

pub struct GraphPackedLiveState {
    base: PackedGraphBase,
    root: CertifiedPackedRoot,
    pending: Option<PackedGraphDelta>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphPackedLiveSnapshot {
    pub scope: NamespaceRef,
    pub revision: CommitRevision,
    /// None while a certified change awaits derived-root repair. Never a v1 digest.
    pub ordered_state_digest: Option<[u8; 32]>,
}
pub struct GraphPackedLivePublication {
    base: PackedGraphBase,
    root: CertifiedPackedRoot,
    predecessor: (CommitRevision, [u8; 32]),
    predecessor_digest: [u8; 32],
    result_digest: [u8; 32],
}
fn matches_claims(base: &PackedGraphBase, claims: PackedRootClaims) -> bool {
    let mut expected = base.publication_claims();
    expected.generation = claims.generation;
    claims.generation != 0 && expected == claims
}
fn matches_root(base: &PackedGraphBase, root: &CertifiedPackedRoot) -> bool {
    let manifest = root.manifest();
    manifest.context().scope == base.scope
        && manifest.context().profile == GRAPH_PACKED_PROFILE_V1
        && matches_claims(base, manifest.claims())
        && manifest.families() == base.families()
}
impl GraphPackedLiveState {
    /// Raw trusted installation from a semantically admitted base, not just a canonical root.
    pub fn from_published<F, W, E, I>(
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        base: PackedGraphBase,
        root: CertifiedPackedRoot,
    ) -> Result<Self, GraphDiskError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if maintenance.anchor() != base.anchor || !matches_root(&base, &root) {
            return Err(GraphDiskError::RootStateMismatch);
        }
        maintenance.validate_root_binding(&root)?;
        for tree in &base.trees {
            maintenance.validate_tree_binding(tree)?;
        }
        Ok(Self {
            base,
            root,
            pending: None,
        })
    }
    pub fn scope(&self) -> NamespaceRef {
        self.base.scope
    }
    pub fn revision(&self) -> CommitRevision {
        self.pending
            .as_ref()
            .map_or(self.base.anchor.0, PackedGraphDelta::revision)
    }
    pub fn current_base(&self) -> Option<&PackedGraphBase> {
        self.pending.is_none().then_some(&self.base)
    }
    pub fn needs_repair(&self) -> bool {
        self.pending.is_some()
    }
    fn can_publish(&self, plan: &PackedGraphDelta) -> bool {
        self.pending.is_none() && plan.matches_base(&self.base)
    }
}
impl TransactionState for GraphPackedLiveState {
    type Prepared = PackedGraphDelta;
    type Snapshot = GraphPackedLiveSnapshot;
    fn prepare(
        &self,
        _: &[u8],
        _: Option<&BlobInventory>,
        _: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        Err(ApplyError::InvalidRequest)
    }
    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.result_digest()
    }
    fn publish(&mut self, prepared: Self::Prepared) {
        assert!(
            self.can_publish(&prepared),
            "packed graph publication requires its exact ready base"
        );
        self.pending = Some(prepared);
    }
    fn snapshot(&self) -> Self::Snapshot {
        GraphPackedLiveSnapshot {
            scope: self.scope(),
            revision: self.revision(),
            ordered_state_digest: self
                .current_base()
                .map(|b| b.publication_claims().state_digest),
        }
    }
}
impl ExternallyPreparedTransactionState for GraphPackedLiveState {
    fn validate_external_prepared(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &Self::Prepared,
    ) -> Result<(), ApplyError> {
        if !self.can_publish(prepared) {
            return Err(ApplyError::Conflict);
        }
        prepared
            .prepared
            .prepared
            .validate_external_request(request, inventory, revision)
    }
}
impl PackedCoordinatorState for GraphPackedLiveState {
    fn validate_packed_owner<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
    ) -> Result<(), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if self.pending.is_some() {
            return Err(TransactionError::Conflict);
        }
        journal
            .validate_packed_root_certificate(&self.root)
            .map_err(TransactionError::Storage)?;
        for tree in &self.base.trees {
            journal
                .validate_packed_tree_binding(tree)
                .map_err(TransactionError::Storage)?;
        }
        Ok(())
    }
    fn validate_packed_metadata(
        &self,
        scope: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError> {
        if self.pending.is_some() || self.scope() != scope || !matches_claims(&self.base, claims) {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}
impl PackedCoordinatorPublicationState for GraphPackedLiveState {
    fn packed_publication_claims(
        &self,
        scope: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError> {
        if self.pending.is_some() || self.scope() != scope || self.base.anchor != anchor {
            return Err(ApplyError::Conflict);
        }
        Ok(self.base.publication_claims())
    }
}
impl uste_txn::AuthorizedDiskPolicyState for GraphPackedLiveState {
    fn current_durable_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        self.current_base()
            .ok_or(ApplyError::Conflict)?
            .namespace_policy()
            .ok_or(ApplyError::InvalidRequest)
    }
}
impl PostCommitStateMaintenance for GraphPackedLiveState {
    type Publication = GraphPackedLivePublication;
    fn install_publication(
        &mut self,
        scope: NamespaceRef,
        anchor: Option<(CommitRevision, [u8; 32])>,
        publication: Self::Publication,
    ) -> Result<(), ApplyError> {
        let pending = self.pending.as_ref().ok_or(ApplyError::Conflict)?;
        let policy = pending
            .prepared
            .prepared
            .policy_change
            .as_ref()
            .or(self.base.policy.as_ref());
        if scope != self.scope()
            || publication.base.scope != scope
            || publication.predecessor != self.base.anchor
            || publication.predecessor_digest != self.base.publication_claims().state_digest
            || publication.result_digest != pending.result_digest()
            || publication.base.anchor.0 != pending.revision()
            || anchor != Some(publication.base.anchor)
            || publication.base.counts != pending.data.target_counts
            || publication.base.policy.as_ref() != policy
            || !matches_root(&publication.base, &publication.root)
        {
            return Err(ApplyError::Conflict);
        }
        self.base = publication.base;
        self.root = publication.root;
        self.pending = None;
        Ok(())
    }
}

/// Repair only an already certified pending change. Errors do not roll back that commit.
/// No-op on ready state; an older retry cannot repair a different pending revision.
pub fn publish_packed_graph_live_base<F, W, E, I>(
    coordinator: &mut PackedCommitCoordinator<GraphPackedLiveState, F, W, E, I>,
    fs: &mut F,
    outcome: TransactionOutcome,
    limits: PackedGraphStageLimits,
    maximum_publication_attempts: u8,
) -> Result<Option<PackedGraphStageReport>, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let mut borrow = coordinator.reducer_and_index_maintenance()?;
    let state = borrow.reducer;
    let Some(plan) = state.pending.as_ref() else {
        return Ok(None);
    };
    if plan.revision() != outcome.revision || plan.result_digest() != outcome.result_digest {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if maximum_publication_attempts == 0 || maximum_publication_attempts > 64 {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    admit_stage(plan, limits)?;
    let (base, report) = {
        let mut maintenance = borrow.indexes.packed_indexes(fs, limits.certificates)?;
        stage_on_maintenance(&mut maintenance, fs, &state.base, plan, limits)?
    };
    let root = borrow.indexes.publish_packed_root(
        fs,
        GRAPH_PACKED_PROFILE_V1,
        base.publication_claims(),
        &base.families(),
        maximum_publication_attempts,
    )?;
    let publication = GraphPackedLivePublication {
        base,
        root,
        predecessor: state.base.anchor,
        predecessor_digest: state.base.publication_claims().state_digest,
        result_digest: plan.result_digest(),
    };
    coordinator.install_postcommit_publication(publication)?;
    Ok(Some(report))
}
