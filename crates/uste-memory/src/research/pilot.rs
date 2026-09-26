//! Read-only mapping from `memory-pilot-v1` state to `research-memory-v1` mutations (DB-R02.5).
//!
//! The mapping plans an ordered transaction sequence; it never mutates the pilot, performs no I/O
//! and is not a migration. Anything the research contract cannot state exactly is refused or
//! reported instead of invented.

use sha2::{Digest, Sha256};
use uste_storage::BlobInventory;
use uste_types::{CommitRevision, RecordId, RecordRef, UtcInstant};

use super::state::ResearchMutation;
use super::{
    CitationInput, ClaimInput, EdgeInput, EdgeKind, FetchOutcome, Freshness, RESEARCH_PROFILE,
    ResearchCodecError, ResearchRecord, RetainedContent, SourceKind, SourceRecordInput,
    SupportKind,
};
use crate::{FactRecord, FactTerminal, MemoryState, SourceVersionId, SourceVersionRecord};

/// Route label written on every mapped source.
pub const PILOT_MAPPING_ROUTE: &str = "memory-pilot-v1-mapping";
/// License label for mapped sources; the pilot recorded no license.
pub const PILOT_MAPPING_LICENSE: &str = "unrecorded-by-memory-pilot-v1";

/// Consumer-declared facts about the mapping itself. The pilot recorded no retrieval time, so
/// `mapped_at` becomes each mapped source's `retrieved_at` and is documented as such.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PilotMappingContext {
    pub mapped_at: UtcInstant,
    pub run_identity: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PilotMappingError {
    /// The pilot is mid-rebuild or has no generation.
    NotReady,
    /// A fact cites an opaque source, so its excerpt digest cannot be computed without a blob read.
    OpaqueCitation(RecordRef),
    /// A fact cites an empty span, which `research-memory-v1` cannot represent.
    EmptyLocator(RecordRef),
    /// A mapped record failed research validation.
    Codec(ResearchCodecError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PilotMappedMutation {
    pub mutation: ResearchMutation,
    /// Exactly the retained blob a mapped source must be committed with, if any.
    pub inventory: Option<BlobInventory>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PilotMapping {
    pub pilot_generation: u64,
    /// Ordered by pilot revision; replaying them after `BeginRebuild` rebuilds the lifecycle.
    pub mutations: Vec<PilotMappedMutation>,
    /// Pilot source and fact event times, which `research-memory-v1` does not carry.
    pub dropped_event_times: usize,
}

/// Deterministic identity for the related edge that represents one pilot fact link.
#[must_use]
pub fn pilot_link_edge_id(fact: RecordRef, link: RecordRef) -> RecordRef {
    let mut digest = Sha256::new();
    digest.update(b"uste-memory-pilot-link-edge-v1");
    digest.update(fact.database().as_bytes());
    digest.update(fact.namespace().as_bytes());
    digest.update(fact.record().as_bytes());
    digest.update(link.record().as_bytes());
    let bytes: [u8; 32] = digest.finalize().into();
    let mut id = [0; 16];
    id.copy_from_slice(&bytes[..16]);
    RecordRef::new(fact.database(), fact.namespace(), RecordId::from_bytes(id))
}

enum Event<'a> {
    Source(SourceVersionId, &'a SourceVersionRecord),
    Fact(&'a FactRecord),
    Retract(RecordRef),
    Revoke(SourceVersionId),
}

/// Plan the research mutations equivalent to the pilot's current generation.
pub fn map_memory_pilot(
    pilot: &MemoryState,
    context: PilotMappingContext,
) -> Result<PilotMapping, PilotMappingError> {
    let generation = pilot.generation().ok_or(PilotMappingError::NotReady)?;
    if !pilot.is_ready() {
        return Err(PilotMappingError::NotReady);
    }
    let mut events: Vec<(CommitRevision, u8, Event<'_>)> = Vec::new();
    let mut dropped_event_times = 0;
    for (id, source) in pilot.sources() {
        events.push((source.recorded_revision, 0, Event::Source(*id, source)));
        if let Some(revoked) = source.revoked_revision {
            events.push((revoked, 3, Event::Revoke(*id)));
        }
        dropped_event_times += usize::from(source.input.source_event_time.is_some());
    }
    for fact in pilot.facts().values() {
        events.push((fact.recorded_revision, 1, Event::Fact(fact)));
        if let Some(FactTerminal::Retracted(revision)) = fact.terminal {
            events.push((revision, 2, Event::Retract(fact.input.id)));
        }
        dropped_event_times += usize::from(fact.input.source_event_time.is_some());
    }
    // A stable order: pilot revision, then sources before facts before retractions before
    // revocations, then identity. Within one pilot revision only one mutation ever occurred.
    events.sort_by(|left, right| {
        (left.0, left.1)
            .cmp(&(right.0, right.1))
            .then_with(|| event_key(&left.2).cmp(&event_key(&right.2)))
    });
    let mut mutations = Vec::new();
    for (_, _, event) in events {
        match event {
            Event::Source(id, source) => mutations.push(map_source(id, source, context)?),
            Event::Fact(fact) => {
                mutations.push(PilotMappedMutation {
                    mutation: put(ResearchRecord::Claim(map_fact(pilot, fact)?)),
                    inventory: None,
                });
                for link in &fact.input.links {
                    mutations.push(PilotMappedMutation {
                        mutation: put(ResearchRecord::Edge(EdgeInput {
                            id: pilot_link_edge_id(fact.input.id, *link),
                            kind: EdgeKind::Related,
                            from: fact.input.id,
                            to: *link,
                            asserted_by: fact.input.id,
                            valid_from: None,
                            valid_until: None,
                        })),
                        inventory: None,
                    });
                }
            }
            Event::Retract(target) => mutations.push(PilotMappedMutation {
                mutation: ResearchMutation::RetractClaim { target },
                inventory: None,
            }),
            Event::Revoke(source) => mutations.push(PilotMappedMutation {
                mutation: ResearchMutation::RevokeSource { source },
                inventory: None,
            }),
        }
    }
    for mapped in &mutations {
        if let ResearchMutation::Put(record) = &mapped.mutation {
            record
                .validate(pilot.scope())
                .map_err(PilotMappingError::Codec)?;
        }
    }
    Ok(PilotMapping {
        pilot_generation: generation,
        mutations,
        dropped_event_times,
    })
}

fn event_key(event: &Event<'_>) -> (RecordRef, u32) {
    match event {
        Event::Source(id, _) | Event::Revoke(id) => (id.source, id.version),
        Event::Fact(fact) => (fact.input.id, 0),
        Event::Retract(target) => (*target, 0),
    }
}

fn put(record: ResearchRecord) -> ResearchMutation {
    ResearchMutation::Put(Box::new(record))
}

fn map_source(
    id: SourceVersionId,
    source: &SourceVersionRecord,
    context: PilotMappingContext,
) -> Result<PilotMappedMutation, PilotMappingError> {
    let blob = source.input.blob;
    let inventory = BlobInventory::new(blob.scope(), [blob])
        .map_err(|_| PilotMappingError::Codec(ResearchCodecError::Invalid))?;
    Ok(PilotMappedMutation {
        mutation: put(ResearchRecord::Source(SourceRecordInput {
            id,
            kind: SourceKind::SuppliedDocument,
            locator_text: format!("memory-pilot-v1 source version {}", id.version),
            version_label: format!("memory-pilot-v1-version-{}", id.version),
            retrieved_at: context.mapped_at,
            run_identity: context.run_identity,
            route_label: PILOT_MAPPING_ROUTE.to_owned(),
            outcome: FetchOutcome::Complete,
            content: Some(RetainedContent {
                blob,
                media_type: source.input.media_type.clone(),
            }),
            license_label: PILOT_MAPPING_LICENSE.to_owned(),
            redistributable: false,
            // A pilot source version is immutable content.
            freshness: Freshness::Pinned,
        })),
        inventory: Some(inventory),
    })
}

fn map_fact(pilot: &MemoryState, fact: &FactRecord) -> Result<ClaimInput, PilotMappingError> {
    let input = &fact.input;
    let (start, end) = input.locator.byte_range();
    if start >= end {
        return Err(PilotMappingError::EmptyLocator(input.id));
    }
    let text = pilot
        .sources()
        .get(&input.source)
        .and_then(|source| source.input.exact_utf8.as_deref())
        .ok_or(PilotMappingError::OpaqueCitation(input.id))?;
    let range = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .ok_or(PilotMappingError::Codec(ResearchCodecError::ResourceLimit))?;
    let bytes = text
        .as_bytes()
        .get(range.0..range.1)
        .ok_or(PilotMappingError::Codec(ResearchCodecError::Invalid))?;
    let excerpt = text
        .get(range.0..range.1)
        .filter(|excerpt| excerpt.len() <= RESEARCH_PROFILE.maximum_excerpt_bytes)
        .map(str::to_owned);
    Ok(ClaimInput {
        id: input.id,
        subject: input.subject.clone(),
        predicate: input.predicate.clone(),
        value: input.value.clone(),
        // A pilot fact is a consumer-stated fact backed by a located span, not a verified quote.
        support: SupportKind::Paraphrase,
        citations: vec![CitationInput {
            source: input.source,
            locator: input.locator,
            excerpt_digest: Sha256::digest(bytes).into(),
            excerpt,
        }],
        valid_from: None,
        valid_until: None,
        corrects: fact.corrects,
    })
}

#[cfg(test)]
mod tests;
