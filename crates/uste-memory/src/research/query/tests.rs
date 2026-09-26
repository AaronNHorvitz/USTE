use sha2::{Digest, Sha256};
use uste_policy::{Action, Target};
use uste_storage::{BlobId, BlobInventory, BlobReference};
use uste_txn::{AuthorizedReadState, TransactionState};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant,
};

use super::*;
use crate::research::state::{ResearchMutation, ResearchTransaction, encode_research_transaction};
use crate::research::{
    CitationInput, ClaimInput, EdgeInput, EdgeKind, FetchOutcome, Freshness, ResearchRecord,
    RetainedContent, SourceKind, SourceRecordInput, SupportKind,
};
use crate::{SourceLocator, SourceVersionId};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn at(seconds: i64) -> UtcInstant {
    UtcInstant::new(seconds, 0).unwrap()
}

const RETRIEVED: i64 = 1_790_000_000;
const PAGE: &str = "alpha protocol uses journal authority; beta cache is derived";

fn source(source: u8, version: u32, freshness: Freshness) -> SourceRecordInput {
    SourceRecordInput {
        id: SourceVersionId {
            source: id(source),
            version,
        },
        kind: SourceKind::WebPage,
        locator_text: format!("https://example.invalid/{source}/{version}"),
        version_label: "unversioned".to_owned(),
        retrieved_at: at(RETRIEVED),
        run_identity: [3; 16],
        route_label: "fake-provider".to_owned(),
        outcome: FetchOutcome::Complete,
        content: Some(RetainedContent {
            blob: BlobReference::new(
                scope(),
                BlobId::from_bytes([source.wrapping_add(u8::try_from(version).unwrap()); 16]),
                u64::try_from(PAGE.len()).unwrap(),
                1,
                Sha256::digest(PAGE.as_bytes()).into(),
            )
            .unwrap(),
            media_type: "text/plain".to_owned(),
        }),
        license_label: "synthetic".to_owned(),
        redistributable: false,
        freshness,
    }
}

fn citation(source: u8, start: u64, end: u64) -> CitationInput {
    let excerpt = &PAGE[usize::try_from(start).unwrap()..usize::try_from(end).unwrap()];
    CitationInput {
        source: SourceVersionId {
            source: id(source),
            version: 1,
        },
        locator: SourceLocator::ByteRange { start, end },
        excerpt_digest: Sha256::digest(excerpt.as_bytes()).into(),
        excerpt: Some(excerpt.to_owned()),
    }
}

fn claim(
    claim: u8,
    value: &str,
    citations: Vec<CitationInput>,
    corrects: Option<u8>,
) -> ClaimInput {
    ClaimInput {
        id: id(claim),
        subject: "protocol".to_owned(),
        predicate: "states".to_owned(),
        value: value.to_owned(),
        support: if citations.is_empty() {
            SupportKind::Unsupported
        } else {
            SupportKind::DirectQuote
        },
        citations,
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

struct Built {
    state: ResearchState,
    revisions: Vec<CommitRevision>,
}

fn build(mutations: Vec<ResearchMutation>) -> Built {
    let mut state = ResearchState::new(scope());
    let mut revisions = Vec::new();
    let all = std::iter::once(ResearchMutation::BeginRebuild { next_generation: 1 })
        .chain(mutations)
        .chain(std::iter::once(ResearchMutation::CompleteRebuild));
    for (index, mutation) in all.enumerate() {
        let inventory = match &mutation {
            ResearchMutation::Put(record) => match &**record {
                ResearchRecord::Source(source) => source
                    .content
                    .as_ref()
                    .map(|content| BlobInventory::new(scope(), [content.blob]).unwrap()),
                _ => None,
            },
            _ => None,
        };
        let bytes = encode_research_transaction(&ResearchTransaction {
            scope: scope(),
            generation: 1,
            mutation,
        })
        .unwrap();
        let revision = CommitRevision::new(u64::try_from(index).unwrap() + 1).unwrap();
        let prepared = state.prepare(&bytes, inventory.as_ref(), revision).unwrap();
        state.publish(prepared);
        revisions.push(revision);
    }
    Built { state, revisions }
}

fn put(record: ResearchRecord) -> ResearchMutation {
    ResearchMutation::Put(Box::new(record))
}

fn allow_all() -> impl FnMut(Action, Target) -> bool {
    |_, _| true
}

fn deny(hidden: RecordRef) -> impl FnMut(Action, Target) -> bool {
    move |_, target| target != Target::Record(hidden)
}

fn get_claim(
    state: &ResearchState,
    claim: u8,
    knowledge: ResearchKnowledge,
    evaluated_at: i64,
    authorize: &mut dyn FnMut(Action, Target) -> bool,
) -> Result<ClaimView, ResearchReadError> {
    match ResearchState::read_authorized(
        state,
        &ResearchReadRequest::GetClaim {
            authority_generation: 1,
            claim: id(claim),
            knowledge,
            evaluated_at: at(evaluated_at),
            maximum_output_bytes: 64 * 1024,
        },
        authorize,
    )? {
        ResearchReadOutput::Claim(view) => Ok(view),
        other => panic!("unexpected output {other:?}"),
    }
}

fn fixture() -> Built {
    build(vec![
        put(ResearchRecord::Source(source(
            10,
            1,
            Freshness::MaxAge { seconds: 3_600 },
        ))),
        put(ResearchRecord::Source(source(11, 1, Freshness::Pinned))),
        // Revision 4: a quote from each source.
        put(ResearchRecord::Claim(claim(
            20,
            "journal authority",
            vec![citation(10, 20, 37), citation(11, 20, 37)],
            None,
        ))),
        // Revision 5: correction of claim 20.
        put(ResearchRecord::Claim(claim(
            21,
            "derived cache",
            vec![citation(10, 39, 60)],
            Some(20),
        ))),
        put(ResearchRecord::Claim(claim(
            22,
            "unsourced guess",
            Vec::new(),
            None,
        ))),
        put(ResearchRecord::Edge(edge(30, 21, 20, EdgeKind::Corrects))),
        put(ResearchRecord::Edge(edge(31, 21, 22, EdgeKind::Related))),
        put(ResearchRecord::Edge(edge(
            32,
            21,
            10,
            EdgeKind::DerivedFrom,
        ))),
        // Revision 10: revoke the source cited by claim 21.
        ResearchMutation::RevokeSource {
            source: SourceVersionId {
                source: id(10),
                version: 1,
            },
        },
        put(ResearchRecord::Source(source(11, 2, Freshness::Pinned))),
    ])
}

#[test]
fn claims_report_freshness_support_revocation_and_history() {
    let built = fixture();
    let state = &built.state;
    let fresh = get_claim(
        state,
        20,
        ResearchKnowledge::Current,
        RETRIEVED + 60,
        &mut allow_all(),
    )
    .unwrap();
    assert_eq!(fresh.status, ClaimStatus::Superseded(built.revisions[4]));
    // One of two citations is revoked; the claim stays supported by the other.
    assert_eq!(
        fresh.effective_support,
        EffectiveSupport::AsRecorded(SupportKind::DirectQuote)
    );
    assert_eq!(fresh.citations[0].excerpt, None);
    assert_eq!(
        fresh.citations[0].revoked_revision,
        Some(built.revisions[9])
    );
    assert_eq!(fresh.citations[0].freshness, FreshnessState::Fresh);
    assert_eq!(
        fresh.citations[1].excerpt.as_deref(),
        Some("journal authority")
    );
    let stale = get_claim(
        state,
        20,
        ResearchKnowledge::Current,
        RETRIEVED + 7_200,
        &mut allow_all(),
    )
    .unwrap();
    assert_eq!(
        stale.citations[0].freshness,
        FreshnessState::Stale { age_seconds: 7_200 }
    );
    assert_eq!(stale.citations[1].freshness, FreshnessState::Fresh);
    let early = get_claim(
        state,
        20,
        ResearchKnowledge::Current,
        RETRIEVED - 1,
        &mut allow_all(),
    )
    .unwrap();
    assert_eq!(early.citations[1].freshness, FreshnessState::Unknown);

    // Every visible citation of claim 21 is revoked, so it reads as unsupported.
    let revoked = get_claim(
        state,
        21,
        ResearchKnowledge::Current,
        RETRIEVED,
        &mut allow_all(),
    )
    .unwrap();
    assert_eq!(
        revoked.effective_support,
        EffectiveSupport::Revoked(built.revisions[9])
    );
    assert!(
        revoked
            .citations
            .iter()
            .all(|citation| citation.excerpt.is_none())
    );
    // Revocation also applies to historical knowledge views read after it.
    let historical = get_claim(
        state,
        21,
        ResearchKnowledge::Revision(built.revisions[5]),
        RETRIEVED,
        &mut allow_all(),
    )
    .unwrap();
    assert_eq!(
        historical.effective_support,
        EffectiveSupport::Revoked(built.revisions[9])
    );
    assert_eq!(
        get_claim(
            state,
            22,
            ResearchKnowledge::Current,
            RETRIEVED,
            &mut allow_all()
        )
        .unwrap()
        .effective_support,
        EffectiveSupport::RecordedUnsupported
    );
    // A citation whose source is hidden is omitted; the claim then has no visible citation.
    let hidden_source = get_claim(
        state,
        21,
        ResearchKnowledge::Current,
        RETRIEVED,
        &mut deny(id(10)),
    )
    .unwrap();
    assert!(hidden_source.citations.is_empty());
    assert_eq!(
        hidden_source.effective_support,
        EffectiveSupport::NoVisibleCitation
    );
    // Before the correction, claim 20 was active; before it was recorded, it is not found.
    assert_eq!(
        get_claim(
            state,
            20,
            ResearchKnowledge::Revision(built.revisions[3]),
            RETRIEVED,
            &mut allow_all()
        )
        .unwrap()
        .status,
        ClaimStatus::Active
    );
    assert_eq!(
        get_claim(
            state,
            20,
            ResearchKnowledge::Revision(built.revisions[2]),
            RETRIEVED,
            &mut allow_all()
        ),
        Err(ResearchReadError::NotFound)
    );
    assert_eq!(
        get_claim(
            state,
            20,
            ResearchKnowledge::Current,
            RETRIEVED,
            &mut deny(id(20))
        ),
        Err(ResearchReadError::NotFound)
    );
    let beyond = CommitRevision::new(built.revisions.last().unwrap().get() + 1).unwrap();
    assert_eq!(
        get_claim(
            state,
            20,
            ResearchKnowledge::Revision(beyond),
            RETRIEVED,
            &mut allow_all()
        ),
        Err(ResearchReadError::HistoryUnavailable)
    );
}

#[test]
fn generation_readiness_and_limits_fail_explicitly() {
    let built = fixture();
    let read = |state: &ResearchState, generation, maximum_output_bytes| {
        ResearchState::read_authorized(
            state,
            &ResearchReadRequest::GetClaim {
                authority_generation: generation,
                claim: id(20),
                knowledge: ResearchKnowledge::Current,
                evaluated_at: at(RETRIEVED),
                maximum_output_bytes,
            },
            &mut allow_all(),
        )
    };
    assert_eq!(
        read(&built.state, 2, 4096),
        Err(ResearchReadError::StaleGeneration)
    );
    assert_eq!(
        read(&built.state, 1, 0),
        Err(ResearchReadError::ResourceLimit)
    );
    assert_eq!(
        read(&built.state, 1, 16),
        Err(ResearchReadError::ResourceLimit)
    );
    let mut rebuilding = ResearchState::new(scope());
    let begin = encode_research_transaction(&ResearchTransaction {
        scope: scope(),
        generation: 1,
        mutation: ResearchMutation::BeginRebuild { next_generation: 1 },
    })
    .unwrap();
    let prepared = rebuilding
        .prepare(&begin, None, CommitRevision::new(1).unwrap())
        .unwrap();
    rebuilding.publish(prepared);
    assert_eq!(
        read(&rebuilding, 1, 4096),
        Err(ResearchReadError::Rebuilding)
    );
}

#[test]
fn source_versions_edges_and_search_are_bounded_and_authorized() {
    let built = fixture();
    let state = &built.state;
    let ResearchReadOutput::SourceVersions(versions) = ResearchState::read_authorized(
        state,
        &ResearchReadRequest::SourceVersions {
            authority_generation: 1,
            source: id(11),
            knowledge: ResearchKnowledge::Current,
            evaluated_at: at(RETRIEVED),
            maximum_results: 8,
        },
        &mut allow_all(),
    )
    .unwrap() else {
        panic!("expected source versions");
    };
    assert_eq!(versions.items.len(), 2);
    assert_eq!(
        versions.items[0].superseded_revision,
        Some(built.revisions[10])
    );
    // At an earlier knowledge revision the second version and its supersession are invisible.
    let ResearchReadOutput::SourceVersions(earlier) = ResearchState::read_authorized(
        state,
        &ResearchReadRequest::SourceVersions {
            authority_generation: 1,
            source: id(11),
            knowledge: ResearchKnowledge::Revision(built.revisions[9]),
            evaluated_at: at(RETRIEVED),
            maximum_results: 1,
        },
        &mut allow_all(),
    )
    .unwrap() else {
        panic!("expected source versions");
    };
    assert_eq!(earlier.items.len(), 1);
    assert_eq!(earlier.items[0].superseded_revision, None);
    assert!(!earlier.truncated);

    let edges = |kind, maximum_results, authorize: &mut dyn FnMut(Action, Target) -> bool| {
        let ResearchReadOutput::Edges(results) = ResearchState::read_authorized(
            state,
            &ResearchReadRequest::EdgesFrom {
                authority_generation: 1,
                from: id(21),
                kind,
                knowledge: ResearchKnowledge::Current,
                maximum_candidates: 16,
                maximum_results,
            },
            authorize,
        )
        .unwrap() else {
            panic!("expected edges");
        };
        results
    };
    let all = edges(None, 8, &mut allow_all());
    assert_eq!(
        all.items.iter().map(|edge| edge.id).collect::<Vec<_>>(),
        [id(30), id(31), id(32)]
    );
    assert_eq!(
        edges(Some(EdgeKind::Related), 8, &mut allow_all())
            .items
            .len(),
        1
    );
    let hidden = edges(None, 8, &mut deny(id(22)));
    assert_eq!(hidden.items.len(), 2);
    // A hidden target contributes no count either.
    assert_eq!(hidden.visited_candidates, 2);
    let limited = edges(None, 2, &mut allow_all());
    assert_eq!(limited.items.len(), 2);
    assert!(limited.truncated);

    let search = |terms: &[&str], authorize: &mut dyn FnMut(Action, Target) -> bool| {
        let ResearchReadOutput::Search(results) = ResearchState::read_authorized(
            state,
            &ResearchReadRequest::Search {
                scope: scope(),
                authority_generation: 1,
                terms: terms.iter().map(|term| (*term).to_owned()).collect(),
                knowledge: ResearchKnowledge::Current,
                evaluated_at: at(RETRIEVED),
                maximum_candidates: 64,
                maximum_results: 8,
                maximum_output_bytes: 64 * 1024,
            },
            authorize,
        )
        .unwrap() else {
            panic!("expected search results");
        };
        results
    };
    // Claim 20 is superseded, so only active claims 21 and 22 are searched.
    assert_eq!(
        search(&["PROTOCOL"], &mut allow_all())
            .items
            .iter()
            .map(|claim| claim.id)
            .collect::<Vec<_>>(),
        [id(21), id(22)]
    );
    // Claim 21's excerpt is revoked, so its text no longer matches excerpt-only terms.
    assert!(search(&["beta"], &mut allow_all()).items.is_empty());
    assert_eq!(
        search(&["derived", "cache"], &mut allow_all()).items.len(),
        1
    );
    let hidden = search(&["protocol"], &mut deny(id(22)));
    assert_eq!(hidden.items.len(), 1);
    assert_eq!(hidden.visited_candidates, 1);
    assert_eq!(
        ResearchState::read_authorized(
            state,
            &ResearchReadRequest::Search {
                scope: scope(),
                authority_generation: 1,
                terms: Vec::new(),
                knowledge: ResearchKnowledge::Current,
                evaluated_at: at(RETRIEVED),
                maximum_candidates: 64,
                maximum_results: 8,
                maximum_output_bytes: 4096,
            },
            &mut allow_all(),
        ),
        Err(ResearchReadError::UnsupportedQuery)
    );
}

#[test]
fn read_requirements_authorize_the_requested_record_or_namespace() {
    let requirements = |request| {
        ResearchState::read_authorization_requirements(&request)
            .unwrap()
            .iter()
            .map(|requirement| (requirement.action, requirement.target))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        requirements(ResearchReadRequest::EdgesFrom {
            authority_generation: 1,
            from: id(21),
            kind: None,
            knowledge: ResearchKnowledge::Current,
            maximum_candidates: 1,
            maximum_results: 1,
        }),
        [
            (Action::ReadRecord, Target::Record(id(21))),
            (Action::ExpandGraph, Target::Record(id(21))),
        ]
    );
    assert_eq!(
        requirements(ResearchReadRequest::SourceVersions {
            authority_generation: 1,
            source: id(11),
            knowledge: ResearchKnowledge::Current,
            evaluated_at: at(RETRIEVED),
            maximum_results: 1,
        }),
        [
            (Action::ReadRecord, Target::Record(id(11))),
            (Action::ReadHistory, Target::Record(id(11))),
        ]
    );
}

#[test]
fn source_version_confirmation_is_served_while_rebuilding_but_not_across_generations() {
    let mut state = ResearchState::new(scope());
    for (index, mutation) in [
        ResearchMutation::BeginRebuild { next_generation: 1 },
        put(ResearchRecord::Source(source(10, 1, Freshness::Pinned))),
    ]
    .into_iter()
    .enumerate()
    {
        let inventory = match &mutation {
            ResearchMutation::Put(record) => match &**record {
                ResearchRecord::Source(source) => source
                    .content
                    .as_ref()
                    .map(|content| BlobInventory::new(scope(), [content.blob]).unwrap()),
                _ => None,
            },
            _ => None,
        };
        let bytes = encode_research_transaction(&ResearchTransaction {
            scope: scope(),
            generation: 1,
            mutation,
        })
        .unwrap();
        let revision = CommitRevision::new(u64::try_from(index).unwrap() + 1).unwrap();
        let prepared = state.prepare(&bytes, inventory.as_ref(), revision).unwrap();
        state.publish(prepared);
    }
    assert!(!state.is_ready());
    let confirm = |generation, version| {
        ResearchState::read_authorized(
            &state,
            &ResearchReadRequest::SourceVersion {
                authority_generation: generation,
                id: SourceVersionId {
                    source: id(10),
                    version,
                },
                evaluated_at: at(RETRIEVED),
            },
            &mut allow_all(),
        )
    };
    let ResearchReadOutput::SourceVersion(view) = confirm(1, 1).unwrap() else {
        panic!("expected a source version");
    };
    assert_eq!(
        view.retained_bytes,
        Some(u64::try_from(PAGE.len()).unwrap())
    );
    assert_eq!(view.media_type.as_deref(), Some("text/plain"));
    assert_eq!(confirm(2, 1), Err(ResearchReadError::StaleGeneration));
    assert_eq!(confirm(1, 2), Err(ResearchReadError::NotFound));
}
