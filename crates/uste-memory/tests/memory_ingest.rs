use std::cell::Cell;

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_memory::{
    EventTimeFilter, FactInput, KnowledgeAt, MemoryMutation, MemoryReadError, MemoryReadOutput,
    MemoryReadRequest, MemoryState, MemoryTransaction, SourceLocator, SourceVersionId,
    SourceVersionInput, encode_transaction,
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
    AuthorizedCoordinator, AuthorizedError, AuthorizedTransactionRequest, Cancellation,
    CommitCoordinator, NeverCancel, RetentionDays, open_authorized,
};

struct CancelAfterChecks(Cell<usize>);

impl CancelAfterChecks {
    const fn new(checks: usize) -> Self {
        Self(Cell::new(checks))
    }
}

impl Cancellation for CancelAfterChecks {
    fn is_cancelled(&self) -> bool {
        let remaining = self.0.get();
        if remaining == 0 {
            true
        } else {
            self.0.set(remaining - 1);
            false
        }
    }
}
use uste_types::{
    DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef, TransactionId,
    UtcInstant,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn record(value: u8) -> RecordRef {
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

fn namespace_policy(version: u64, deny_source: Option<RecordId>) -> NamespacePolicy {
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
        Action::InspectQuota,
        Action::ManagePolicy,
        Action::ManageSchema,
        Action::ManageRetention,
    ]);
    let mut grant = NamespaceGrant::new(actions, limits);
    if let Some(source) = deny_source {
        grant
            .deny_record(
                source,
                PermissionSet::from_actions([Action::ReadRecord, Action::ReadBlob]),
            )
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

fn principal(kernel: &PolicyKernel) -> uste_policy::AuthenticatedPrincipal {
    kernel.authenticate(&mut AuthAdapter, &7).unwrap()
}

fn request<'a>(
    identity: u8,
    encoded: &'a [u8],
    inventory: Option<&'a BlobInventory>,
) -> AuthorizedTransactionRequest<'a> {
    AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([identity; 16]),
        transaction_id: TransactionId::from_bytes([identity.wrapping_add(64); 16]),
        canonical_request: encoded,
        blob_inventory: inventory,
    }
}

fn transaction(generation: u64, mutation: MemoryMutation) -> Vec<u8> {
    encode_transaction(&MemoryTransaction {
        scope: scope(),
        generation,
        mutation,
    })
    .unwrap()
}

#[test]
fn approved_source_and_fact_retry_reopen_and_abandoned_upload_reconciliation_are_exact() {
    let mut filesystem = MemoryFileSystem::default();
    let name = EntryName::new("memory-pilot-ingest").unwrap();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 10),
        CounterEntropy(20),
        MemoryState::new(scope()),
    )
    .unwrap();
    let kernel = policy();
    let actor = principal(&kernel);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();

    let begin = transaction(1, MemoryMutation::BeginRebuild { next_generation: 1 });
    coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(1, &begin, None),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();

    let text = b"alpha is current\nbeta is historical\n";
    let mut upload = coordinator.start_blob_upload(&actor).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &actor, &mut upload, text)
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &actor, &mut upload)
        .unwrap();
    assert_eq!(
        reference.content_digest(),
        <[u8; 32]>::from(Sha256::digest(text))
    );
    let source_id = SourceVersionId {
        source: record(10),
        version: 1,
    };
    let source = transaction(
        1,
        MemoryMutation::PutSource(SourceVersionInput {
            id: source_id,
            blob: reference,
            media_type: "text/plain; charset=utf-8".to_owned(),
            exact_utf8: Some(String::from_utf8(text.to_vec()).unwrap()),
            source_event_time: Some(UtcInstant::new(1_700_000_000, 0).unwrap()),
        }),
    );
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let source_outcome = coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(2, &source, Some(&inventory)),
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                &actor,
                request(2, &source, Some(&inventory)),
                &mut clock(3),
                &NeverCancel,
            )
            .unwrap(),
        source_outcome
    );

    let fact = transaction(
        1,
        MemoryMutation::PutFact(FactInput {
            id: record(20),
            subject: "project".to_owned(),
            predicate: "status".to_owned(),
            value: "alpha".to_owned(),
            links: Vec::new(),
            source: source_id,
            locator: SourceLocator::Utf8Lines {
                start: 0,
                end: 16,
                first_line: 1,
                last_line: 1,
            },
            source_event_time: Some(UtcInstant::new(1_700_000_001, 0).unwrap()),
        }),
    );
    coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(3, &fact, None),
            &mut clock(4),
            &NeverCancel,
        )
        .unwrap();
    let complete = transaction(1, MemoryMutation::CompleteRebuild);
    let complete_outcome = coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(4, &complete, None),
            &mut clock(5),
            &NeverCancel,
        )
        .unwrap();

    let initial_view = coordinator.read_view(&actor).unwrap();
    let initial_search = MemoryReadRequest::Search {
        scope: scope(),
        authority_generation: 1,
        terms: vec!["alpha".to_owned()],
        knowledge: KnowledgeAt::Current,
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Search(initial_results) = coordinator
        .read(&actor, &initial_view, &initial_search)
        .unwrap()
    else {
        panic!("search output")
    };
    assert_eq!(initial_results.facts.len(), 1);
    let citation = MemoryReadRequest::ResolveCitation {
        authority_generation: 1,
        fact: record(20),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Citation(citation) =
        coordinator.read(&actor, &initial_view, &citation).unwrap()
    else {
        panic!("citation output")
    };
    assert_eq!(citation.source, source_id);
    assert_eq!(
        citation.exact_utf8_excerpt.as_deref(),
        Some("alpha is current")
    );

    let correction = transaction(
        1,
        MemoryMutation::CorrectFact {
            target: record(20),
            replacement: FactInput {
                id: record(21),
                subject: "project".to_owned(),
                predicate: "status".to_owned(),
                value: "beta".to_owned(),
                links: Vec::new(),
                source: source_id,
                locator: SourceLocator::Utf8Lines {
                    start: 17,
                    end: 35,
                    first_line: 2,
                    last_line: 2,
                },
                source_event_time: Some(UtcInstant::new(1_700_000_002, 0).unwrap()),
            },
        },
    );
    coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(6, &correction, None),
            &mut clock(6),
            &NeverCancel,
        )
        .unwrap();
    let contradiction = transaction(
        1,
        MemoryMutation::PutFact(FactInput {
            id: record(30),
            subject: "project".to_owned(),
            predicate: "status".to_owned(),
            value: "alpha".to_owned(),
            links: Vec::new(),
            source: source_id,
            locator: SourceLocator::Utf8Lines {
                start: 0,
                end: 16,
                first_line: 1,
                last_line: 1,
            },
            source_event_time: Some(UtcInstant::new(1_700_000_003, 0).unwrap()),
        }),
    );
    coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(7, &contradiction, None),
            &mut clock(7),
            &NeverCancel,
        )
        .unwrap();
    let linked = transaction(
        1,
        MemoryMutation::PutFact(FactInput {
            id: record(40),
            subject: "note".to_owned(),
            predicate: "supports".to_owned(),
            value: "status evidence".to_owned(),
            links: vec![record(30)],
            source: source_id,
            locator: SourceLocator::ByteRange { start: 0, end: 16 },
            source_event_time: None,
        }),
    );
    let linked_outcome = coordinator
        .commit(
            &mut filesystem,
            &actor,
            request(8, &linked, None),
            &mut clock(8),
            &NeverCancel,
        )
        .unwrap();

    let current_view = coordinator.read_view(&actor).unwrap();
    let current_request = MemoryReadRequest::Search {
        scope: scope(),
        authority_generation: 1,
        terms: vec!["status".to_owned()],
        knowledge: KnowledgeAt::Current,
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Search(current_results) = coordinator
        .read(&actor, &current_view, &current_request)
        .unwrap()
    else {
        panic!("current search output")
    };
    assert_eq!(
        current_results
            .facts
            .iter()
            .map(|fact| fact.id)
            .collect::<Vec<_>>(),
        vec![record(21), record(30), record(40)]
    );
    let mut budgeted = current_request.clone();
    let MemoryReadRequest::Search {
        maximum_candidates, ..
    } = &mut budgeted
    else {
        unreachable!()
    };
    *maximum_candidates = 1;
    assert_eq!(
        coordinator
            .read(&actor, &current_view, &budgeted)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::ResourceLimit)
    );
    let mut truncated = current_request.clone();
    let MemoryReadRequest::Search {
        maximum_results, ..
    } = &mut truncated
    else {
        unreachable!()
    };
    *maximum_results = 1;
    let MemoryReadOutput::Search(truncated) =
        coordinator.read(&actor, &current_view, &truncated).unwrap()
    else {
        panic!("truncated search output")
    };
    assert_eq!(truncated.facts.len(), 1);
    assert!(truncated.truncated);
    let mut event_filtered = current_request.clone();
    let MemoryReadRequest::Search { event_time, .. } = &mut event_filtered else {
        unreachable!()
    };
    *event_time = EventTimeFilter::Exact(UtcInstant::new(1_700_000_003, 0).unwrap());
    let MemoryReadOutput::Search(event_filtered) = coordinator
        .read(&actor, &current_view, &event_filtered)
        .unwrap()
    else {
        panic!("event-filtered search output")
    };
    assert_eq!(event_filtered.facts.len(), 1);
    assert_eq!(event_filtered.facts[0].id, record(30));
    assert_eq!(
        coordinator
            .read_cancellable(
                &actor,
                &current_view,
                &current_request,
                &CancelAfterChecks::new(3),
            )
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Authorization(AuthorizedError::Transaction(
            uste_txn::TransactionError::Cancelled
        ))
    );
    let historical = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(20),
        knowledge: KnowledgeAt::Revision(complete_outcome.revision),
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Fact(historical) = coordinator
        .read(&actor, &current_view, &historical)
        .unwrap()
    else {
        panic!("historical fact output")
    };
    assert_eq!(historical.value, "alpha");
    let one_hop = MemoryReadRequest::OneHop {
        authority_generation: 1,
        from: record(40),
        knowledge: KnowledgeAt::Current,
        maximum_candidates: 8,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Search(one_hop) =
        coordinator.read(&actor, &current_view, &one_hop).unwrap()
    else {
        panic!("one-hop output")
    };
    assert_eq!(one_hop.facts.len(), 1);
    assert_eq!(one_hop.facts[0].id, record(30));
    let stale = MemoryReadRequest::GetFact {
        authority_generation: 2,
        fact: record(21),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator.read(&actor, &current_view, &stale).unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::StaleGeneration)
    );

    let mut abandoned = coordinator.start_blob_upload(&actor).unwrap();
    let abandoned_token = abandoned.token();
    coordinator
        .write_blob_upload(
            &mut filesystem,
            &actor,
            &mut abandoned,
            &vec![0x5a; BLOB_CHUNK_BYTES],
        )
        .unwrap();
    drop(abandoned);
    drop(coordinator);
    filesystem.restart().unwrap();

    let reopened_kernel = policy();
    let reopened_actor = principal(&reopened_kernel);
    let (mut coordinator, report) = open_authorized(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(30),
        CounterEntropy(40),
        &mut TestKeyAdapter,
        MemoryState::new(scope()),
        reopened_kernel,
    )
    .unwrap();
    assert_eq!(report.frontier, Some(linked_outcome.revision));
    assert_eq!(
        coordinator.start_blob_upload(&reopened_actor).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                &reopened_actor,
                request(2, &source, Some(&inventory)),
                &mut clock(9),
                &NeverCancel,
            )
            .unwrap(),
        source_outcome
    );
    let mut resumed = coordinator
        .resume_blob_upload(&mut filesystem, &reopened_actor, abandoned_token)
        .unwrap();
    assert_eq!(
        resumed.accepted_bytes(),
        u64::try_from(BLOB_CHUNK_BYTES).unwrap()
    );
    coordinator
        .abort_blob_upload(&mut filesystem, &reopened_actor, &mut resumed)
        .unwrap();
    coordinator
        .complete_recovered_upload_reconciliation(
            &mut filesystem,
            &reopened_actor,
            &[abandoned_token],
        )
        .unwrap();

    let next_text = b"gamma";
    let mut next_upload = coordinator.start_blob_upload(&reopened_actor).unwrap();
    coordinator
        .write_blob_upload(
            &mut filesystem,
            &reopened_actor,
            &mut next_upload,
            next_text,
        )
        .unwrap();
    let next_reference = coordinator
        .finish_blob_upload(&mut filesystem, &reopened_actor, &mut next_upload)
        .unwrap();
    let next_source = transaction(
        1,
        MemoryMutation::PutSource(SourceVersionInput {
            id: SourceVersionId {
                source: record(10),
                version: 2,
            },
            blob: next_reference,
            media_type: "text/plain; charset=utf-8".to_owned(),
            exact_utf8: Some("gamma".to_owned()),
            source_event_time: None,
        }),
    );
    let next_inventory = BlobInventory::new(scope(), [next_reference]).unwrap();
    coordinator
        .commit(
            &mut filesystem,
            &reopened_actor,
            request(9, &next_source, Some(&next_inventory)),
            &mut clock(10),
            &NeverCancel,
        )
        .unwrap();

    let after_source_change = coordinator.read_view(&reopened_actor).unwrap();
    let stale_current = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(21),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator
            .read(&reopened_actor, &after_source_change, &stale_current)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::NotFound)
    );
    let retained_history = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(21),
        knowledge: KnowledgeAt::Revision(linked_outcome.revision),
        maximum_output_bytes: 4096,
    };
    assert!(matches!(
        coordinator
            .read(&reopened_actor, &after_source_change, &retained_history)
            .unwrap(),
        MemoryReadOutput::Fact(_)
    ));
    let historical_search = MemoryReadRequest::Search {
        scope: scope(),
        authority_generation: 1,
        terms: vec!["status".to_owned()],
        knowledge: KnowledgeAt::Revision(complete_outcome.revision),
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    let MemoryReadOutput::Search(historical_search) = coordinator
        .read(&reopened_actor, &after_source_change, &historical_search)
        .unwrap()
    else {
        panic!("historical search output")
    };
    let oracle = independent_oracle_ids(
        complete_outcome.revision.get(),
        linked_outcome.revision.get() + 1,
        "status",
    );
    assert_eq!(
        historical_search
            .facts
            .iter()
            .map(|fact| fact.id)
            .collect::<Vec<_>>(),
        oracle
    );
    let unsupported = MemoryReadRequest::Unsupported {
        authority_generation: 1,
    };
    assert_eq!(
        coordinator
            .read(&reopened_actor, &after_source_change, &unsupported)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Authorization(AuthorizedError::Transaction(
            uste_txn::TransactionError::UnsupportedPredicate
        ))
    );
    let foreign_search = MemoryReadRequest::Search {
        scope: NamespaceRef::new(scope().database(), NamespaceId::from_bytes([99; 16])),
        authority_generation: 1,
        terms: vec!["gamma".to_owned()],
        knowledge: KnowledgeAt::Current,
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator
            .read(&reopened_actor, &after_source_change, &foreign_search)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Authorization(AuthorizedError::Unauthorized)
    );

    let source_two = SourceVersionId {
        source: record(10),
        version: 2,
    };
    let source_two_fact = transaction(
        1,
        MemoryMutation::PutFact(FactInput {
            id: record(50),
            subject: "project".to_owned(),
            predicate: "status".to_owned(),
            value: "gamma".to_owned(),
            links: Vec::new(),
            source: source_two,
            locator: SourceLocator::Utf8Lines {
                start: 0,
                end: 5,
                first_line: 1,
                last_line: 1,
            },
            source_event_time: None,
        }),
    );
    let source_two_fact_outcome = coordinator
        .commit(
            &mut filesystem,
            &reopened_actor,
            request(10, &source_two_fact, None),
            &mut clock(11),
            &NeverCancel,
        )
        .unwrap();
    let before_revoke = coordinator.read_view(&reopened_actor).unwrap();
    let source_two_read = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(50),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    assert!(matches!(
        coordinator
            .read(&reopened_actor, &before_revoke, &source_two_read)
            .unwrap(),
        MemoryReadOutput::Fact(_)
    ));
    let revoke = transaction(1, MemoryMutation::RevokeSource { source: source_two });
    coordinator
        .commit(
            &mut filesystem,
            &reopened_actor,
            request(11, &revoke, None),
            &mut clock(12),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .read(&reopened_actor, &before_revoke, &source_two_read)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::StaleView)
    );
    let after_revoke = coordinator.read_view(&reopened_actor).unwrap();
    let revoked_history = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(50),
        knowledge: KnowledgeAt::Revision(source_two_fact_outcome.revision),
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator
            .read(&reopened_actor, &after_revoke, &revoked_history)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::NotFound)
    );

    let begin_two = transaction(2, MemoryMutation::BeginRebuild { next_generation: 2 });
    let begin_two_outcome = coordinator
        .commit(
            &mut filesystem,
            &reopened_actor,
            request(12, &begin_two, None),
            &mut clock(13),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .read(&reopened_actor, &after_revoke, &revoked_history)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::StaleView)
    );
    let rebuilding_view = coordinator.read_view(&reopened_actor).unwrap();
    let rebuilding_read = MemoryReadRequest::Search {
        scope: scope(),
        authority_generation: 2,
        terms: vec!["alpha".to_owned()],
        knowledge: KnowledgeAt::Current,
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator
            .read(&reopened_actor, &rebuilding_view, &rebuilding_read)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::Rebuilding)
    );
    drop(coordinator);
    filesystem.restart().unwrap();
    let rebuilding_kernel = policy();
    let rebuilding_actor = principal(&rebuilding_kernel);
    let (mut coordinator, report) = open_authorized(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(50),
        CounterEntropy(60),
        &mut TestKeyAdapter,
        MemoryState::new(scope()),
        rebuilding_kernel,
    )
    .unwrap();
    assert_eq!(report.frontier, Some(begin_two_outcome.revision));
    let rebuilding_view = coordinator.read_view(&rebuilding_actor).unwrap();
    assert_eq!(
        coordinator
            .read(&rebuilding_actor, &rebuilding_view, &rebuilding_read)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::Rebuilding)
    );

    let rebuild_source = transaction(
        2,
        MemoryMutation::PutSource(SourceVersionInput {
            id: source_id,
            blob: reference,
            media_type: "text/plain; charset=utf-8".to_owned(),
            exact_utf8: Some(String::from_utf8(text.to_vec()).unwrap()),
            source_event_time: Some(UtcInstant::new(1_700_000_000, 0).unwrap()),
        }),
    );
    coordinator
        .commit(
            &mut filesystem,
            &rebuilding_actor,
            request(13, &rebuild_source, Some(&inventory)),
            &mut clock(14),
            &NeverCancel,
        )
        .unwrap();
    let rebuild_fact = transaction(
        2,
        MemoryMutation::PutFact(FactInput {
            id: record(20),
            subject: "project".to_owned(),
            predicate: "status".to_owned(),
            value: "alpha".to_owned(),
            links: Vec::new(),
            source: source_id,
            locator: SourceLocator::Utf8Lines {
                start: 0,
                end: 16,
                first_line: 1,
                last_line: 1,
            },
            source_event_time: Some(UtcInstant::new(1_700_000_001, 0).unwrap()),
        }),
    );
    coordinator
        .commit(
            &mut filesystem,
            &rebuilding_actor,
            request(14, &rebuild_fact, None),
            &mut clock(15),
            &NeverCancel,
        )
        .unwrap();
    let complete_two = transaction(2, MemoryMutation::CompleteRebuild);
    coordinator
        .commit(
            &mut filesystem,
            &rebuilding_actor,
            request(15, &complete_two, None),
            &mut clock(16),
            &NeverCancel,
        )
        .unwrap();
    let rebuilt_view = coordinator.read_view(&rebuilding_actor).unwrap();
    let MemoryReadOutput::Search(rebuilt) = coordinator
        .read(&rebuilding_actor, &rebuilt_view, &rebuilding_read)
        .unwrap()
    else {
        panic!("rebuilt search output")
    };
    assert_eq!(rebuilt.facts.len(), 1);
    assert_eq!(rebuilt.facts[0].id, record(20));

    // The trusted adapter may use the namespace-scoped blob API after the memory
    // projection has authorized the owning source. Consumer callers never receive
    // this raw capability.
    let mut exact = vec![0_u8; text.len()];
    assert_eq!(
        coordinator
            .read_blob_range(&mut filesystem, &rebuilding_actor, reference, 0, &mut exact)
            .unwrap(),
        text.len()
    );
    assert_eq!(exact, text);

    coordinator
        .replace_namespace_policy(
            &rebuilding_actor,
            PolicyVersion::new(1).unwrap(),
            namespace_policy(2, Some(record(10).record())),
        )
        .unwrap();
    assert_eq!(
        coordinator
            .read(&rebuilding_actor, &rebuilt_view, &rebuilding_read)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Authorization(AuthorizedError::StalePolicy)
    );
    let denied_view = coordinator.read_view(&rebuilding_actor).unwrap();
    let MemoryReadOutput::Search(denied) = coordinator
        .read(&rebuilding_actor, &denied_view, &rebuilding_read)
        .unwrap()
    else {
        panic!("denied search output")
    };
    assert!(denied.facts.is_empty());
    let denied_citation = MemoryReadRequest::ResolveCitation {
        authority_generation: 2,
        fact: record(20),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    assert_eq!(
        coordinator
            .read(&rebuilding_actor, &denied_view, &denied_citation)
            .unwrap_err(),
        uste_txn::AuthorizedReadError::Domain(MemoryReadError::NotFound)
    );
}

/// Deliberately separate scan oracle: it knows only fixture revisions and strings, not
/// `MemoryState`, query helpers, eligibility code or indexes.
fn independent_oracle_ids(
    knowledge_revision: u64,
    source_one_superseded_revision: u64,
    term: &str,
) -> Vec<RecordRef> {
    struct OracleFact {
        id: u8,
        created: u64,
        terminal: Option<u64>,
        text: &'static str,
    }
    let facts = [
        OracleFact {
            id: 20,
            created: 3,
            terminal: Some(6),
            text: "project status alpha",
        },
        OracleFact {
            id: 21,
            created: 6,
            terminal: None,
            text: "project status beta",
        },
        OracleFact {
            id: 30,
            created: 7,
            terminal: None,
            text: "project status alpha",
        },
        OracleFact {
            id: 40,
            created: 8,
            terminal: None,
            text: "note supports status evidence",
        },
    ];
    facts
        .into_iter()
        .filter(|fact| {
            fact.created <= knowledge_revision
                && fact
                    .terminal
                    .is_none_or(|terminal| terminal > knowledge_revision)
                && source_one_superseded_revision > knowledge_revision
                && fact.text.contains(term)
        })
        .map(|fact| record(fact.id))
        .collect()
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

fn create_vault(database: DatabaseId, seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
    KeyVault::create(database, &mut TestKeyAdapter, CounterEntropy(seed)).unwrap()
}
