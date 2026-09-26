use sha2::{Digest, Sha256};
use uste_storage::{BlobId, BlobInventory, BlobReference};
use uste_txn::TransactionState;
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant,
};

use super::*;
use crate::research::state::{ClaimStatus, ResearchState, ResearchTransaction};
use crate::research::{SupportKind, encode_research_transaction};
use crate::{
    FactInput, MemoryMutation, MemoryState, MemoryTransaction, OPAQUE_MEDIA_TYPE, SourceLocator,
    SourceVersionId, SourceVersionInput, TEXT_MEDIA_TYPE, encode_transaction,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([6; 16]),
        NamespaceId::from_bytes([7; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn context() -> PilotMappingContext {
    PilotMappingContext {
        mapped_at: UtcInstant::new(1_795_000_000, 0).unwrap(),
        run_identity: [9; 16],
    }
}

fn text_source(source: u8, version: u32, text: &str) -> (SourceVersionInput, BlobInventory) {
    let blob = BlobReference::new(
        scope(),
        BlobId::from_bytes(
            [source
                .wrapping_mul(7)
                .wrapping_add(u8::try_from(version).unwrap()); 16],
        ),
        u64::try_from(text.len()).unwrap(),
        1,
        Sha256::digest(text.as_bytes()).into(),
    )
    .unwrap();
    (
        SourceVersionInput {
            id: SourceVersionId {
                source: id(source),
                version,
            },
            blob,
            media_type: TEXT_MEDIA_TYPE.to_owned(),
            exact_utf8: Some(text.to_owned()),
            source_event_time: Some(UtcInstant::new(1_700_000_000, 0).unwrap()),
        },
        BlobInventory::new(scope(), [blob]).unwrap(),
    )
}

fn fact(fact: u8, source: (u8, u32), start: u64, end: u64, links: Vec<u8>) -> FactInput {
    FactInput {
        id: id(fact),
        subject: "project".to_owned(),
        predicate: "status".to_owned(),
        value: format!("value {fact}"),
        links: links.into_iter().map(id).collect(),
        source: SourceVersionId {
            source: id(source.0),
            version: source.1,
        },
        locator: SourceLocator::ByteRange { start, end },
        source_event_time: None,
    }
}

struct Pilot {
    state: MemoryState,
    next: u64,
}

impl Pilot {
    fn new() -> Self {
        let mut pilot = Self {
            state: MemoryState::new(scope()),
            next: 1,
        };
        pilot.apply(MemoryMutation::BeginRebuild { next_generation: 1 }, None);
        pilot
    }

    fn apply(&mut self, mutation: MemoryMutation, inventory: Option<&BlobInventory>) {
        let bytes = encode_transaction(&MemoryTransaction {
            scope: scope(),
            generation: 1,
            mutation,
        })
        .unwrap();
        let prepared = self
            .state
            .prepare(&bytes, inventory, CommitRevision::new(self.next).unwrap())
            .unwrap();
        self.state.publish(prepared);
        self.next += 1;
    }
}

const TEXT: &str = "alpha is current\nbeta is historical\ngamma is unknown\n";

fn populated_pilot() -> Pilot {
    let mut pilot = Pilot::new();
    let (first, inventory) = text_source(10, 1, TEXT);
    pilot.apply(MemoryMutation::PutSource(first), Some(&inventory));
    let (second, inventory) = text_source(10, 2, "alpha is current\n");
    pilot.apply(MemoryMutation::PutSource(second), Some(&inventory));
    let (other, inventory) = text_source(11, 1, "delta is revoked\n");
    pilot.apply(MemoryMutation::PutSource(other), Some(&inventory));
    pilot.apply(
        MemoryMutation::PutFact(fact(20, (10, 1), 0, 16, Vec::new())),
        None,
    );
    pilot.apply(
        MemoryMutation::PutFact(fact(21, (10, 1), 17, 35, vec![20])),
        None,
    );
    pilot.apply(
        MemoryMutation::CorrectFact {
            target: id(20),
            replacement: fact(22, (10, 2), 0, 16, Vec::new()),
        },
        None,
    );
    pilot.apply(
        MemoryMutation::PutFact(fact(23, (11, 1), 0, 5, Vec::new())),
        None,
    );
    pilot.apply(MemoryMutation::RetractFact { target: id(21) }, None);
    pilot.apply(
        MemoryMutation::RevokeSource {
            source: SourceVersionId {
                source: id(11),
                version: 1,
            },
        },
        None,
    );
    pilot.apply(MemoryMutation::CompleteRebuild, None);
    pilot
}

fn replay(mapping: &PilotMapping) -> ResearchState {
    let mut state = ResearchState::new(scope());
    let mut revision = 1_u64;
    let mut commit = |state: &mut ResearchState, mutation, inventory: Option<&BlobInventory>| {
        let bytes = encode_research_transaction(&ResearchTransaction {
            scope: scope(),
            generation: 1,
            mutation,
        })
        .unwrap();
        let prepared = state
            .prepare(&bytes, inventory, CommitRevision::new(revision).unwrap())
            .unwrap();
        state.publish(prepared);
        revision += 1;
    };
    commit(
        &mut state,
        ResearchMutation::BeginRebuild { next_generation: 1 },
        None,
    );
    for mapped in &mapping.mutations {
        commit(
            &mut state,
            mapped.mutation.clone(),
            mapped.inventory.as_ref(),
        );
    }
    commit(&mut state, ResearchMutation::CompleteRebuild, None);
    state
}

#[test]
fn mapped_pilot_replays_to_an_equivalent_research_state() {
    let pilot = populated_pilot();
    let mapping = map_memory_pilot(&pilot.state, context()).unwrap();
    assert_eq!(mapping.pilot_generation, 1);
    // Three sources carried an event time; facts in this fixture did not.
    assert_eq!(mapping.dropped_event_times, 3);
    // The mapping is deterministic and leaves the pilot untouched.
    assert_eq!(map_memory_pilot(&pilot.state, context()).unwrap(), mapping);
    let research = replay(&mapping);
    assert!(research.is_ready());

    assert_eq!(research.sources().len(), pilot.state.sources().len());
    for (id, source) in pilot.state.sources() {
        let mapped = &research.sources()[id];
        assert_eq!(
            mapped.input.content.as_ref().unwrap().blob,
            source.input.blob
        );
        assert_eq!(mapped.input.retrieved_at, context().mapped_at);
        assert_eq!(mapped.input.route_label, PILOT_MAPPING_ROUTE);
        assert_eq!(
            mapped.revoked_revision.is_some(),
            source.revoked_revision.is_some()
        );
        assert_eq!(
            mapped.superseded_revision.is_some(),
            source.superseded_revision.is_some()
        );
    }
    assert_eq!(research.claims().len(), pilot.state.facts().len());
    for (id, fact) in pilot.state.facts() {
        let claim = &research.claims()[id];
        assert_eq!(claim.input.subject, fact.input.subject);
        assert_eq!(claim.input.predicate, fact.input.predicate);
        assert_eq!(claim.input.value, fact.input.value);
        assert_eq!(claim.input.corrects, fact.corrects);
        assert_eq!(claim.input.support, SupportKind::Paraphrase);
        let citation = &claim.input.citations[0];
        assert_eq!(citation.source, fact.input.source);
        assert_eq!(citation.locator, fact.input.locator);
        let expected = match fact.terminal {
            None => "active",
            Some(FactTerminal::Superseded(_)) => "superseded",
            Some(FactTerminal::Retracted(_)) => "retracted",
        };
        let actual = match claim.status {
            ClaimStatus::Active => "active",
            ClaimStatus::Superseded(_) => "superseded",
            ClaimStatus::Retracted(_) => "retracted",
            ClaimStatus::Expired(_) => "expired",
        };
        assert_eq!(actual, expected, "{id:?}");
    }
    let excerpt = &research.claims()[&id(21)].input.citations[0];
    assert_eq!(excerpt.excerpt.as_deref(), Some("beta is historical"));
    assert_eq!(
        excerpt.excerpt_digest,
        <[u8; 32]>::from(Sha256::digest(b"beta is historical"))
    );
    let edge_id = pilot_link_edge_id(id(21), id(20));
    let edge = &research.edges()[&edge_id].input;
    assert_eq!(
        (edge.kind, edge.from, edge.to, edge.asserted_by),
        (EdgeKind::Related, id(21), id(20), id(21))
    );
    assert_eq!(research.edges().len(), 1);
}

#[test]
fn unrepresentable_pilot_state_is_refused_not_invented() {
    let mut rebuilding = Pilot::new();
    assert_eq!(
        map_memory_pilot(&rebuilding.state, context()),
        Err(PilotMappingError::NotReady)
    );
    rebuilding.apply(MemoryMutation::CompleteRebuild, None);
    assert_eq!(
        map_memory_pilot(&rebuilding.state, context())
            .unwrap()
            .mutations,
        Vec::new()
    );

    let mut opaque = Pilot::new();
    let blob = BlobReference::new(
        scope(),
        BlobId::from_bytes([40; 16]),
        8,
        1,
        Sha256::digest([0; 8]).into(),
    )
    .unwrap();
    let inventory = BlobInventory::new(scope(), [blob]).unwrap();
    opaque.apply(
        MemoryMutation::PutSource(SourceVersionInput {
            id: SourceVersionId {
                source: id(12),
                version: 1,
            },
            blob,
            media_type: OPAQUE_MEDIA_TYPE.to_owned(),
            exact_utf8: None,
            source_event_time: None,
        }),
        Some(&inventory),
    );
    opaque.apply(
        MemoryMutation::PutFact(fact(30, (12, 1), 0, 4, Vec::new())),
        None,
    );
    opaque.apply(MemoryMutation::CompleteRebuild, None);
    assert_eq!(
        map_memory_pilot(&opaque.state, context()),
        Err(PilotMappingError::OpaqueCitation(id(30)))
    );

    let mut empty = Pilot::new();
    let (source, inventory) = text_source(13, 1, TEXT);
    empty.apply(MemoryMutation::PutSource(source), Some(&inventory));
    empty.apply(
        MemoryMutation::PutFact(fact(31, (13, 1), 4, 4, Vec::new())),
        None,
    );
    empty.apply(MemoryMutation::CompleteRebuild, None);
    assert_eq!(
        map_memory_pilot(&empty.state, context()),
        Err(PilotMappingError::EmptyLocator(id(31)))
    );
}
