//! Runs the `uste-memory-consumer` 1.0 synthetic conformance fixture against the producer over
//! the authorized durable coordinator, then checks restart and reopen.

#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_memory::contract::conformance::{
    CONFORMANCE_DENIED_SOURCE, CONFORMANCE_FIXTURE_DIGEST, CONFORMANCE_STEPS, ConformanceInputs,
    fixture_digest, run_conformance,
};
use uste_memory::contract::{
    ContractError, ContractWrite, MEMORY_CONTRACT_VERSION, MemoryConsumerContract, OperationId,
};
use uste_memory::{ResearchKnowledge, ResearchReadOutput, ResearchReadRequest};
use uste_memory_adapter::research::ResearchMemoryProducer;
use uste_policy::{
    Action, AuthenticationError, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, PrincipalDigest, QuotaLimits, TrustedPrincipalAdapter,
};
use uste_storage::{
    AdapterError, BLOB_CHUNK_BYTES, Clock, ClockObservation, EntryName,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{NeverCancel, RetentionDays};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x31; 16]),
        NamespaceId::from_bytes([0x32; 16]),
    )
}

fn namespace_policy(version: u64, deny_read: Option<RecordId>) -> NamespacePolicy {
    let limits = QuotaLimits::new(
        1024 * 1024,
        2 * 1024 * 1024,
        32 * 1024 * 1024,
        8,
        u32::try_from(BLOB_CHUNK_BYTES).unwrap(),
    )
    .unwrap();
    let actions = PermissionSet::from_actions([
        Action::ReadRecord,
        Action::ReadHistory,
        Action::ExpandGraph,
        Action::Search,
        Action::ReadBlob,
        Action::Commit,
        Action::StartUpload,
        Action::ResumeUpload,
        Action::WriteUpload,
        Action::FinishUpload,
        Action::AbortUpload,
        Action::ReadOwnOutcome,
        Action::ManagePolicy,
        Action::ManageSchema,
        Action::ManageRetention,
    ]);
    let mut grant = NamespaceGrant::new(actions, limits);
    if let Some(record) = deny_read {
        grant
            .deny_record(record, PermissionSet::from_actions([Action::ReadRecord]))
            .unwrap();
    }
    let mut namespace = NamespacePolicy::new(scope(), PolicyVersion::new(version).unwrap(), limits);
    namespace
        .grant(PrincipalDigest::from_bytes([7; 32]), grant)
        .unwrap();
    namespace
}

fn kernel() -> PolicyKernel {
    let mut kernel = PolicyKernel::new();
    kernel
        .install_initial_policy(namespace_policy(1, None))
        .unwrap();
    kernel
}

/// Monotonic synthetic clock: one minute per observation.
struct TickClock(i64);

impl Clock for TickClock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError> {
        self.0 += 60;
        Ok(ClockObservation {
            wall_utc: UtcInstant::new(1_790_000_000 + self.0, 0).unwrap(),
            monotonic_ticks: u64::try_from(self.0).unwrap(),
        })
    }
}

#[test]
fn producer_passes_the_pinned_conformance_fixture_and_survives_reopen() {
    let kernel = kernel();
    let principal = kernel.authenticate(&mut AuthAdapter, &7).unwrap();
    let name = EntryName::new("research-contract").unwrap();
    let mut producer = ResearchMemoryProducer::create(
        MemoryFileSystem::default(),
        name.clone(),
        scope(),
        RetentionDays::new(30).unwrap(),
        KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(10)).unwrap(),
        CounterEntropy(20),
        kernel,
        principal,
        TickClock(0),
    )
    .unwrap();
    assert_eq!(producer.contract_version(), MEMORY_CONTRACT_VERSION);
    let report = run_conformance(
        &mut producer,
        ConformanceInputs {
            scope: scope(),
            foreign_scope: NamespaceRef::new(
                scope().database(),
                NamespaceId::from_bytes([0x99; 16]),
            ),
            restricted_policy: (
                PolicyVersion::new(1).unwrap(),
                namespace_policy(2, Some(RecordId::from_bytes(CONFORMANCE_DENIED_SOURCE))),
            ),
        },
    )
    .unwrap();
    assert_eq!(report.steps_passed, CONFORMANCE_STEPS.len());
    assert_eq!(report.fixture_digest, fixture_digest());
    let hex: String = report
        .fixture_digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(hex, CONFORMANCE_FIXTURE_DIGEST);

    // Reopen after process loss: generation 2 is still rebuilding and the store accepts writes.
    let mut filesystem = producer.into_filesystem();
    filesystem.restart().unwrap();
    let kernel = self::kernel();
    let principal = kernel.authenticate(&mut AuthAdapter, &7).unwrap();
    let mut reopened = ResearchMemoryProducer::open(
        filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(30),
        CounterEntropy(40),
        &mut TestKeyAdapter,
        kernel,
        principal,
        TickClock(100_000),
    )
    .unwrap();
    let claim = RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([0xC3; 16]),
    );
    let read = |producer: &ResearchMemoryProducer<_, _, _, _, _>| {
        let view = producer.view().unwrap();
        producer.read(
            &view,
            &ResearchReadRequest::GetClaim {
                authority_generation: 2,
                claim,
                knowledge: ResearchKnowledge::Current,
                evaluated_at: UtcInstant::new(1_790_000_000, 0).unwrap(),
                maximum_output_bytes: 4096,
            },
            &NeverCancel,
        )
    };
    assert_eq!(read(&reopened).unwrap_err(), ContractError::Rebuilding);
    reopened
        .write(
            OperationId {
                idempotency_key: [0xF0; 16],
                transaction_id: [0xF1; 16],
            },
            &ContractWrite::CompleteGeneration { generation: 2 },
            &NeverCancel,
        )
        .unwrap();
    // The rebuilt generation holds no generation-1 records.
    assert_eq!(read(&reopened).unwrap_err(), ContractError::NotFound);
    assert!(!matches!(read(&reopened), Ok(ResearchReadOutput::Claim(_))));
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = u8;

    fn authenticate(
        &mut self,
        credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*credential; 32]))
    }
}

#[derive(Debug)]
struct TestEnvelope([u8; 32]);

impl DurableKeyEnvelope for TestEnvelope {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(self.0.to_vec())
    }

    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self(
            encoded
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        ))
    }
}

struct TestKeyAdapter;

impl KeyAdapter for TestKeyAdapter {
    type Envelope = TestEnvelope;

    fn wrap(
        &mut self,
        _database: DatabaseId,
        key: &SecretKeyMaterial,
        _entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(TestEnvelope(*key.expose_to_adapter()))
    }

    fn unwrap(
        &mut self,
        _database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(envelope.0))
    }
}

#[derive(Debug)]
struct CounterEntropy(u64);

impl EntropySource for CounterEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
        for (index, chunk) in output.chunks_mut(8).enumerate() {
            let value = self
                .0
                .checked_add(u64::try_from(index).map_err(|_| EntropyFailure)?)
                .ok_or(EntropyFailure)?;
            chunk.copy_from_slice(&value.to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
