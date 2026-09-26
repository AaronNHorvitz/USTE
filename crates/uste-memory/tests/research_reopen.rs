//! Durable journal admission and reopen equivalence for `research-memory-v1` (DB-R02.3).

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_memory::{
    CitationInput, ClaimInput, ClaimStatus, EdgeInput, EdgeKind, FetchOutcome, Freshness,
    ResearchMutation, ResearchRecord, ResearchState, ResearchTransaction, RetainedContent,
    SourceKind, SourceLocator, SourceRecordInput, SourceVersionId, SupportKind,
    encode_research_transaction,
};
use uste_policy::PrincipalDigest;
use uste_storage::{
    BlobInventory, ClockObservation, EntryName, fault::ScriptedClock, journal::DurableKeyEnvelope,
    memory::MemoryFileSystem,
};
use uste_txn::{
    CommitCoordinator, NeverCancel, RetentionDays, TransactionError, TransactionRequest,
    TransactionState,
};
use uste_types::{
    CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([4; 16]),
        NamespaceId::from_bytes([5; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(day * 86_400, 0).unwrap(),
        monotonic_ticks: u64::try_from(day).unwrap(),
    })])
}

fn transaction(mutation: ResearchMutation) -> Vec<u8> {
    encode_research_transaction(&ResearchTransaction {
        scope: scope(),
        generation: 1,
        mutation,
    })
    .unwrap()
}

fn put(record: ResearchRecord) -> ResearchMutation {
    ResearchMutation::Put(Box::new(record))
}

fn request<'a>(
    identity: u8,
    encoded: &'a [u8],
    inventory: Option<&'a BlobInventory>,
) -> TransactionRequest<'a> {
    TransactionRequest {
        principal: PrincipalDigest::from_bytes([7; 32]),
        idempotency_key: IdempotencyKey::from_bytes([identity; 16]),
        transaction_id: TransactionId::from_bytes([identity.wrapping_add(64); 16]),
        canonical_request: encoded,
        blob_inventory: inventory,
    }
}

fn claim(claim: u8, source: SourceVersionId, excerpt: &str, corrects: Option<u8>) -> ClaimInput {
    ClaimInput {
        id: id(claim),
        subject: "storage guide".to_owned(),
        predicate: "states".to_owned(),
        value: excerpt.to_owned(),
        support: SupportKind::DirectQuote,
        citations: vec![CitationInput {
            source,
            locator: SourceLocator::ByteRange {
                start: 0,
                end: u64::try_from(excerpt.len()).unwrap(),
            },
            excerpt_digest: Sha256::digest(excerpt.as_bytes()).into(),
            excerpt: Some(excerpt.to_owned()),
        }],
        valid_from: None,
        valid_until: None,
        corrects: corrects.map(id),
    }
}

#[test]
fn research_records_survive_journal_reopen_and_equal_in_memory_replay() {
    let mut filesystem = MemoryFileSystem::default();
    let name = EntryName::new("research-memory-reopen").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(10)).unwrap(),
        CounterEntropy(20),
        ResearchState::new(scope()),
    )
    .unwrap();
    let mut committed: Vec<(Vec<u8>, Option<BlobInventory>)> = Vec::new();
    let mut commit = |coordinator: &mut CommitCoordinator<_, _, _, _, _>,
                      filesystem: &mut MemoryFileSystem,
                      identity: u8,
                      bytes: Vec<u8>,
                      inventory: Option<BlobInventory>| {
        let outcome = coordinator
            .commit(
                filesystem,
                request(identity, &bytes, inventory.as_ref()),
                &mut clock(10 * i64::from(identity)),
                &NeverCancel,
            )
            .unwrap();
        // An exact retry returns the recorded outcome without a second effect.
        assert_eq!(
            coordinator
                .commit(
                    filesystem,
                    request(identity, &bytes, inventory.as_ref()),
                    &mut clock(10 * i64::from(identity) + 1),
                    &NeverCancel,
                )
                .unwrap(),
            outcome
        );
        committed.push((bytes, inventory));
        outcome
    };

    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        transaction(ResearchMutation::BeginRebuild { next_generation: 1 }),
        None,
    );
    let page = b"storage keeps journal authority; indexes are derived";
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, page)
        .unwrap();
    let blob = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let source_id = SourceVersionId {
        source: id(10),
        version: 1,
    };
    let source = SourceRecordInput {
        id: source_id,
        kind: SourceKind::DocPackPage,
        locator_text: "guide/storage.html".to_owned(),
        version_label: "synthetic-1".to_owned(),
        retrieved_at: UtcInstant::new(1_790_000_000, 0).unwrap(),
        run_identity: [3; 16],
        route_label: "offline-doc-pack".to_owned(),
        outcome: FetchOutcome::Complete,
        content: Some(RetainedContent {
            blob,
            media_type: "text/plain; charset=utf-8".to_owned(),
        }),
        license_label: "synthetic".to_owned(),
        redistributable: true,
        freshness: Freshness::Pinned,
    };
    let inventory = BlobInventory::new(scope(), [blob]).unwrap();
    // The source must arrive with exactly its own finalized blob.
    assert!(matches!(
        coordinator.commit(
            &mut filesystem,
            request(
                2,
                &transaction(put(ResearchRecord::Source(source.clone()))),
                None
            ),
            &mut clock(25),
            &NeverCancel,
        ),
        Err(TransactionError::InvalidRequest)
    ));
    commit(
        &mut coordinator,
        &mut filesystem,
        3,
        transaction(put(ResearchRecord::Source(source))),
        Some(inventory),
    );
    let quote = "storage keeps journal authority";
    commit(
        &mut coordinator,
        &mut filesystem,
        4,
        transaction(put(ResearchRecord::Claim(claim(
            20, source_id, quote, None,
        )))),
        None,
    );
    let correction = "indexes are derived";
    let mut corrected = claim(21, source_id, correction, Some(20));
    if let SourceLocator::ByteRange { start, end } = &mut corrected.citations[0].locator {
        *start = 33;
        *end = 52;
    }
    commit(
        &mut coordinator,
        &mut filesystem,
        5,
        transaction(put(ResearchRecord::Claim(corrected))),
        None,
    );
    commit(
        &mut coordinator,
        &mut filesystem,
        6,
        transaction(put(ResearchRecord::Edge(EdgeInput {
            id: id(30),
            kind: EdgeKind::Corrects,
            from: id(21),
            to: id(20),
            asserted_by: id(21),
            valid_from: None,
            valid_until: None,
        }))),
        None,
    );
    commit(
        &mut coordinator,
        &mut filesystem,
        7,
        transaction(ResearchMutation::RevokeSource { source: source_id }),
        None,
    );
    let complete = commit(
        &mut coordinator,
        &mut filesystem,
        8,
        transaction(ResearchMutation::CompleteRebuild),
        None,
    );
    let before = coordinator.read_view().unwrap();
    assert_eq!(before.revision(), Some(complete.revision));
    let state = before.state().clone();
    assert!(state.is_ready());
    assert!(matches!(
        state.claims()[&id(20)].status,
        ClaimStatus::Superseded(_)
    ));
    assert!(state.sources()[&source_id].revoked_revision.is_some());
    drop(coordinator);
    filesystem.restart().unwrap();

    let (reopened, report) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(30),
        CounterEntropy(40),
        &mut TestKeyAdapter,
        ResearchState::new(scope()),
    )
    .unwrap();
    assert_eq!(report.frontier, Some(complete.revision));
    let after = reopened.read_view().unwrap();
    assert_eq!(after.state(), &state);
    assert_eq!(after.state().state_digest(), state.state_digest());

    // An independent in-memory replay of exactly the committed requests reaches the same state.
    let mut replay = ResearchState::new(scope());
    for (index, (bytes, inventory)) in committed.iter().enumerate() {
        let revision = CommitRevision::new(u64::try_from(index).unwrap() + 1).unwrap();
        let prepared = replay.prepare(bytes, inventory.as_ref(), revision).unwrap();
        replay.publish(prepared);
    }
    assert_eq!(replay, state);
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
