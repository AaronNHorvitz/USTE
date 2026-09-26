//! DB-R03 adversarial lifecycle matrix for `research-memory-v1` through the authorized durable
//! coordinator: contradictions, correction chains, revocation with stale views, cross-scope
//! denial, per-record policy denial, stale generations, rebuild and restart.

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_memory::{
    CitationInput, ClaimInput, ClaimStatus, ClaimView, EdgeInput, EdgeKind, EffectiveSupport,
    FetchOutcome, Freshness, ResearchKnowledge, ResearchMutation, ResearchReadError,
    ResearchReadOutput, ResearchReadRequest, ResearchRecord, ResearchState, ResearchTransaction,
    RetainedContent, SourceKind, SourceLocator, SourceRecordInput, SourceVersionId, SupportKind,
    encode_research_transaction,
};
use uste_policy::{
    Action, AuthenticationError, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, PrincipalDigest, QuotaLimits, TrustedPrincipalAdapter,
};
use uste_storage::{
    BLOB_CHUNK_BYTES, BlobInventory, ClockObservation, EntryName, fault::ScriptedClock,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedError, AuthorizedReadError, AuthorizedTransactionRequest,
    CommitCoordinator, NeverCancel, RetentionDays, open_authorized,
};
use uste_types::{
    CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([8; 16]),
        NamespaceId::from_bytes([9; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn clock(step: u64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(i64::try_from(step).unwrap() * 3_600, 0).unwrap(),
        monotonic_ticks: step,
    })])
}

fn namespace_policy(version: u64, deny_read: Option<RecordId>) -> NamespacePolicy {
    let limits = QuotaLimits::new(
        128 * 1024,
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

fn policy() -> PolicyKernel {
    let mut kernel = PolicyKernel::new();
    kernel
        .install_initial_policy(namespace_policy(1, None))
        .unwrap();
    kernel
}

type Coordinator = AuthorizedCoordinator<
    ResearchState,
    MemoryFileSystem,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

struct Lane {
    filesystem: MemoryFileSystem,
    coordinator: Coordinator,
    actor: uste_policy::AuthenticatedPrincipal,
    next: u8,
}

impl Lane {
    fn commit(
        &mut self,
        generation: u64,
        mutation: ResearchMutation,
        inventory: Option<&BlobInventory>,
    ) -> CommitRevision {
        let bytes = encode_research_transaction(&ResearchTransaction {
            scope: scope(),
            generation,
            mutation,
        })
        .unwrap();
        let identity = self.next;
        self.next += 1;
        self.coordinator
            .commit(
                &mut self.filesystem,
                &self.actor,
                AuthorizedTransactionRequest {
                    idempotency_key: IdempotencyKey::from_bytes([identity; 16]),
                    transaction_id: TransactionId::from_bytes([identity.wrapping_add(100); 16]),
                    canonical_request: &bytes,
                    blob_inventory: inventory,
                },
                &mut clock(u64::from(identity)),
                &NeverCancel,
            )
            .unwrap()
            .revision
    }

    fn put_source(&mut self, generation: u64, source: u8, text: &str) -> SourceVersionId {
        let mut upload = self.coordinator.start_blob_upload(&self.actor).unwrap();
        self.coordinator
            .write_blob_upload(
                &mut self.filesystem,
                &self.actor,
                &mut upload,
                text.as_bytes(),
            )
            .unwrap();
        let blob = self
            .coordinator
            .finish_blob_upload(&mut self.filesystem, &self.actor, &mut upload)
            .unwrap();
        let id_version = SourceVersionId {
            source: id(source),
            version: 1,
        };
        let inventory = BlobInventory::new(scope(), [blob]).unwrap();
        self.commit(
            generation,
            put(ResearchRecord::Source(SourceRecordInput {
                id: id_version,
                kind: SourceKind::WebPage,
                locator_text: format!("https://example.invalid/{source}"),
                version_label: "unversioned".to_owned(),
                retrieved_at: UtcInstant::new(3_600, 0).unwrap(),
                run_identity: [1; 16],
                route_label: "fake-provider".to_owned(),
                outcome: FetchOutcome::Complete,
                content: Some(RetainedContent {
                    blob,
                    media_type: "text/plain".to_owned(),
                }),
                license_label: "synthetic".to_owned(),
                redistributable: false,
                freshness: Freshness::MaxAge { seconds: 86_400 },
            })),
            Some(&inventory),
        );
        id_version
    }

    fn get_claim(
        &self,
        view: &uste_txn::AuthorizedReadView<ResearchState>,
        generation: u64,
        claim: u8,
        knowledge: ResearchKnowledge,
    ) -> Result<ClaimView, AuthorizedReadError<ResearchReadError>> {
        match self.coordinator.read(
            &self.actor,
            view,
            &ResearchReadRequest::GetClaim {
                authority_generation: generation,
                claim: id(claim),
                knowledge,
                evaluated_at: UtcInstant::new(7_200, 0).unwrap(),
                maximum_output_bytes: 16 * 1024,
            },
        )? {
            ResearchReadOutput::Claim(view) => Ok(view),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn search(
        &self,
        view: &uste_txn::AuthorizedReadView<ResearchState>,
        scope: NamespaceRef,
        term: &str,
    ) -> Result<Vec<RecordRef>, AuthorizedReadError<ResearchReadError>> {
        match self.coordinator.read(
            &self.actor,
            view,
            &ResearchReadRequest::Search {
                scope,
                authority_generation: 1,
                terms: vec![term.to_owned()],
                knowledge: ResearchKnowledge::Current,
                evaluated_at: UtcInstant::new(7_200, 0).unwrap(),
                maximum_candidates: 64,
                maximum_results: 16,
                maximum_output_bytes: 64 * 1024,
            },
        )? {
            ResearchReadOutput::Search(results) => {
                Ok(results.items.into_iter().map(|claim| claim.id).collect())
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn put(record: ResearchRecord) -> ResearchMutation {
    ResearchMutation::Put(Box::new(record))
}

fn quote(
    claim: u8,
    source: SourceVersionId,
    text: &str,
    start: u64,
    corrects: Option<u8>,
) -> ClaimInput {
    let end = start + u64::try_from(text.len()).unwrap();
    ClaimInput {
        id: id(claim),
        subject: "cache policy".to_owned(),
        predicate: "states".to_owned(),
        value: text.to_owned(),
        support: SupportKind::DirectQuote,
        citations: vec![CitationInput {
            source,
            locator: SourceLocator::ByteRange { start, end },
            excerpt_digest: Sha256::digest(text.as_bytes()).into(),
            excerpt: Some(text.to_owned()),
        }],
        valid_from: None,
        valid_until: None,
        corrects: corrects.map(id),
    }
}

fn edge(edge: u8, from: u8, to: u8, kind: EdgeKind) -> EdgeInput {
    EdgeInput {
        id: id(edge),
        kind,
        from: id(from),
        to: id(to),
        asserted_by: id(from),
        valid_from: None,
        valid_until: None,
    }
}

const FIRST: &str = "the cache is authoritative";
const SECOND: &str = "the cache is derived and rebuildable";

#[test]
fn research_lifecycle_matrix_through_the_authorized_durable_coordinator() {
    let mut filesystem = MemoryFileSystem::default();
    let name = EntryName::new("research-memory-lifecycle").unwrap();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(10)).unwrap(),
        CounterEntropy(20),
        ResearchState::new(scope()),
    )
    .unwrap();
    let kernel = policy();
    let actor = kernel.authenticate(&mut AuthAdapter, &7).unwrap();
    let mut lane = Lane {
        filesystem,
        coordinator: AuthorizedCoordinator::new(raw, kernel).unwrap(),
        actor,
        next: 1,
    };
    lane.commit(
        1,
        ResearchMutation::BeginRebuild { next_generation: 1 },
        None,
    );
    let first = lane.put_source(1, 10, FIRST);
    let second = lane.put_source(1, 11, SECOND);
    lane.commit(
        1,
        put(ResearchRecord::Claim(quote(20, first, FIRST, 0, None))),
        None,
    );
    lane.commit(
        1,
        put(ResearchRecord::Claim(quote(21, second, SECOND, 0, None))),
        None,
    );
    // Contradiction: both claims stay visible and active; the relation is explicit.
    lane.commit(
        1,
        put(ResearchRecord::Edge(edge(
            30,
            21,
            20,
            EdgeKind::Contradicts,
        ))),
        None,
    );
    lane.commit(1, ResearchMutation::CompleteRebuild, None);

    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    for claim in [20, 21] {
        let read = lane
            .get_claim(&view, 1, claim, ResearchKnowledge::Current)
            .unwrap();
        assert_eq!(read.status, ClaimStatus::Active);
        assert_eq!(
            read.effective_support,
            EffectiveSupport::AsRecorded(SupportKind::DirectQuote)
        );
    }
    let ResearchReadOutput::Edges(contradictions) = lane
        .coordinator
        .read(
            &lane.actor,
            &view,
            &ResearchReadRequest::EdgesFrom {
                authority_generation: 1,
                from: id(21),
                kind: Some(EdgeKind::Contradicts),
                knowledge: ResearchKnowledge::Current,
                maximum_candidates: 8,
                maximum_results: 8,
            },
        )
        .unwrap()
    else {
        panic!("edges");
    };
    assert_eq!(contradictions.items.len(), 1);

    // Correction chain: 22 corrects 21, 23 corrects 22; history still shows 21 active earlier.
    let before_corrections = lane.coordinator.read_view(&lane.actor).unwrap();
    let before_revision = lane
        .coordinator
        .read_view_revision(&before_corrections)
        .unwrap()
        .unwrap();
    lane.commit(
        1,
        put(ResearchRecord::Claim(quote(
            22,
            second,
            "derived",
            13,
            Some(21),
        ))),
        None,
    );
    lane.commit(
        1,
        put(ResearchRecord::Claim(quote(
            23,
            second,
            "rebuildable",
            25,
            Some(22),
        ))),
        None,
    );
    // A view taken before a newer commit is stale rather than silently outdated.
    assert_eq!(
        lane.get_claim(&before_corrections, 1, 21, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::StaleView)
    );
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    for (claim, superseded) in [(21, true), (22, true), (23, false)] {
        let status = lane
            .get_claim(&view, 1, claim, ResearchKnowledge::Current)
            .unwrap()
            .status;
        assert_eq!(matches!(status, ClaimStatus::Superseded(_)), superseded);
    }
    assert_eq!(
        lane.get_claim(&view, 1, 21, ResearchKnowledge::Revision(before_revision))
            .unwrap()
            .status,
        ClaimStatus::Active
    );

    // Revocation: the pre-revocation view becomes stale; afterwards the excerpt is withheld in
    // current and historical views and excerpt-only search terms stop matching.
    assert_eq!(
        lane.search(&view, scope(), "authoritative").unwrap(),
        [id(20)]
    );
    lane.commit(1, ResearchMutation::RevokeSource { source: first }, None);
    assert_eq!(
        lane.get_claim(&view, 1, 20, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::StaleView)
    );
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    for knowledge in [
        ResearchKnowledge::Current,
        ResearchKnowledge::Revision(before_revision),
    ] {
        let revoked = lane.get_claim(&view, 1, 20, knowledge).unwrap();
        assert!(matches!(
            revoked.effective_support,
            EffectiveSupport::Revoked(_)
        ));
        assert_eq!(revoked.citations[0].excerpt, None);
    }
    // The claim's own value still contains the words; only the cited excerpt is withheld.
    assert_eq!(
        lane.search(&view, scope(), "authoritative").unwrap(),
        [id(20)]
    );

    // Cross-scope requests fail at the authorization boundary.
    let foreign = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([99; 16]));
    assert_eq!(
        lane.search(&view, foreign, "cache").unwrap_err(),
        AuthorizedReadError::Authorization(AuthorizedError::Unauthorized)
    );

    // Per-record policy denial: the old view fails as stale policy; the new view hides the
    // denied source's citation without revealing that it exists.
    lane.coordinator
        .replace_namespace_policy(
            &lane.actor,
            PolicyVersion::new(1).unwrap(),
            namespace_policy(2, Some(id(11).record())),
        )
        .unwrap();
    assert_eq!(
        lane.get_claim(&view, 1, 23, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Authorization(AuthorizedError::StalePolicy)
    );
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    let hidden = lane
        .get_claim(&view, 1, 23, ResearchKnowledge::Current)
        .unwrap();
    assert!(hidden.citations.is_empty());
    assert_eq!(
        hidden.effective_support,
        EffectiveSupport::NoVisibleCitation
    );

    // Stale generation and rebuild across restart.
    lane.commit(
        2,
        ResearchMutation::BeginRebuild { next_generation: 2 },
        None,
    );
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    assert_eq!(
        lane.get_claim(&view, 1, 23, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::StaleGeneration)
    );
    assert_eq!(
        lane.get_claim(&view, 2, 23, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::Rebuilding)
    );
    let Lane {
        mut filesystem,
        coordinator,
        ..
    } = lane;
    drop(coordinator);
    filesystem.restart().unwrap();
    let kernel = policy();
    let actor = kernel.authenticate(&mut AuthAdapter, &7).unwrap();
    let (coordinator, _) = open_authorized(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(30),
        CounterEntropy(40),
        &mut TestKeyAdapter,
        ResearchState::new(scope()),
        kernel,
    )
    .unwrap();
    let mut lane = Lane {
        filesystem,
        coordinator,
        actor,
        next: 40,
    };
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    assert_eq!(
        lane.get_claim(&view, 2, 23, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::Rebuilding)
    );
    // After reopen, uploads stay closed until recovered-upload reconciliation completes.
    assert_eq!(
        lane.coordinator.start_blob_upload(&lane.actor).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    lane.coordinator
        .complete_recovered_upload_reconciliation(&mut lane.filesystem, &lane.actor, &[])
        .unwrap();
    let rebuilt = lane.put_source(2, 12, FIRST);
    lane.commit(
        2,
        put(ResearchRecord::Claim(quote(24, rebuilt, FIRST, 0, None))),
        None,
    );
    lane.commit(2, ResearchMutation::CompleteRebuild, None);
    let view = lane.coordinator.read_view(&lane.actor).unwrap();
    // The rebuild cleared generation-1 derived records; only rebuilt records remain.
    assert_eq!(
        lane.get_claim(&view, 2, 20, ResearchKnowledge::Current)
            .unwrap_err(),
        AuthorizedReadError::Domain(ResearchReadError::NotFound)
    );
    assert_eq!(
        lane.get_claim(&view, 2, 24, ResearchKnowledge::Current)
            .unwrap()
            .status,
        ClaimStatus::Active
    );
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
