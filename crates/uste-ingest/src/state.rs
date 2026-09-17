use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use uste_graph::{
    EntityLifecycle, GraphSnapshot, GraphState, Operation, Record,
    decode_transaction as decode_graph_transaction,
};
use uste_policy::{
    Action, AuthorizationRequirement, AuthorizationRequirements, NamespacePolicy, Target,
};
use uste_spatial::{
    SpatialRecord, SpatialRecordRef, SpatialSnapshot, SpatialState,
    decode_transaction as decode_spatial_transaction,
};
use uste_storage::{BlobInventory, BlobReference, MAX_CHECKPOINT_BYTES};
use uste_txn::{
    ApplyError, AuthorizedReadState, AuthorizedTransactionState, CheckpointState,
    CheckpointStateError, DurablePolicyChange, TransactionState,
};
use uste_types::{CommitRevision, NamespaceRef, RecordRef, SourceEventRef};

use crate::codec::{
    Cursor, decode_checkpoint as decode_import_checkpoint, decode_scope, decode_source_event,
    encode_checkpoint as encode_import_checkpoint, encode_scope, encode_source_event,
};
use crate::{
    EngineTransaction, ImportAction, ImportBatch, ImportBatchOutcome, ImportCheckpoint,
    ImportError, ImportJob, ImportJobStatus, ImportReadOutput, ImportReadRequest, ImportStart,
    MAX_IMPORT_BATCHES, MAX_IMPORT_JOBS, MappingBinding, SourceBinding, decode_transaction,
    encode_transaction,
};

const INGEST_REDUCER_PROFILE: [u8; 32] = [
    0x75, 0x73, 0x74, 0x65, 0x2d, 0x69, 0x6e, 0x67, 0x65, 0x73, 0x74, 0x2d, 0x76, 0x31, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];
const INGEST_CHECKPOINT_MAGIC: &[u8; 8] = b"UICP\0\x01\0\0";

type ComponentOutcome = (ImportBatchOutcome, [u8; 32], Option<[u8; 32]>);

#[derive(Clone, Debug, Eq, PartialEq)]
struct BatchReceipt {
    request_digest: [u8; 32],
    revision: CommitRevision,
    row_count: u32,
    checkpoint: ImportCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JobState {
    checkpoint: ImportCheckpoint,
    batches: BTreeMap<u64, BatchReceipt>,
    source_events: BTreeMap<SourceEventRef, [u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct EngineSnapshot {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    graph: GraphSnapshot,
    spatial: SpatialSnapshot,
    jobs: BTreeMap<RecordRef, JobState>,
    batch_count: usize,
}

impl EngineSnapshot {
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }
    #[must_use]
    pub const fn revision(&self) -> Option<CommitRevision> {
        self.revision
    }
    #[must_use]
    pub const fn graph(&self) -> &GraphSnapshot {
        &self.graph
    }
    #[must_use]
    pub const fn spatial(&self) -> &SpatialSnapshot {
        &self.spatial
    }
    #[must_use]
    pub fn job(&self, id: RecordRef) -> Option<ImportJob> {
        self.jobs.get(&id).map(|job| ImportJob {
            checkpoint: job.checkpoint.clone(),
        })
    }
    #[must_use]
    pub const fn batch_count(&self) -> usize {
        self.batch_count
    }
}

#[derive(Clone, Debug)]
pub struct IngestState {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    graph: GraphState,
    spatial: SpatialState,
    jobs: BTreeMap<RecordRef, JobState>,
    source_events: BTreeMap<SourceEventRef, RecordRef>,
    batch_count: usize,
}

impl IngestState {
    #[must_use]
    pub fn new(scope: NamespaceRef) -> Self {
        Self {
            scope,
            revision: None,
            graph: GraphState::new(scope),
            spatial: SpatialState::new(scope),
            jobs: BTreeMap::new(),
            source_events: BTreeMap::new(),
            batch_count: 0,
        }
    }

    pub fn preview(
        &self,
        transaction: &EngineTransaction,
        blob_inventory: Option<&BlobInventory>,
    ) -> Result<ImportPreview, ImportError> {
        let base_revision = self.revision;
        let revision = match base_revision {
            Some(value) => value
                .checked_next()
                .map_err(|_| ImportError::VersionExhausted)?,
            None => CommitRevision::FIRST,
        };
        let canonical_request = encode_transaction(transaction)?;
        let prepared = self.prepare_domain(
            transaction.clone(),
            &canonical_request,
            blob_inventory,
            revision,
        )?;
        Ok(ImportPreview {
            base_revision,
            proposed_revision: revision,
            outcome: prepared.outcome.clone(),
            result_digest: prepared.result_digest,
        })
    }

    fn prepare_domain(
        &self,
        transaction: EngineTransaction,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<PreparedIngest, ImportError> {
        let expected_revision = match self.revision {
            Some(current) => current
                .checked_next()
                .map_err(|_| ImportError::VersionExhausted)?,
            None => CommitRevision::FIRST,
        };
        if transaction.scope() != self.scope || revision != expected_revision {
            return Err(ImportError::InvalidCheckpoint);
        }
        let mut candidate = self.clone();
        let request_digest: [u8; 32] = Sha256::digest(canonical_request).into();
        let (outcome, graph_digest, spatial_digest) = match transaction {
            EngineTransaction::Graph { graph_request, .. } => {
                if blob_inventory.is_some() {
                    return Err(ImportError::InventoryMismatch);
                }
                let graph_digest = apply_graph(
                    &mut candidate.graph,
                    self.scope,
                    &graph_request,
                    revision,
                    true,
                )?;
                validate_catalog_closure_import(
                    &candidate.graph.snapshot(),
                    candidate.spatial.snapshot().catalog(),
                )?;
                validate_job_closure(&candidate.graph.snapshot(), &candidate.jobs)?;
                (ImportBatchOutcome::GraphCommitted, graph_digest, None)
            }
            EngineTransaction::Import { action, .. } => match *action {
                ImportAction::StartJob(start) => {
                    candidate.prepare_start(&start, blob_inventory, request_digest, revision)?
                }
                ImportAction::ApplyBatch(batch) => {
                    candidate.prepare_batch(*batch, blob_inventory, request_digest, revision)?
                }
            },
        };
        candidate.revision = Some(revision);
        let result_digest = digest_result(
            revision,
            request_digest,
            graph_digest,
            spatial_digest,
            &outcome,
        );
        Ok(PreparedIngest {
            state: candidate,
            outcome,
            result_digest,
        })
    }

    fn prepare_start(
        &mut self,
        start: &ImportStart,
        blob_inventory: Option<&BlobInventory>,
        request_digest: [u8; 32],
        revision: CommitRevision,
    ) -> Result<ComponentOutcome, ImportError> {
        let job = start.job();
        let source = start.source();
        let mapping = start.mapping();
        require_scope(self.scope, job)?;
        require_binding_scope(self.scope, source, mapping)?;
        if self.jobs.contains_key(&job) {
            return Err(ImportError::JobExists);
        }
        if self.jobs.len() == MAX_IMPORT_JOBS {
            return Err(ImportError::ResourceLimit);
        }
        validate_inventory(blob_inventory, [source.blob(), mapping.blob()])?;
        let graph_digest = apply_graph(
            &mut self.graph,
            self.scope,
            start.graph_request(),
            revision,
            false,
        )?;
        let graph = self.graph.snapshot();
        require_active_entity(&graph, job)?;
        require_bound_evidence(&graph, source)?;
        require_bound_mapping(&graph, mapping)?;
        let chain_digest = digest_chain([0; 32], request_digest, revision, graph_digest, None);
        let checkpoint =
            ImportCheckpoint::started(job, source.clone(), mapping.clone(), revision, chain_digest);
        self.jobs.insert(
            job,
            JobState {
                checkpoint: checkpoint.clone(),
                batches: BTreeMap::new(),
                source_events: BTreeMap::new(),
            },
        );
        Ok((
            ImportBatchOutcome::JobStarted(checkpoint),
            graph_digest,
            None,
        ))
    }

    fn prepare_batch(
        &mut self,
        batch: ImportBatch,
        blob_inventory: Option<&BlobInventory>,
        request_digest: [u8; 32],
        revision: CommitRevision,
    ) -> Result<ComponentOutcome, ImportError> {
        if blob_inventory.is_some_and(|inventory| !inventory.is_empty()) {
            return Err(ImportError::InventoryMismatch);
        }
        let job_id = batch.id().job();
        require_scope(self.scope, job_id)?;
        let job = self.jobs.get(&job_id).ok_or(ImportError::MissingJob)?;
        let current_graph = self.graph.snapshot();
        require_active_entity(&current_graph, job_id)?;
        require_bound_evidence(&current_graph, job.checkpoint.source())?;
        require_bound_mapping(&current_graph, job.checkpoint.mapping())?;
        if batch.expected().source() != job.checkpoint.source()
            || batch.expected().mapping() != job.checkpoint.mapping()
        {
            return Err(ImportError::SourceChanged);
        }
        if batch.id().sequence() < job.checkpoint.next_batch() {
            return Err(ImportError::BatchConflict);
        }
        if batch.id().sequence() != job.checkpoint.next_batch()
            || batch.expected() != &job.checkpoint
            || batch.start() != job.checkpoint.cursor()
        {
            return Err(ImportError::InvalidCheckpoint);
        }
        if job.checkpoint.status() != ImportJobStatus::Open {
            return Err(ImportError::JobCompleted);
        }
        if self.batch_count == MAX_IMPORT_BATCHES {
            return Err(ImportError::ResourceLimit);
        }
        validate_rows(self.scope, &batch, &self.source_events)?;

        let graph_digest = apply_graph(
            &mut self.graph,
            self.scope,
            batch.graph_request(),
            revision,
            false,
        )?;
        let spatial_transaction = batch
            .spatial_request()
            .map(decode_spatial_transaction)
            .transpose()
            .map_err(|_| ImportError::InvalidEncoding)?;
        let spatial_digest = match batch.spatial_request() {
            Some(request) => {
                let prepared = self
                    .spatial
                    .prepare(request, None, revision)
                    .map_err(map_component_apply)?;
                let digest = SpatialState::result_digest(&prepared);
                self.spatial.publish(prepared);
                Some(digest)
            }
            None => None,
        };
        let graph = self.graph.snapshot();
        require_bound_evidence(&graph, job.checkpoint.source())?;
        require_bound_mapping(&graph, job.checkpoint.mapping())?;
        if let Some(transaction) = &spatial_transaction {
            validate_external_closure(&graph, transaction.records())?;
        }
        let previous_chain = job.checkpoint.chain_digest();
        let chain_digest = digest_chain(
            previous_chain,
            request_digest,
            revision,
            graph_digest,
            spatial_digest,
        );
        let row_count =
            u32::try_from(batch.rows().len()).map_err(|_| ImportError::ResourceLimit)?;
        let checkpoint = job.checkpoint.advanced(
            u64::from(row_count),
            revision,
            batch.final_batch(),
            chain_digest,
        )?;
        let job = self.jobs.get_mut(&job_id).ok_or(ImportError::MissingJob)?;
        for row in batch.rows() {
            job.source_events
                .insert(row.source_event(), row.declared_payload_digest());
            self.source_events.insert(row.source_event(), job_id);
        }
        job.batches.insert(
            batch.id().sequence(),
            BatchReceipt {
                request_digest,
                revision,
                row_count,
                checkpoint: checkpoint.clone(),
            },
        );
        job.checkpoint = checkpoint.clone();
        self.batch_count = self
            .batch_count
            .checked_add(1)
            .ok_or(ImportError::ResourceLimit)?;
        Ok((
            ImportBatchOutcome::BatchCommitted {
                id: batch.id(),
                revision,
                accepted: row_count,
                checkpoint,
            },
            graph_digest,
            spatial_digest,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct PreparedIngest {
    state: IngestState,
    outcome: ImportBatchOutcome,
    result_digest: [u8; 32],
}

impl PreparedIngest {
    #[must_use]
    pub const fn outcome(&self) -> &ImportBatchOutcome {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPreview {
    base_revision: Option<CommitRevision>,
    proposed_revision: CommitRevision,
    outcome: ImportBatchOutcome,
    result_digest: [u8; 32],
}

impl ImportPreview {
    #[must_use]
    pub const fn base_revision(&self) -> Option<CommitRevision> {
        self.base_revision
    }
    #[must_use]
    pub const fn proposed_revision(&self) -> CommitRevision {
        self.proposed_revision
    }
    #[must_use]
    pub const fn outcome(&self) -> &ImportBatchOutcome {
        &self.outcome
    }
    #[must_use]
    pub const fn result_digest(&self) -> [u8; 32] {
        self.result_digest
    }
}

impl TransactionState for IngestState {
    type Prepared = PreparedIngest;
    type Snapshot = EngineSnapshot;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        let transaction = decode_transaction(canonical_request).map_err(map_import_apply)?;
        self.prepare_domain(transaction, canonical_request, blob_inventory, revision)
            .map_err(map_import_apply)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.result_digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        *self = prepared.state;
    }

    fn snapshot(&self) -> Self::Snapshot {
        EngineSnapshot {
            scope: self.scope,
            revision: self.revision,
            graph: self.graph.snapshot(),
            spatial: self.spatial.snapshot(),
            jobs: self.jobs.clone(),
            batch_count: self.batch_count,
        }
    }
}

impl AuthorizedTransactionState for IngestState {
    const REQUIRES_DURABLE_POLICY: bool = true;

    fn authorization_requirements(
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let transaction = decode_transaction(canonical_request).map_err(map_import_apply)?;
        match transaction {
            EngineTransaction::Graph { graph_request, .. } => {
                GraphState::authorization_requirements(&graph_request, blob_inventory)
            }
            EngineTransaction::Import { scope, action } => {
                let mut requirements = BTreeSet::new();
                requirements.insert((Action::Import, Target::Namespace(scope)));
                requirements.insert((Action::Commit, Target::Namespace(scope)));
                collect_import_requirements(scope, &action, blob_inventory, &mut requirements)
                    .map_err(map_import_apply)?;
                AuthorizationRequirements::new(
                    requirements
                        .into_iter()
                        .map(|(action, target)| AuthorizationRequirement { action, target }),
                )
                .map_err(|_| ApplyError::ResourceLimit)
            }
        }
    }

    fn durable_namespace_policy(snapshot: &Self::Snapshot) -> Option<NamespacePolicy> {
        snapshot.graph.namespace_policy().cloned()
    }

    fn durable_policy_change(
        canonical_request: &[u8],
    ) -> Result<Option<DurablePolicyChange>, ApplyError> {
        match decode_transaction(canonical_request).map_err(map_import_apply)? {
            EngineTransaction::Graph { graph_request, .. } => {
                GraphState::durable_policy_change(&graph_request)
            }
            EngineTransaction::Import { .. } => Ok(None),
        }
    }
}

impl AuthorizedReadState for IngestState {
    type ReadRequest = ImportReadRequest;
    type ReadOutput = ImportReadOutput;
    type ReadError = ImportError;

    fn read_authorization_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let ImportReadRequest::Job { id } = request;
        AuthorizationRequirements::new([
            AuthorizationRequirement {
                action: Action::Import,
                target: Target::Namespace(NamespaceRef::new(id.database(), id.namespace())),
            },
            AuthorizationRequirement {
                action: Action::ReadRecord,
                target: Target::Record(*id),
            },
            AuthorizationRequirement {
                action: Action::ReadBlob,
                target: Target::Namespace(NamespaceRef::new(id.database(), id.namespace())),
            },
        ])
        .map_err(|_| ApplyError::ResourceLimit)
    }

    fn read_authorized(
        snapshot: &Self::Snapshot,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError> {
        let ImportReadRequest::Job { id } = request;
        require_scope(snapshot.scope, *id)?;
        let visible = authorize_candidate(Action::ReadRecord, Target::Record(*id));
        let Some(job) = visible.then(|| snapshot.job(*id)).flatten() else {
            return Ok(ImportReadOutput::Job(None));
        };
        let checkpoint = job.checkpoint();
        let bindings_visible =
            authorize_candidate(
                Action::ReadRecord,
                Target::Record(checkpoint.source().evidence()),
            ) && authorize_candidate(
                Action::ReadRecord,
                Target::Record(checkpoint.mapping().evidence()),
            ) && authorize_candidate(Action::ReadBlob, Target::Namespace(snapshot.scope));
        Ok(ImportReadOutput::Job(bindings_visible.then_some(job)))
    }
}

fn apply_graph(
    graph: &mut GraphState,
    scope: NamespaceRef,
    request: &[u8],
    revision: CommitRevision,
    allow_policy: bool,
) -> Result<[u8; 32], ImportError> {
    if request.is_empty() {
        return Err(ImportError::InvalidBatch);
    }
    let transaction =
        decode_graph_transaction(request).map_err(|_| ImportError::InvalidEncoding)?;
    if transaction.scope() != scope
        || (!allow_policy && transaction.policy_mutation().is_some())
        || (!allow_policy
            && transaction
                .operations()
                .iter()
                .any(|operation| matches!(operation, Operation::DeleteEntity { .. })))
    {
        return Err(ImportError::InvalidBatch);
    }
    let prepared = graph
        .prepare(request, None, revision)
        .map_err(map_component_apply)?;
    let digest = GraphState::result_digest(&prepared);
    graph.publish(prepared);
    Ok(digest)
}

fn validate_inventory(
    inventory: Option<&BlobInventory>,
    values: [BlobReference; 2],
) -> Result<(), ImportError> {
    let inventory = inventory.ok_or(ImportError::InventoryMismatch)?;
    let mut expected = values.to_vec();
    expected.sort_unstable();
    if expected
        .windows(2)
        .any(|pair| pair[0].id() == pair[1].id() && pair[0] != pair[1])
    {
        return Err(ImportError::InventoryMismatch);
    }
    expected.dedup();
    if inventory.references() == expected {
        Ok(())
    } else {
        Err(ImportError::InventoryMismatch)
    }
}

fn validate_rows(
    scope: NamespaceRef,
    batch: &ImportBatch,
    existing: &BTreeMap<SourceEventRef, RecordRef>,
) -> Result<(), ImportError> {
    let mut seen = BTreeSet::new();
    for row in batch.rows() {
        let event = row.source_event();
        if event.database() != scope.database()
            || event.namespace() != scope.namespace()
            || !seen.insert(event)
        {
            return Err(ImportError::SourceEventConflict);
        }
        if existing.contains_key(&event) {
            return Err(ImportError::SourceEventConflict);
        }
    }
    let rows = u64::try_from(batch.rows().len()).map_err(|_| ImportError::ResourceLimit)?;
    batch
        .start()
        .next_row()
        .checked_add(rows)
        .filter(|value| *value <= crate::MAX_IMPORT_ROWS)
        .ok_or(ImportError::ResourceLimit)?;
    Ok(())
}

fn require_binding_scope(
    scope: NamespaceRef,
    source: &SourceBinding,
    mapping: &MappingBinding,
) -> Result<(), ImportError> {
    require_scope(scope, source.evidence())?;
    require_scope(scope, mapping.evidence())?;
    if source.blob().scope() != scope || mapping.blob().scope() != scope {
        return Err(ImportError::ScopeMismatch);
    }
    Ok(())
}

fn require_bound_evidence(
    graph: &GraphSnapshot,
    binding: &SourceBinding,
) -> Result<(), ImportError> {
    match graph.record(binding.evidence()) {
        Some(Record::Evidence(record))
            if record.version.get() == binding.version()
                && record.digest == binding.blob().content_digest() =>
        {
            Ok(())
        }
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_bound_mapping(
    graph: &GraphSnapshot,
    binding: &MappingBinding,
) -> Result<(), ImportError> {
    match graph.record(binding.evidence()) {
        Some(Record::Evidence(record))
            if record.version.get() == binding.version()
                && record.digest == binding.blob().content_digest() =>
        {
            Ok(())
        }
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_bound_evidence_at(
    graph: &GraphSnapshot,
    revision: CommitRevision,
    binding: &SourceBinding,
) -> Result<(), ImportError> {
    match graph
        .record_at(revision, binding.evidence())
        .map_err(|_| ImportError::MissingReference)?
    {
        Some(Record::Evidence(record))
            if record.version.get() == binding.version()
                && record.digest == binding.blob().content_digest() =>
        {
            Ok(())
        }
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_bound_mapping_at(
    graph: &GraphSnapshot,
    revision: CommitRevision,
    binding: &MappingBinding,
) -> Result<(), ImportError> {
    match graph
        .record_at(revision, binding.evidence())
        .map_err(|_| ImportError::MissingReference)?
    {
        Some(Record::Evidence(record))
            if record.version.get() == binding.version()
                && record.digest == binding.blob().content_digest() =>
        {
            Ok(())
        }
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn validate_job_closure(
    graph: &GraphSnapshot,
    jobs: &BTreeMap<RecordRef, JobState>,
) -> Result<(), ImportError> {
    for (id, job) in jobs {
        require_active_entity(graph, *id)?;
        require_bound_evidence(graph, job.checkpoint.source())?;
        require_bound_mapping(graph, job.checkpoint.mapping())?;
    }
    Ok(())
}

fn validate_external_closure(
    graph: &GraphSnapshot,
    records: &[SpatialRecord],
) -> Result<(), ImportError> {
    for record in records {
        validate_external_record(graph, SpatialRecordRef::from(record))?;
    }
    Ok(())
}

fn validate_external_record(
    graph: &GraphSnapshot,
    record: SpatialRecordRef<'_>,
) -> Result<(), ImportError> {
    require_active_entity(graph, record.id())?;
    match record {
        SpatialRecordRef::World(value) => {
            require_active_entity(graph, value.root_frame().record)?;
        }
        SpatialRecordRef::Frame(value) => {
            require_active_entity(graph, value.world())?;
            if let Some(parent) = value.parent() {
                require_active_entity(graph, parent.frame.record)?;
                // T-49 proves graph existence only. T-50 validates transform schema/version.
                require_active_entity(graph, parent.transform.record)?;
            }
        }
        SpatialRecordRef::Geometry(value) => {
            require_active_entity(graph, value.world())?;
            require_active_entity(graph, value.frame().record)?;
            if let Some(predecessor) = value.predecessor() {
                require_active_entity(graph, predecessor.record)?;
            }
        }
        SpatialRecordRef::Observation(value) => {
            require_active_entity(graph, value.entity())?;
            require_active_entity(graph, value.world())?;
            require_active_entity(graph, value.frame().record)?;
            require_active_entity(graph, value.key().source())?;
            require_evidence(graph, value.evidence())?;
            if let Some(previous) = value.correction_of() {
                require_active_entity(graph, previous)?;
            }
        }
    }
    Ok(())
}

fn validate_external_record_at(
    graph: &GraphSnapshot,
    revision: CommitRevision,
    record: SpatialRecordRef<'_>,
) -> Result<(), ImportError> {
    require_active_entity_at(graph, revision, record.id())?;
    match record {
        SpatialRecordRef::World(value) => {
            require_active_entity_at(graph, revision, value.root_frame().record)?;
        }
        SpatialRecordRef::Frame(value) => {
            require_active_entity_at(graph, revision, value.world())?;
            if let Some(parent) = value.parent() {
                require_active_entity_at(graph, revision, parent.frame.record)?;
                require_active_entity_at(graph, revision, parent.transform.record)?;
            }
        }
        SpatialRecordRef::Geometry(value) => {
            require_active_entity_at(graph, revision, value.world())?;
            require_active_entity_at(graph, revision, value.frame().record)?;
            if let Some(predecessor) = value.predecessor() {
                require_active_entity_at(graph, revision, predecessor.record)?;
            }
        }
        SpatialRecordRef::Observation(value) => {
            require_active_entity_at(graph, revision, value.entity())?;
            require_active_entity_at(graph, revision, value.world())?;
            require_active_entity_at(graph, revision, value.frame().record)?;
            require_active_entity_at(graph, revision, value.key().source())?;
            require_evidence_at(graph, revision, value.evidence())?;
            if let Some(previous) = value.correction_of() {
                require_active_entity_at(graph, revision, previous)?;
            }
        }
    }
    Ok(())
}

fn require_active_entity(graph: &GraphSnapshot, id: RecordRef) -> Result<(), ImportError> {
    match graph.record(id) {
        Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active => Ok(()),
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_active_entity_at(
    graph: &GraphSnapshot,
    revision: CommitRevision,
    id: RecordRef,
) -> Result<(), ImportError> {
    match graph
        .record_at(revision, id)
        .map_err(|_| ImportError::MissingReference)?
    {
        Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active => Ok(()),
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_evidence(graph: &GraphSnapshot, id: RecordRef) -> Result<(), ImportError> {
    match graph.record(id) {
        Some(Record::Evidence(_)) => Ok(()),
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn require_evidence_at(
    graph: &GraphSnapshot,
    revision: CommitRevision,
    id: RecordRef,
) -> Result<(), ImportError> {
    match graph
        .record_at(revision, id)
        .map_err(|_| ImportError::MissingReference)?
    {
        Some(Record::Evidence(_)) => Ok(()),
        Some(_) => Err(ImportError::WrongReferenceKind),
        None => Err(ImportError::MissingReference),
    }
}

fn collect_import_requirements(
    scope: NamespaceRef,
    action: &ImportAction,
    blob_inventory: Option<&BlobInventory>,
    output: &mut BTreeSet<(Action, Target)>,
) -> Result<(), ImportError> {
    let (job, source, mapping, graph_request, spatial_request) = match action {
        ImportAction::StartJob(start) => {
            let job = start.job();
            let source = start.source();
            let mapping = start.mapping();
            validate_inventory(blob_inventory, [source.blob(), mapping.blob()])?;
            (job, source, mapping, start.graph_request(), None)
        }
        ImportAction::ApplyBatch(batch) => {
            if blob_inventory.is_some_and(|inventory| !inventory.is_empty()) {
                return Err(ImportError::InventoryMismatch);
            }
            (
                batch.id().job(),
                batch.expected().source(),
                batch.expected().mapping(),
                batch.graph_request(),
                batch.spatial_request(),
            )
        }
    };
    require_scope(scope, job)?;
    require_binding_scope(scope, source, mapping)?;
    output.insert((Action::Commit, Target::Record(job)));
    output.insert((Action::ReadRecord, Target::Record(job)));
    output.insert((Action::ReadRecord, Target::Record(source.evidence())));
    output.insert((Action::ReadRecord, Target::Record(mapping.evidence())));

    if graph_request.is_empty() {
        return Err(ImportError::InvalidBatch);
    }
    let graph =
        decode_graph_transaction(graph_request).map_err(|_| ImportError::InvalidEncoding)?;
    if graph.scope() != scope
        || graph.policy_mutation().is_some()
        || graph
            .operations()
            .iter()
            .any(|operation| matches!(operation, Operation::DeleteEntity { .. }))
    {
        return Err(ImportError::InvalidBatch);
    }
    for requirement in GraphState::authorization_requirements(graph_request, None)
        .map_err(map_apply_import)?
        .iter()
    {
        output.insert((requirement.action, requirement.target));
    }
    if let Some(bytes) = spatial_request {
        let transaction =
            decode_spatial_transaction(bytes).map_err(|_| ImportError::InvalidEncoding)?;
        if transaction.scope() != scope {
            return Err(ImportError::ScopeMismatch);
        }
        for record in transaction.records() {
            collect_spatial_requirements(scope, record, output)?;
        }
    }
    Ok(())
}

fn collect_spatial_requirements(
    scope: NamespaceRef,
    record: &SpatialRecord,
    output: &mut BTreeSet<(Action, Target)>,
) -> Result<(), ImportError> {
    let mut read = BTreeSet::new();
    read.insert(record.id());
    match record {
        SpatialRecord::World(value) => {
            read.insert(value.root_frame().record);
        }
        SpatialRecord::Frame(value) => {
            read.insert(value.world());
            if let Some(parent) = value.parent() {
                read.insert(parent.frame.record);
                read.insert(parent.transform.record);
            }
        }
        SpatialRecord::Geometry(value) => {
            read.insert(value.world());
            read.insert(value.frame().record);
            if let Some(predecessor) = value.predecessor() {
                read.insert(predecessor.record);
            }
        }
        SpatialRecord::Observation(value) => {
            read.insert(value.entity());
            read.insert(value.world());
            read.insert(value.frame().record);
            read.insert(value.key().source());
            read.insert(value.evidence());
            if let Some(previous) = value.correction_of() {
                read.insert(previous);
            }
        }
    }
    output.insert((Action::Commit, Target::Record(record.id())));
    for id in read {
        require_scope(scope, id)?;
        output.insert((Action::ReadRecord, Target::Record(id)));
    }
    Ok(())
}

fn require_scope(scope: NamespaceRef, id: RecordRef) -> Result<(), ImportError> {
    if id.database() == scope.database() && id.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(ImportError::ScopeMismatch)
    }
}

fn digest_chain(
    previous: [u8; 32],
    request: [u8; 32],
    revision: CommitRevision,
    graph: [u8; 32],
    spatial: Option<[u8; 32]>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"USTE-IMPORT-CHAIN-V1\0");
    digest.update(previous);
    digest.update(request);
    digest.update(revision.get().to_be_bytes());
    digest.update(graph);
    digest.update(spatial.unwrap_or([0; 32]));
    digest.finalize().into()
}

fn digest_result(
    revision: CommitRevision,
    request: [u8; 32],
    graph: [u8; 32],
    spatial: Option<[u8; 32]>,
    outcome: &ImportBatchOutcome,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"USTE-INGEST-RESULT-V1\0");
    digest.update(revision.get().to_be_bytes());
    digest.update(request);
    digest.update(graph);
    digest.update(spatial.unwrap_or([0; 32]));
    match outcome {
        ImportBatchOutcome::GraphCommitted => digest.update([0]),
        ImportBatchOutcome::JobStarted(checkpoint) => {
            digest.update([1]);
            digest.update(checkpoint.chain_digest());
        }
        ImportBatchOutcome::BatchCommitted {
            id,
            revision,
            accepted,
            checkpoint,
        } => {
            digest.update([2]);
            digest.update(id.sequence().to_be_bytes());
            digest.update(revision.get().to_be_bytes());
            digest.update(accepted.to_be_bytes());
            digest.update(checkpoint.chain_digest());
        }
    }
    digest.finalize().into()
}

const fn map_import_apply(error: ImportError) -> ApplyError {
    match error {
        ImportError::SourceChanged => ApplyError::SourceChanged,
        ImportError::JobExists
        | ImportError::BatchConflict
        | ImportError::InvalidCheckpoint
        | ImportError::SourceEventConflict => ApplyError::Conflict,
        ImportError::ResourceLimit | ImportError::VersionExhausted => ApplyError::ResourceLimit,
        _ => ApplyError::InvalidRequest,
    }
}

const fn map_component_apply(error: ApplyError) -> ImportError {
    match error {
        ApplyError::Conflict => ImportError::BatchConflict,
        ApplyError::SourceChanged => ImportError::SourceChanged,
        ApplyError::ResourceLimit => ImportError::ResourceLimit,
        ApplyError::InvalidRequest | ApplyError::UnsupportedPredicate => ImportError::InvalidBatch,
    }
}

const fn map_apply_import(error: ApplyError) -> ImportError {
    map_component_apply(error)
}

impl CheckpointState for IngestState {
    const REDUCER_PROFILE: [u8; 32] = INGEST_REDUCER_PROFILE;

    fn checkpoint_scope(snapshot: &Self::Snapshot) -> NamespaceRef {
        snapshot.scope
    }

    fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
        snapshot.revision
    }

    fn logical_state_digest(snapshot: &Self::Snapshot) -> Result<[u8; 32], CheckpointStateError> {
        let mut digest = Sha256::new();
        digest.update(b"USTE-INGEST-LOGICAL-STATE-V1\0");
        encode_scope_to_digest(&mut digest, snapshot.scope);
        digest.update(
            snapshot
                .revision
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        digest.update(GraphState::logical_state_digest(&snapshot.graph)?);
        digest.update(SpatialState::logical_state_digest(&snapshot.spatial)?);
        digest.update(
            u64::try_from(snapshot.jobs.len())
                .map_err(|_| CheckpointStateError::ResourceLimit)?
                .to_be_bytes(),
        );
        digest.update(
            u64::try_from(snapshot.batch_count)
                .map_err(|_| CheckpointStateError::ResourceLimit)?
                .to_be_bytes(),
        );
        for job in snapshot.jobs.values() {
            digest_checkpoint(&mut digest, &job.checkpoint)?;
            digest.update(
                u64::try_from(job.batches.len())
                    .map_err(|_| CheckpointStateError::ResourceLimit)?
                    .to_be_bytes(),
            );
            for (sequence, receipt) in &job.batches {
                digest.update(sequence.to_be_bytes());
                digest.update(receipt.request_digest);
                digest.update(receipt.revision.get().to_be_bytes());
                digest.update(receipt.row_count.to_be_bytes());
                digest_checkpoint(&mut digest, &receipt.checkpoint)?;
            }
            digest.update(
                u64::try_from(job.source_events.len())
                    .map_err(|_| CheckpointStateError::ResourceLimit)?
                    .to_be_bytes(),
            );
            for (event, mapped_digest) in &job.source_events {
                digest_source_event(&mut digest, *event);
                digest.update(mapped_digest);
            }
        }
        Ok(digest.finalize().into())
    }

    fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
        encode_ingest_checkpoint(snapshot)
    }

    fn decode_checkpoint(
        scope: NamespaceRef,
        revision: CommitRevision,
        encoded: &[u8],
    ) -> Result<Self, CheckpointStateError> {
        decode_ingest_checkpoint(scope, revision, encoded)
    }
}

fn encode_ingest_checkpoint(snapshot: &EngineSnapshot) -> Result<Vec<u8>, CheckpointStateError> {
    let revision = snapshot.revision.ok_or(CheckpointStateError::Invalid)?;
    if snapshot.jobs.len() > MAX_IMPORT_JOBS || snapshot.batch_count > MAX_IMPORT_BATCHES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let graph = GraphState::encode_checkpoint(&snapshot.graph)?;
    let (spatial_revision, spatial) = match snapshot.spatial.revision() {
        Some(value) => (
            value.get(),
            SpatialState::encode_checkpoint(&snapshot.spatial)?,
        ),
        None => (0, Vec::new()),
    };
    let mut output = Vec::new();
    checkpoint_extend(&mut output, INGEST_CHECKPOINT_MAGIC)?;
    checkpoint_scope(&mut output, snapshot.scope)?;
    checkpoint_u64(&mut output, revision.get())?;
    checkpoint_u64(&mut output, spatial_revision)?;
    checkpoint_frame(&mut output, &graph)?;
    checkpoint_frame(&mut output, &spatial)?;
    checkpoint_u64_from_usize(&mut output, snapshot.jobs.len())?;
    checkpoint_u64_from_usize(&mut output, snapshot.batch_count)?;
    for job in snapshot.jobs.values() {
        checkpoint_import_checkpoint(&mut output, &job.checkpoint)?;
        checkpoint_u64_from_usize(&mut output, job.batches.len())?;
        for (sequence, receipt) in &job.batches {
            checkpoint_u64(&mut output, *sequence)?;
            checkpoint_extend(&mut output, &receipt.request_digest)?;
            checkpoint_u64(&mut output, receipt.revision.get())?;
            checkpoint_u32(&mut output, receipt.row_count)?;
            checkpoint_extend(&mut output, &[0; 4])?;
            checkpoint_import_checkpoint(&mut output, &receipt.checkpoint)?;
        }
        checkpoint_u64_from_usize(&mut output, job.source_events.len())?;
        for (event, mapped_digest) in &job.source_events {
            checkpoint_source_event(&mut output, *event)?;
            checkpoint_extend(&mut output, mapped_digest)?;
        }
    }
    Ok(output)
}

fn decode_ingest_checkpoint(
    expected_scope: NamespaceRef,
    expected_revision: CommitRevision,
    encoded: &[u8],
) -> Result<IngestState, CheckpointStateError> {
    if encoded.len() > MAX_CHECKPOINT_BYTES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let mut cursor = Cursor::new(encoded);
    if cursor.take(8).map_err(map_import_checkpoint)? != INGEST_CHECKPOINT_MAGIC {
        return Err(CheckpointStateError::UnsupportedProfile);
    }
    let scope = decode_scope(&mut cursor).map_err(map_import_checkpoint)?;
    let revision = checkpoint_revision(&mut cursor)?;
    let spatial_revision = cursor.u64().map_err(map_import_checkpoint)?;
    if scope != expected_scope || revision != expected_revision || spatial_revision > revision.get()
    {
        return Err(CheckpointStateError::Invalid);
    }
    let graph_bytes = cursor.frame().map_err(map_import_checkpoint)?;
    let spatial_bytes = cursor.frame().map_err(map_import_checkpoint)?;
    let graph = GraphState::decode_checkpoint(scope, revision, graph_bytes)?;
    let spatial = if spatial_revision == 0 {
        if !spatial_bytes.is_empty() {
            return Err(CheckpointStateError::Invalid);
        }
        SpatialState::new(scope)
    } else {
        let spatial_revision =
            CommitRevision::new(spatial_revision).map_err(|_| CheckpointStateError::Invalid)?;
        SpatialState::decode_checkpoint(scope, spatial_revision, spatial_bytes)?
    };
    let job_count = checkpoint_count(&mut cursor, MAX_IMPORT_JOBS)?;
    let declared_batch_count = checkpoint_count(&mut cursor, MAX_IMPORT_BATCHES)?;
    if job_count > cursor.remaining() / 464 {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let graph_snapshot = graph.snapshot();
    let mut jobs = BTreeMap::new();
    let mut source_event_index = BTreeMap::new();
    let mut previous_job = None;
    let mut actual_batch_count = 0_usize;
    let mut import_revisions = BTreeSet::new();
    let mut batch_revisions = BTreeSet::new();
    for _ in 0..job_count {
        let checkpoint = decode_import_checkpoint(&mut cursor).map_err(map_import_checkpoint)?;
        let job_id = checkpoint.job();
        if previous_job.is_some_and(|previous| previous >= job_id)
            || require_scope(scope, job_id).is_err()
            || require_binding_scope(scope, checkpoint.source(), checkpoint.mapping()).is_err()
            || checkpoint.last_revision() > revision
            || require_active_entity(&graph_snapshot, job_id).is_err()
            || require_bound_evidence(&graph_snapshot, checkpoint.source()).is_err()
            || require_bound_mapping(&graph_snapshot, checkpoint.mapping()).is_err()
            || require_active_entity_at(&graph_snapshot, checkpoint.first_revision(), job_id)
                .is_err()
            || require_bound_evidence_at(
                &graph_snapshot,
                checkpoint.first_revision(),
                checkpoint.source(),
            )
            .is_err()
            || require_bound_mapping_at(
                &graph_snapshot,
                checkpoint.first_revision(),
                checkpoint.mapping(),
            )
            .is_err()
        {
            return Err(CheckpointStateError::Invalid);
        }
        if !import_revisions.insert(checkpoint.first_revision()) {
            return Err(CheckpointStateError::Invalid);
        }
        previous_job = Some(job_id);
        let batch_count = checkpoint_count(&mut cursor, MAX_IMPORT_BATCHES)?;
        actual_batch_count = actual_batch_count
            .checked_add(batch_count)
            .ok_or(CheckpointStateError::ResourceLimit)?;
        if actual_batch_count > MAX_IMPORT_BATCHES || batch_count > cursor.remaining() / 504 {
            return Err(CheckpointStateError::ResourceLimit);
        }
        let mut batches = BTreeMap::new();
        let mut accepted_rows = 0_u64;
        let mut previous_revision = checkpoint.first_revision();
        let mut last_receipt = None;
        for index in 0..batch_count {
            let sequence = cursor.u64().map_err(map_import_checkpoint)?;
            let expected_sequence = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(CheckpointStateError::ResourceLimit)?;
            let request_digest = cursor.array().map_err(map_import_checkpoint)?;
            let receipt_revision = checkpoint_revision(&mut cursor)?;
            let row_count = cursor.u32().map_err(map_import_checkpoint)?;
            cursor.zeros(4).map_err(map_import_checkpoint)?;
            let receipt_checkpoint =
                decode_import_checkpoint(&mut cursor).map_err(map_import_checkpoint)?;
            accepted_rows = accepted_rows
                .checked_add(u64::from(row_count))
                .ok_or(CheckpointStateError::ResourceLimit)?;
            if sequence != expected_sequence
                || row_count == 0
                || usize::try_from(row_count)
                    .ok()
                    .is_none_or(|count| count > crate::MAX_ROWS_PER_BATCH)
                || receipt_revision <= previous_revision
                || receipt_revision > revision
                || receipt_checkpoint.job() != job_id
                || receipt_checkpoint.source() != checkpoint.source()
                || receipt_checkpoint.mapping() != checkpoint.mapping()
                || receipt_checkpoint.version() != sequence.saturating_add(1)
                || receipt_checkpoint.next_batch() != sequence.saturating_add(1)
                || receipt_checkpoint.accepted_rows() != accepted_rows
                || receipt_checkpoint.cursor().next_row() != accepted_rows
                || receipt_checkpoint.first_revision() != checkpoint.first_revision()
                || receipt_checkpoint.last_revision() != receipt_revision
                || (index + 1 < batch_count && receipt_checkpoint.status() != ImportJobStatus::Open)
                || !import_revisions.insert(receipt_revision)
            {
                return Err(CheckpointStateError::Invalid);
            }
            previous_revision = receipt_revision;
            batch_revisions.insert(receipt_revision);
            last_receipt = Some(receipt_checkpoint.clone());
            batches.insert(
                sequence,
                BatchReceipt {
                    request_digest,
                    revision: receipt_revision,
                    row_count,
                    checkpoint: receipt_checkpoint,
                },
            );
        }
        if let Some(last) = last_receipt {
            if last != checkpoint {
                return Err(CheckpointStateError::Invalid);
            }
        } else if checkpoint.version() != 1
            || checkpoint.next_batch() != 1
            || checkpoint.accepted_rows() != 0
            || checkpoint.cursor() != crate::ImportCursor::START
            || checkpoint.first_revision() != checkpoint.last_revision()
            || checkpoint.status() != ImportJobStatus::Open
        {
            return Err(CheckpointStateError::Invalid);
        }
        let event_count_u64 = cursor.u64().map_err(map_import_checkpoint)?;
        if event_count_u64 != accepted_rows {
            return Err(CheckpointStateError::Invalid);
        }
        let event_count =
            usize::try_from(event_count_u64).map_err(|_| CheckpointStateError::ResourceLimit)?;
        if event_count > cursor.remaining() / 80 {
            return Err(CheckpointStateError::ResourceLimit);
        }
        let mut source_events = BTreeMap::new();
        let mut previous_event = None;
        for _ in 0..event_count {
            let event = decode_source_event(&mut cursor).map_err(map_import_checkpoint)?;
            let mapped_digest = cursor.array().map_err(map_import_checkpoint)?;
            if event.database() != scope.database()
                || event.namespace() != scope.namespace()
                || previous_event.is_some_and(|previous| previous >= event)
                || source_event_index.contains_key(&event)
            {
                return Err(CheckpointStateError::Invalid);
            }
            previous_event = Some(event);
            source_events.insert(event, mapped_digest);
            source_event_index.insert(event, job_id);
        }
        jobs.insert(
            job_id,
            JobState {
                checkpoint,
                batches,
                source_events,
            },
        );
    }
    if cursor.remaining() != 0 || actual_batch_count != declared_batch_count {
        return Err(CheckpointStateError::Invalid);
    }
    validate_catalog_closure(
        &graph_snapshot,
        spatial.snapshot().catalog(),
        &batch_revisions,
    )?;
    Ok(IngestState {
        scope,
        revision: Some(revision),
        graph,
        spatial,
        jobs,
        source_events: source_event_index,
        batch_count: actual_batch_count,
    })
}

fn validate_catalog_closure(
    graph: &GraphSnapshot,
    catalog: &uste_spatial::SpatialCatalog,
    batch_revisions: &BTreeSet<CommitRevision>,
) -> Result<(), CheckpointStateError> {
    catalog.visit_records(|recorded_revision, record| {
        if !batch_revisions.contains(&recorded_revision) {
            return Err(CheckpointStateError::Invalid);
        }
        validate_external_record(graph, record).map_err(|_| CheckpointStateError::Invalid)?;
        validate_external_record_at(graph, recorded_revision, record)
            .map_err(|_| CheckpointStateError::Invalid)
    })
}

fn validate_catalog_closure_import(
    graph: &GraphSnapshot,
    catalog: &uste_spatial::SpatialCatalog,
) -> Result<(), ImportError> {
    catalog.visit_records(|_, record| validate_external_record(graph, record))
}

fn checkpoint_import_checkpoint(
    output: &mut Vec<u8>,
    checkpoint: &ImportCheckpoint,
) -> Result<(), CheckpointStateError> {
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(448)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    encode_import_checkpoint(&mut encoded, checkpoint);
    checkpoint_extend(output, &encoded)
}

fn checkpoint_source_event(
    output: &mut Vec<u8>,
    event: SourceEventRef,
) -> Result<(), CheckpointStateError> {
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(48)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    encode_source_event(&mut encoded, event);
    checkpoint_extend(output, &encoded)
}

fn checkpoint_scope(output: &mut Vec<u8>, scope: NamespaceRef) -> Result<(), CheckpointStateError> {
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(32)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    encode_scope(&mut encoded, scope);
    checkpoint_extend(output, &encoded)
}

fn checkpoint_frame(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CheckpointStateError> {
    checkpoint_u64_from_usize(output, bytes.len())?;
    checkpoint_extend(output, bytes)
}

fn checkpoint_u64_from_usize(
    output: &mut Vec<u8>,
    value: usize,
) -> Result<(), CheckpointStateError> {
    checkpoint_u64(
        output,
        u64::try_from(value).map_err(|_| CheckpointStateError::ResourceLimit)?,
    )
}

fn checkpoint_u64(output: &mut Vec<u8>, value: u64) -> Result<(), CheckpointStateError> {
    checkpoint_extend(output, &value.to_be_bytes())
}

fn checkpoint_u32(output: &mut Vec<u8>, value: u32) -> Result<(), CheckpointStateError> {
    checkpoint_extend(output, &value.to_be_bytes())
}

fn checkpoint_extend(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CheckpointStateError> {
    let next = output
        .len()
        .checked_add(bytes.len())
        .ok_or(CheckpointStateError::ResourceLimit)?;
    if next > MAX_CHECKPOINT_BYTES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    output
        .try_reserve(bytes.len())
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn checkpoint_count(
    cursor: &mut Cursor<'_>,
    maximum: usize,
) -> Result<usize, CheckpointStateError> {
    let value = usize::try_from(cursor.u64().map_err(map_import_checkpoint)?)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    if value > maximum {
        return Err(CheckpointStateError::ResourceLimit);
    }
    Ok(value)
}

fn checkpoint_revision(cursor: &mut Cursor<'_>) -> Result<CommitRevision, CheckpointStateError> {
    CommitRevision::new(cursor.u64().map_err(map_import_checkpoint)?)
        .map_err(|_| CheckpointStateError::Invalid)
}

fn digest_checkpoint(
    digest: &mut Sha256,
    checkpoint: &ImportCheckpoint,
) -> Result<(), CheckpointStateError> {
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(448)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    encode_import_checkpoint(&mut encoded, checkpoint);
    digest.update(encoded);
    Ok(())
}

fn digest_source_event(digest: &mut Sha256, event: SourceEventRef) {
    digest.update(event.database().as_bytes());
    digest.update(event.namespace().as_bytes());
    digest.update(event.source_event().as_bytes());
}

fn encode_scope_to_digest(digest: &mut Sha256, scope: NamespaceRef) {
    digest.update(scope.database().as_bytes());
    digest.update(scope.namespace().as_bytes());
}

const fn map_import_checkpoint(error: ImportError) -> CheckpointStateError {
    match error {
        ImportError::UnsupportedProfile => CheckpointStateError::UnsupportedProfile,
        ImportError::ResourceLimit | ImportError::VersionExhausted => {
            CheckpointStateError::ResourceLimit
        }
        _ => CheckpointStateError::Invalid,
    }
}
