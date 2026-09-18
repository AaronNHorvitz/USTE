//! Bounded authorized reference retrieval for the memory pilot.

use uste_policy::{Action, AuthorizationRequirement, AuthorizationRequirements, Target};
use uste_storage::BlobReference;
use uste_txn::{ApplyError, AuthorizedReadState};
use uste_types::{CommitRevision, RecordRef, UtcInstant};

use crate::{FactRecord, MemoryState, PILOT_PROFILE, SourceLocator, SourceVersionId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnowledgeAt {
    Current,
    Revision(CommitRevision),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventTimeFilter {
    Any,
    Exact(UtcInstant),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryReadRequest {
    GetFact {
        authority_generation: u64,
        fact: RecordRef,
        knowledge: KnowledgeAt,
        maximum_output_bytes: usize,
    },
    Search {
        scope: uste_types::NamespaceRef,
        authority_generation: u64,
        terms: Vec<String>,
        knowledge: KnowledgeAt,
        event_time: EventTimeFilter,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
    },
    OneHop {
        authority_generation: u64,
        from: RecordRef,
        knowledge: KnowledgeAt,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
    },
    ResolveCitation {
        authority_generation: u64,
        fact: RecordRef,
        knowledge: KnowledgeAt,
        maximum_output_bytes: usize,
    },
    /// Reserved request shapes fail explicitly instead of silently dropping predicates.
    Unsupported { authority_generation: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactView {
    pub id: RecordRef,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub links: Vec<RecordRef>,
    pub source: SourceVersionId,
    pub locator: SourceLocator,
    pub source_event_time: Option<UtcInstant>,
    pub recorded_revision: CommitRevision,
    pub corrects: Option<RecordRef>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Citation {
    pub fact: RecordRef,
    pub source: SourceVersionId,
    pub blob: BlobReference,
    pub content_digest: [u8; 32],
    pub locator: SourceLocator,
    pub exact_utf8_excerpt: Option<String>,
    pub source_recorded_revision: CommitRevision,
    pub fact_recorded_revision: CommitRevision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResults {
    pub knowledge_revision: CommitRevision,
    pub facts: Vec<FactView>,
    pub visited_candidates: usize,
    pub output_bytes: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryReadOutput {
    Fact(FactView),
    Search(SearchResults),
    Citation(Citation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryReadError {
    StaleGeneration,
    StaleView,
    Rebuilding,
    HistoryUnavailable,
    NotFound,
    ResourceLimit,
    UnsupportedQuery,
    IntegrityFailure,
}

impl AuthorizedReadState for MemoryState {
    type ReadRequest = MemoryReadRequest;
    type ReadOutput = MemoryReadOutput;
    type ReadError = MemoryReadError;

    fn read_authorization_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let requirements = match request {
            MemoryReadRequest::GetFact { fact, .. } => vec![AuthorizationRequirement {
                action: Action::ReadRecord,
                target: Target::Record(*fact),
            }],
            MemoryReadRequest::Search { scope, .. } => vec![AuthorizationRequirement {
                action: Action::Search,
                target: Target::Namespace(*scope),
            }],
            MemoryReadRequest::OneHop { from, .. } => vec![
                AuthorizationRequirement {
                    action: Action::ExpandGraph,
                    target: Target::Record(*from),
                },
                AuthorizationRequirement {
                    action: Action::ReadRecord,
                    target: Target::Record(*from),
                },
            ],
            MemoryReadRequest::ResolveCitation { fact, .. } => vec![AuthorizationRequirement {
                action: Action::ReadRecord,
                target: Target::Record(*fact),
            }],
            MemoryReadRequest::Unsupported { .. } => {
                return Err(ApplyError::UnsupportedPredicate);
            }
        };
        AuthorizationRequirements::new(requirements).map_err(|_| ApplyError::ResourceLimit)
    }

    fn read_authorized(
        snapshot: &Self::Snapshot,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError> {
        snapshot.ensure_queryable(authority_generation(request))?;
        match request {
            MemoryReadRequest::GetFact {
                fact,
                knowledge,
                maximum_output_bytes,
                ..
            } => {
                check_output_limit(*maximum_output_bytes)?;
                let revision = snapshot.resolve_revision(*knowledge)?;
                let record = snapshot
                    .eligible_fact(*fact, revision, authorize_candidate)
                    .ok_or(MemoryReadError::NotFound)?;
                let view = fact_view(record);
                if fact_view_bytes(&view)? > *maximum_output_bytes {
                    return Err(MemoryReadError::ResourceLimit);
                }
                Ok(MemoryReadOutput::Fact(view))
            }
            MemoryReadRequest::Search {
                terms,
                knowledge,
                event_time,
                maximum_candidates,
                maximum_results,
                maximum_output_bytes,
                ..
            } => snapshot.search(
                terms,
                *knowledge,
                *event_time,
                *maximum_candidates,
                *maximum_results,
                *maximum_output_bytes,
                authorize_candidate,
            ),
            MemoryReadRequest::OneHop {
                from,
                knowledge,
                maximum_candidates,
                maximum_results,
                maximum_output_bytes,
                ..
            } => snapshot.one_hop(
                *from,
                *knowledge,
                *maximum_candidates,
                *maximum_results,
                *maximum_output_bytes,
                authorize_candidate,
            ),
            MemoryReadRequest::ResolveCitation {
                fact,
                knowledge,
                maximum_output_bytes,
                ..
            } => snapshot.resolve_citation(
                *fact,
                *knowledge,
                *maximum_output_bytes,
                authorize_candidate,
            ),
            MemoryReadRequest::Unsupported { .. } => Err(MemoryReadError::UnsupportedQuery),
        }
    }
}

const fn authority_generation(request: &MemoryReadRequest) -> u64 {
    match request {
        MemoryReadRequest::GetFact {
            authority_generation,
            ..
        }
        | MemoryReadRequest::Search {
            authority_generation,
            ..
        }
        | MemoryReadRequest::OneHop {
            authority_generation,
            ..
        }
        | MemoryReadRequest::ResolveCitation {
            authority_generation,
            ..
        }
        | MemoryReadRequest::Unsupported {
            authority_generation,
        } => *authority_generation,
    }
}

impl MemoryState {
    fn ensure_queryable(&self, generation: u64) -> Result<(), MemoryReadError> {
        if !self.is_runtime_current() {
            return Err(MemoryReadError::StaleView);
        }
        if self.generation() != Some(generation) {
            return Err(MemoryReadError::StaleGeneration);
        }
        if !self.is_ready() {
            return Err(MemoryReadError::Rebuilding);
        }
        Ok(())
    }

    fn resolve_revision(&self, knowledge: KnowledgeAt) -> Result<CommitRevision, MemoryReadError> {
        let current = self
            .current_revision()
            .ok_or(MemoryReadError::HistoryUnavailable)?;
        let revision = match knowledge {
            KnowledgeAt::Current => current,
            KnowledgeAt::Revision(revision) => revision,
        };
        if revision > current || self.retained_from().is_none_or(|start| revision < start) {
            Err(MemoryReadError::HistoryUnavailable)
        } else {
            Ok(revision)
        }
    }

    fn eligible_fact<'a>(
        &'a self,
        id: RecordRef,
        revision: CommitRevision,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Option<&'a FactRecord> {
        let fact = self.facts().get(&id)?;
        if fact.recorded_revision > revision
            || terminal_revision(fact).is_some_and(|terminal| terminal <= revision)
            || !authorize(Action::ReadRecord, Target::Record(id))
            || !authorize(Action::ReadRecord, Target::Record(fact.input.source.source))
            || fact
                .input
                .links
                .iter()
                .any(|link| !authorize(Action::ReadRecord, Target::Record(*link)))
        {
            return None;
        }
        let source = self.sources().get(&fact.input.source)?;
        if source.recorded_revision > revision
            || source.revoked_revision.is_some()
            || source
                .superseded_revision
                .is_some_and(|superseded| superseded <= revision)
        {
            return None;
        }
        Some(fact)
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        terms: &[String],
        knowledge: KnowledgeAt,
        event_time: EventTimeFilter,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<MemoryReadOutput, MemoryReadError> {
        if terms.is_empty() {
            return Err(MemoryReadError::UnsupportedQuery);
        }
        check_query_limits(
            terms,
            maximum_candidates,
            maximum_results,
            maximum_output_bytes,
        )?;
        let revision = self.resolve_revision(knowledge)?;
        let normalized = terms
            .iter()
            .map(|term| term.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut facts = Vec::new();
        let mut visited = 0_usize;
        let mut output_bytes = 0_usize;
        let mut truncated = false;
        for id in self.facts().keys() {
            let Some(fact) = self.eligible_fact(*id, revision, authorize) else {
                continue;
            };
            visited = visited
                .checked_add(1)
                .ok_or(MemoryReadError::ResourceLimit)?;
            if visited > maximum_candidates {
                return Err(MemoryReadError::ResourceLimit);
            }
            if !event_matches(fact.input.source_event_time, event_time)
                || !lexical_match(self, fact, &normalized)
            {
                continue;
            }
            let view = fact_view(fact);
            let bytes = fact_view_bytes(&view)?;
            if facts.len() == maximum_results
                || output_bytes
                    .checked_add(bytes)
                    .is_none_or(|value| value > maximum_output_bytes)
            {
                truncated = true;
                break;
            }
            output_bytes += bytes;
            facts.push(view);
        }
        Ok(MemoryReadOutput::Search(SearchResults {
            knowledge_revision: revision,
            facts,
            visited_candidates: visited,
            output_bytes,
            truncated,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn one_hop(
        &self,
        from: RecordRef,
        knowledge: KnowledgeAt,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<MemoryReadOutput, MemoryReadError> {
        check_query_limits(
            &[],
            maximum_candidates,
            maximum_results,
            maximum_output_bytes,
        )?;
        let revision = self.resolve_revision(knowledge)?;
        let root = self
            .eligible_fact(from, revision, authorize)
            .ok_or(MemoryReadError::NotFound)?;
        let mut facts = Vec::new();
        let mut visited = 0_usize;
        let mut output_bytes = 0_usize;
        let mut truncated = false;
        for link in &root.input.links {
            let Some(fact) = self.eligible_fact(*link, revision, authorize) else {
                continue;
            };
            visited = visited
                .checked_add(1)
                .ok_or(MemoryReadError::ResourceLimit)?;
            if visited > maximum_candidates {
                return Err(MemoryReadError::ResourceLimit);
            }
            let view = fact_view(fact);
            let bytes = fact_view_bytes(&view)?;
            if facts.len() == maximum_results
                || output_bytes
                    .checked_add(bytes)
                    .is_none_or(|value| value > maximum_output_bytes)
            {
                truncated = true;
                break;
            }
            output_bytes += bytes;
            facts.push(view);
        }
        Ok(MemoryReadOutput::Search(SearchResults {
            knowledge_revision: revision,
            facts,
            visited_candidates: visited,
            output_bytes,
            truncated,
        }))
    }

    fn resolve_citation(
        &self,
        id: RecordRef,
        knowledge: KnowledgeAt,
        maximum_output_bytes: usize,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<MemoryReadOutput, MemoryReadError> {
        check_output_limit(maximum_output_bytes)?;
        let revision = self.resolve_revision(knowledge)?;
        let fact = self
            .eligible_fact(id, revision, authorize)
            .ok_or(MemoryReadError::NotFound)?;
        if !authorize(Action::ReadBlob, Target::Record(fact.input.source.source)) {
            return Err(MemoryReadError::NotFound);
        }
        let source = self
            .sources()
            .get(&fact.input.source)
            .ok_or(MemoryReadError::IntegrityFailure)?;
        let (start, end) = fact.input.locator.byte_range();
        let excerpt = source
            .input
            .exact_utf8
            .as_ref()
            .map(|text| {
                let start = usize::try_from(start).map_err(|_| MemoryReadError::ResourceLimit)?;
                let end = usize::try_from(end).map_err(|_| MemoryReadError::ResourceLimit)?;
                text.get(start..end)
                    .map(ToOwned::to_owned)
                    .ok_or(MemoryReadError::IntegrityFailure)
            })
            .transpose()?;
        let output_bytes = excerpt.as_ref().map_or(0, String::len) + 160;
        if output_bytes > maximum_output_bytes {
            return Err(MemoryReadError::ResourceLimit);
        }
        Ok(MemoryReadOutput::Citation(Citation {
            fact: id,
            source: fact.input.source,
            blob: source.input.blob,
            content_digest: source.input.blob.content_digest(),
            locator: fact.input.locator,
            exact_utf8_excerpt: excerpt,
            source_recorded_revision: source.recorded_revision,
            fact_recorded_revision: fact.recorded_revision,
        }))
    }
}

fn terminal_revision(fact: &FactRecord) -> Option<CommitRevision> {
    fact.terminal.map(|terminal| match terminal {
        crate::FactTerminal::Superseded(revision) | crate::FactTerminal::Retracted(revision) => {
            revision
        }
    })
}

fn event_matches(value: Option<UtcInstant>, filter: EventTimeFilter) -> bool {
    match filter {
        EventTimeFilter::Any => true,
        EventTimeFilter::Exact(expected) => value == Some(expected),
        EventTimeFilter::Missing => value.is_none(),
    }
}

fn lexical_match(state: &MemoryState, fact: &FactRecord, terms: &[String]) -> bool {
    let source_text = state
        .sources()
        .get(&fact.input.source)
        .and_then(|source| source.input.exact_utf8.as_deref())
        .unwrap_or("");
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        fact.input.subject, fact.input.predicate, fact.input.value, source_text
    )
    .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn fact_view(fact: &FactRecord) -> FactView {
    FactView {
        id: fact.input.id,
        subject: fact.input.subject.clone(),
        predicate: fact.input.predicate.clone(),
        value: fact.input.value.clone(),
        links: fact.input.links.clone(),
        source: fact.input.source,
        locator: fact.input.locator,
        source_event_time: fact.input.source_event_time,
        recorded_revision: fact.recorded_revision,
        corrects: fact.corrects,
    }
}

fn fact_view_bytes(fact: &FactView) -> Result<usize, MemoryReadError> {
    192_usize
        .checked_add(fact.subject.len())
        .and_then(|value| value.checked_add(fact.predicate.len()))
        .and_then(|value| value.checked_add(fact.value.len()))
        .and_then(|value| value.checked_add(fact.links.len().checked_mul(16)?))
        .ok_or(MemoryReadError::ResourceLimit)
}

fn check_query_limits(
    terms: &[String],
    maximum_candidates: usize,
    maximum_results: usize,
    maximum_output_bytes: usize,
) -> Result<(), MemoryReadError> {
    if terms.is_empty() && maximum_candidates == 0 {
        return Err(MemoryReadError::ResourceLimit);
    }
    if terms.len() > PILOT_PROFILE.maximum_query_terms
        || terms.iter().any(|term| term.is_empty() || term.len() > 256)
        || maximum_candidates == 0
        || maximum_candidates > PILOT_PROFILE.maximum_query_candidates
        || maximum_results == 0
        || maximum_results > PILOT_PROFILE.maximum_query_results
        || maximum_output_bytes == 0
        || maximum_output_bytes > PILOT_PROFILE.maximum_query_output_bytes
    {
        return Err(MemoryReadError::ResourceLimit);
    }
    Ok(())
}

fn check_output_limit(maximum: usize) -> Result<(), MemoryReadError> {
    if maximum == 0 || maximum > PILOT_PROFILE.maximum_query_output_bytes {
        Err(MemoryReadError::ResourceLimit)
    } else {
        Ok(())
    }
}
