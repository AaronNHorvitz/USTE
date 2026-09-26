//! Producer implementation of the `uste-memory-consumer` 1.0 contract over the authorized,
//! durable research-memory coordinator.
//!
//! The consumer's trusted adapter supplies the policy kernel and authenticated principal; this
//! producer never mints authority. It exposes no raw coordinator, filesystem, snapshot or blob
//! capability. Every write goes through the ordinary authorized journal path; every read goes
//! through the mandatory authorization facade with cooperative cancellation.

use sha2::{Digest, Sha256};
use uste_crypto::{EntropySource, KeyAdapter, KeyVault};
use uste_memory::{
    FetchOutcome, RESEARCH_PROFILE, ResearchMutation, ResearchReadError, ResearchReadOutput,
    ResearchReadRequest, ResearchRecord, ResearchState, ResearchTransaction,
    contract::{
        ContractError, ContractSource, ContractVersion, ContractWrite, DynCancellation,
        MEMORY_CONTRACT_VERSION, MemoryConsumerContract, OperationId, WriteReceipt,
        source_matches_view, source_record,
    },
    encode_research_transaction,
};
use uste_policy::{AuthenticatedPrincipal, PolicyKernel};
use uste_storage::{
    BlobInventory, BlobReference, Clock, EntryName, OwnershipFileSystem,
    journal::DurableKeyEnvelope,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedError, AuthorizedReadError, AuthorizedReadView,
    AuthorizedTransactionRequest, Cancellation, CommitCoordinator, RetentionDays, TransactionError,
    open_authorized,
};
use uste_types::{IdempotencyKey, NamespaceRef, TransactionId};

/// One namespace, one principal, one owning process.
pub struct ResearchMemoryProducer<F, W, E, I, C>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    C: Clock,
{
    filesystem: F,
    coordinator: AuthorizedCoordinator<ResearchState, F, W, E, I>,
    principal: AuthenticatedPrincipal,
    clock: C,
}

impl<F, W, E, I, C> ResearchMemoryProducer<F, W, E, I, C>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    C: Clock,
{
    /// Create a new, empty research store.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        mut filesystem: F,
        name: EntryName,
        scope: NamespaceRef,
        retention: RetentionDays,
        vault: KeyVault<W, E>,
        identity_entropy: I,
        kernel: PolicyKernel,
        principal: AuthenticatedPrincipal,
        clock: C,
    ) -> Result<Self, ContractError> {
        let raw = CommitCoordinator::create(
            &mut filesystem,
            scope,
            retention,
            name,
            vault,
            identity_entropy,
            ResearchState::new(scope),
        )
        .map_err(transaction_error)?;
        let coordinator = AuthorizedCoordinator::new(raw, kernel).map_err(authorized_error)?;
        Ok(Self {
            filesystem,
            coordinator,
            principal,
            clock,
        })
    }

    /// Reopen an existing store. Contract 1.0 keeps no upload outbox: any upload interrupted by a
    /// crash was never committed, so reconciliation completes with an empty outbox and the
    /// abandoned staged bytes remain unreferenced storage for later reclamation.
    #[allow(clippy::too_many_arguments)]
    pub fn open<A>(
        mut filesystem: F,
        name: &EntryName,
        scope: NamespaceRef,
        retention: RetentionDays,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        kernel: PolicyKernel,
        principal: AuthenticatedPrincipal,
        clock: C,
    ) -> Result<Self, ContractError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let (mut coordinator, _) = open_authorized(
            &mut filesystem,
            name,
            scope,
            retention,
            vault_entropy,
            identity_entropy,
            key_adapter,
            ResearchState::new(scope),
            kernel,
        )
        .map_err(authorized_error)?;
        coordinator
            .complete_recovered_upload_reconciliation(&mut filesystem, &principal, &[])
            .map_err(authorized_error)?;
        Ok(Self {
            filesystem,
            coordinator,
            principal,
            clock,
        })
    }

    /// Release the owned filesystem, for example to simulate process loss in tests.
    pub fn into_filesystem(self) -> F {
        self.filesystem
    }

    fn scope(&self) -> NamespaceRef {
        self.coordinator.scope()
    }

    fn commit(
        &mut self,
        operation: OperationId,
        transaction: &ResearchTransaction,
        inventory: Option<&BlobInventory>,
        cancellation: &dyn Cancellation,
    ) -> Result<WriteReceipt, ContractError> {
        let bytes = encode_research_transaction(transaction).map_err(codec_error)?;
        let outcome = self
            .coordinator
            .commit(
                &mut self.filesystem,
                &self.principal,
                AuthorizedTransactionRequest {
                    idempotency_key: IdempotencyKey::from_bytes(operation.idempotency_key),
                    transaction_id: TransactionId::from_bytes(operation.transaction_id),
                    canonical_request: &bytes,
                    blob_inventory: inventory,
                },
                &mut self.clock,
                &DynCancellation(cancellation),
            )
            .map_err(authorized_error)?;
        Ok(WriteReceipt {
            revision: Some(outcome.revision),
        })
    }

    /// Answer a retried source write from its recorded outcome, verifying that the stored
    /// version is exactly what is requested, so the bytes are never stored twice.
    fn retried_source(
        &mut self,
        operation: OperationId,
        generation: u64,
        source: &ContractSource,
    ) -> Result<Option<WriteReceipt>, ContractError> {
        let Some(outcome) = self
            .coordinator
            .outcome(
                &self.principal,
                IdempotencyKey::from_bytes(operation.idempotency_key),
                &mut self.clock,
            )
            .map_err(authorized_error)?
        else {
            return Ok(None);
        };
        if outcome.transaction_id != TransactionId::from_bytes(operation.transaction_id) {
            return Err(ContractError::Conflict);
        }
        let view = self.view()?;
        let output = self.read(
            &view,
            &ResearchReadRequest::SourceVersion {
                authority_generation: generation,
                id: source.id,
                evaluated_at: source.retrieved_at,
            },
            &uste_txn::NeverCancel,
        );
        let matches = matches!(
            output,
            Ok(ResearchReadOutput::SourceVersion(stored))
                if stored.recorded_revision == outcome.revision
                    && source_matches_view(source, &stored)
        );
        if matches {
            Ok(Some(WriteReceipt {
                revision: Some(outcome.revision),
            }))
        } else {
            Err(ContractError::Conflict)
        }
    }

    fn store_content(&mut self, bytes: &[u8]) -> Result<BlobReference, ContractError> {
        let mut upload = self
            .coordinator
            .start_blob_upload(&self.principal)
            .map_err(authorized_error)?;
        self.coordinator
            .write_blob_upload(&mut self.filesystem, &self.principal, &mut upload, bytes)
            .map_err(authorized_error)?;
        let blob = self
            .coordinator
            .finish_blob_upload(&mut self.filesystem, &self.principal, &mut upload)
            .map_err(authorized_error)?;
        if blob.byte_len()
            != u64::try_from(bytes.len()).map_err(|_| ContractError::ResourceLimit)?
            || blob.content_digest() != <[u8; 32]>::from(Sha256::digest(bytes))
        {
            return Err(ContractError::IntegrityFailure);
        }
        Ok(blob)
    }

    fn put_source(
        &mut self,
        operation: OperationId,
        generation: u64,
        source: &ContractSource,
        cancellation: &dyn Cancellation,
    ) -> Result<WriteReceipt, ContractError> {
        let inaccessible = matches!(source.outcome, FetchOutcome::Inaccessible { .. });
        if inaccessible == source.content.is_some() {
            return Err(ContractError::InvalidRequest);
        }
        if let Some(content) = &source.content
            && u64::try_from(content.bytes.len()).map_err(|_| ContractError::ResourceLimit)?
                > RESEARCH_PROFILE.maximum_source_bytes_per_version
        {
            return Err(ContractError::ResourceLimit);
        }
        if let Some(receipt) = self.retried_source(operation, generation, source)? {
            return Ok(receipt);
        }
        // Validate the record without content before storing any bytes.
        let described = ResearchRecord::Source(source_record(source, None));
        if source.content.is_none() {
            described.validate(self.scope()).map_err(codec_error)?;
        }
        if cancellation.is_cancelled() {
            return Err(ContractError::Cancelled);
        }
        let blob = match &source.content {
            Some(content) => Some(self.store_content(&content.bytes)?),
            None => None,
        };
        let inventory = blob
            .map(|blob| BlobInventory::new(self.scope(), [blob]))
            .transpose()
            .map_err(|_| ContractError::IntegrityFailure)?;
        let transaction = ResearchTransaction {
            scope: self.scope(),
            generation,
            mutation: ResearchMutation::Put(Box::new(ResearchRecord::Source(source_record(
                source, blob,
            )))),
        };
        self.commit(operation, &transaction, inventory.as_ref(), cancellation)
    }
}

impl<F, W, E, I, C> MemoryConsumerContract for ResearchMemoryProducer<F, W, E, I, C>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    C: Clock,
{
    type View = AuthorizedReadView<ResearchState>;

    fn contract_version(&self) -> ContractVersion {
        MEMORY_CONTRACT_VERSION
    }

    fn write(
        &mut self,
        operation: OperationId,
        request: &ContractWrite,
        cancellation: &dyn Cancellation,
    ) -> Result<WriteReceipt, ContractError> {
        let scope = self.scope();
        let (generation, mutation) = match request {
            ContractWrite::PutSource { generation, source } => {
                return self.put_source(operation, *generation, source, cancellation);
            }
            ContractWrite::ReplacePolicy {
                expected_version,
                next,
            } => {
                if cancellation.is_cancelled() {
                    return Err(ContractError::Cancelled);
                }
                self.coordinator
                    .replace_namespace_policy(&self.principal, *expected_version, next.clone())
                    .map_err(authorized_error)?;
                return Ok(WriteReceipt { revision: None });
            }
            ContractWrite::BeginGeneration { next_generation } => (
                *next_generation,
                ResearchMutation::BeginRebuild {
                    next_generation: *next_generation,
                },
            ),
            ContractWrite::PutArtifact {
                generation,
                artifact,
            } => {
                if artifact.content.is_some() {
                    return Err(ContractError::InvalidRequest);
                }
                (
                    *generation,
                    ResearchMutation::Put(Box::new(ResearchRecord::Artifact(artifact.clone()))),
                )
            }
            ContractWrite::PutClaim { generation, claim } => (
                *generation,
                ResearchMutation::Put(Box::new(ResearchRecord::Claim(claim.clone()))),
            ),
            ContractWrite::PutEdge { generation, edge } => (
                *generation,
                ResearchMutation::Put(Box::new(ResearchRecord::Edge(edge.clone()))),
            ),
            ContractWrite::RetractClaim { generation, claim } => (
                *generation,
                ResearchMutation::RetractClaim { target: *claim },
            ),
            ContractWrite::ExpireClaim { generation, claim } => (
                *generation,
                ResearchMutation::ExpireClaim { target: *claim },
            ),
            ContractWrite::RevokeSource { generation, source } => (
                *generation,
                ResearchMutation::RevokeSource { source: *source },
            ),
            ContractWrite::CompleteGeneration { generation } => {
                (*generation, ResearchMutation::CompleteRebuild)
            }
        };
        self.commit(
            operation,
            &ResearchTransaction {
                scope,
                generation,
                mutation,
            },
            None,
            cancellation,
        )
    }

    fn view(&self) -> Result<Self::View, ContractError> {
        self.coordinator
            .read_view(&self.principal)
            .map_err(authorized_error)
    }

    fn read(
        &self,
        view: &Self::View,
        request: &ResearchReadRequest,
        cancellation: &dyn Cancellation,
    ) -> Result<ResearchReadOutput, ContractError> {
        self.coordinator
            .read_cancellable(
                &self.principal,
                view,
                request,
                &DynCancellation(cancellation),
            )
            .map_err(|error| match error {
                AuthorizedReadError::Authorization(error) => authorized_error(error),
                AuthorizedReadError::Domain(error) => read_error(error),
            })
    }
}

const fn transaction_error(error: TransactionError) -> ContractError {
    match error {
        TransactionError::Conflict => ContractError::Conflict,
        TransactionError::SourceChanged => ContractError::SourceChanged,
        TransactionError::InvalidRequest => ContractError::InvalidRequest,
        TransactionError::ResourceLimit | TransactionError::RevisionExhausted => {
            ContractError::ResourceLimit
        }
        TransactionError::UnsupportedPredicate => ContractError::UnsupportedQuery,
        TransactionError::Cancelled => ContractError::Cancelled,
        TransactionError::OutcomeUnknown => ContractError::OutcomeUnknown,
        TransactionError::IdempotencyExpired => ContractError::IdempotencyExpired,
        TransactionError::IntegrityFailure => ContractError::IntegrityFailure,
        TransactionError::RetryableUnavailable => ContractError::Unavailable,
        TransactionError::Storage(_) => ContractError::Storage,
    }
}

const fn authorized_error(error: AuthorizedError) -> ContractError {
    match error {
        AuthorizedError::Unauthorized => ContractError::Unauthorized,
        AuthorizedError::ResourceLimit => ContractError::ResourceLimit,
        AuthorizedError::StalePolicy => ContractError::StalePolicy,
        AuthorizedError::InvalidPolicy => ContractError::InvalidRequest,
        AuthorizedError::IntegrityFailure => ContractError::IntegrityFailure,
        AuthorizedError::Transaction(error) => transaction_error(error),
    }
}

const fn read_error(error: ResearchReadError) -> ContractError {
    match error {
        ResearchReadError::StaleView => ContractError::StaleView,
        ResearchReadError::StaleGeneration => ContractError::StaleGeneration,
        ResearchReadError::Rebuilding => ContractError::Rebuilding,
        ResearchReadError::HistoryUnavailable => ContractError::HistoryUnavailable,
        ResearchReadError::NotFound => ContractError::NotFound,
        ResearchReadError::ResourceLimit => ContractError::ResourceLimit,
        ResearchReadError::UnsupportedQuery => ContractError::UnsupportedQuery,
    }
}

const fn codec_error(error: uste_memory::ResearchCodecError) -> ContractError {
    match error {
        uste_memory::ResearchCodecError::ResourceLimit => ContractError::ResourceLimit,
        uste_memory::ResearchCodecError::ScopeMismatch => ContractError::Unauthorized,
        uste_memory::ResearchCodecError::Invalid
        | uste_memory::ResearchCodecError::UnsupportedVersion => ContractError::InvalidRequest,
    }
}
