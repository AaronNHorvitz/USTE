//! Bounded authorized reads over `research-memory-v1` (DB-R02.4).
//!
//! Every candidate record is routed through the caller's authorization callback before it can
//! contribute to a result, so hidden records add neither content nor counts. Revocation applies to
//! every read that follows it, including historical knowledge views: a revoked source version never
//! returns excerpt text, and a claim whose visible citations are all revoked is reported as
//! unsupported. Freshness is evaluated at a caller-supplied instant; nothing is ever refetched.

use uste_policy::{Action, AuthorizationRequirement, AuthorizationRequirements, Target};
use uste_txn::{ApplyError, AuthorizedReadState};
use uste_types::{CommitRevision, NamespaceRef, RecordRef, UtcInstant};

use super::state::{ClaimStatus, ResearchClaimEntry, ResearchSourceEntry, ResearchState};
use super::{
    CitationInput, ClaimInput, EdgeInput, EdgeKind, FetchOutcome, Freshness, RESEARCH_PROFILE,
    SupportKind,
};
use crate::SourceVersionId;

/// Upper bound on accepted query terms and on each term's bytes.
pub const MAXIMUM_RESEARCH_QUERY_TERMS: usize = 8;
pub const MAXIMUM_RESEARCH_QUERY_TERM_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResearchKnowledge {
    Current,
    Revision(CommitRevision),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResearchReadRequest {
    /// One claim with its citations, freshness and effective support.
    GetClaim {
        authority_generation: u64,
        claim: RecordRef,
        knowledge: ResearchKnowledge,
        evaluated_at: UtcInstant,
        maximum_output_bytes: usize,
    },
    /// Versions of one source visible at the knowledge revision, oldest first.
    SourceVersions {
        authority_generation: u64,
        source: RecordRef,
        knowledge: ResearchKnowledge,
        evaluated_at: UtcInstant,
        maximum_results: usize,
    },
    /// Edges leaving one record, optionally of one kind, in edge-identity order.
    EdgesFrom {
        authority_generation: u64,
        from: RecordRef,
        kind: Option<EdgeKind>,
        knowledge: ResearchKnowledge,
        maximum_candidates: usize,
        maximum_results: usize,
    },
    /// Active claims whose fields or visible excerpts contain every ASCII-case-folded term.
    Search {
        scope: NamespaceRef,
        authority_generation: u64,
        terms: Vec<String>,
        knowledge: ResearchKnowledge,
        evaluated_at: UtcInstant,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FreshnessState {
    Fresh,
    Stale {
        age_seconds: u64,
    },
    /// The evaluation instant precedes retrieval, so no age can be stated.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveSupport {
    AsRecorded(SupportKind),
    /// The claim was recorded without a citation.
    RecordedUnsupported,
    /// Every visible citation names a revoked source version; the latest revocation revision.
    Revoked(CommitRevision),
    /// The claim has citations, but none is visible to this caller.
    NoVisibleCitation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationView {
    pub source: SourceVersionId,
    pub locator: crate::SourceLocator,
    pub excerpt_digest: [u8; 32],
    /// Withheld when the source version is revoked.
    pub excerpt: Option<String>,
    pub revoked_revision: Option<CommitRevision>,
    pub freshness: FreshnessState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimView {
    pub id: RecordRef,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub recorded_support: SupportKind,
    pub effective_support: EffectiveSupport,
    pub status: ClaimStatus,
    pub recorded_revision: CommitRevision,
    pub corrects: Option<RecordRef>,
    pub citations: Vec<CitationView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceVersionView {
    pub id: SourceVersionId,
    pub locator_text: String,
    pub version_label: String,
    pub retrieved_at: UtcInstant,
    pub outcome: FetchOutcome,
    pub retained_bytes: Option<u64>,
    pub content_digest: Option<[u8; 32]>,
    pub recorded_revision: CommitRevision,
    pub superseded_revision: Option<CommitRevision>,
    pub revoked_revision: Option<CommitRevision>,
    pub freshness: FreshnessState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchReadResults<T> {
    pub knowledge_revision: CommitRevision,
    pub items: Vec<T>,
    pub visited_candidates: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResearchReadOutput {
    Claim(ClaimView),
    SourceVersions(ResearchReadResults<SourceVersionView>),
    Edges(ResearchReadResults<EdgeInput>),
    Search(ResearchReadResults<ClaimView>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResearchReadError {
    /// The read view predates a newer commit; take a fresh view.
    StaleView,
    StaleGeneration,
    Rebuilding,
    HistoryUnavailable,
    NotFound,
    ResourceLimit,
    UnsupportedQuery,
}

impl AuthorizedReadState for ResearchState {
    type ReadRequest = ResearchReadRequest;
    type ReadOutput = ResearchReadOutput;
    type ReadError = ResearchReadError;

    fn read_authorization_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let record = |action, reference| AuthorizationRequirement {
            action,
            target: Target::Record(reference),
        };
        let requirements = match request {
            ResearchReadRequest::GetClaim { claim, .. } => {
                vec![record(Action::ReadRecord, *claim)]
            }
            ResearchReadRequest::SourceVersions { source, .. } => vec![
                record(Action::ReadRecord, *source),
                record(Action::ReadHistory, *source),
            ],
            ResearchReadRequest::EdgesFrom { from, .. } => vec![
                record(Action::ReadRecord, *from),
                record(Action::ExpandGraph, *from),
            ],
            ResearchReadRequest::Search { scope, .. } => vec![AuthorizationRequirement {
                action: Action::Search,
                target: Target::Namespace(*scope),
            }],
        };
        AuthorizationRequirements::new(requirements).map_err(|_| ApplyError::ResourceLimit)
    }

    fn read_authorized(
        snapshot: &Self::Snapshot,
        request: &Self::ReadRequest,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError> {
        match request {
            ResearchReadRequest::GetClaim {
                authority_generation,
                claim,
                knowledge,
                evaluated_at,
                maximum_output_bytes,
            } => {
                snapshot.ensure_queryable(*authority_generation)?;
                check_output(*maximum_output_bytes)?;
                let revision = snapshot.resolve(*knowledge)?;
                let entry = snapshot
                    .visible_claim(*claim, revision, authorize)
                    .ok_or(ResearchReadError::NotFound)?;
                let view = snapshot.claim_view(entry, revision, *evaluated_at, authorize);
                if claim_view_bytes(&view) > *maximum_output_bytes {
                    return Err(ResearchReadError::ResourceLimit);
                }
                Ok(ResearchReadOutput::Claim(view))
            }
            ResearchReadRequest::SourceVersions {
                authority_generation,
                source,
                knowledge,
                evaluated_at,
                maximum_results,
            } => {
                snapshot.ensure_queryable(*authority_generation)?;
                check_count(*maximum_results, RESEARCH_PROFILE.maximum_query_results)?;
                let revision = snapshot.resolve(*knowledge)?;
                let mut items = Vec::new();
                let mut visited = 0;
                let mut truncated = false;
                for (id, entry) in snapshot.sources().range(
                    SourceVersionId {
                        source: *source,
                        version: 0,
                    }..=SourceVersionId {
                        source: *source,
                        version: u32::MAX,
                    },
                ) {
                    if entry.recorded_revision > revision {
                        break;
                    }
                    visited += 1;
                    if items.len() == *maximum_results {
                        truncated = true;
                        break;
                    }
                    items.push(source_view(*id, entry, revision, *evaluated_at));
                }
                if items.is_empty() {
                    return Err(ResearchReadError::NotFound);
                }
                Ok(ResearchReadOutput::SourceVersions(ResearchReadResults {
                    knowledge_revision: revision,
                    items,
                    visited_candidates: visited,
                    truncated,
                }))
            }
            ResearchReadRequest::EdgesFrom {
                authority_generation,
                from,
                kind,
                knowledge,
                maximum_candidates,
                maximum_results,
            } => {
                snapshot.ensure_queryable(*authority_generation)?;
                check_count(
                    *maximum_candidates,
                    RESEARCH_PROFILE.maximum_query_candidates,
                )?;
                check_count(*maximum_results, RESEARCH_PROFILE.maximum_query_results)?;
                let revision = snapshot.resolve(*knowledge)?;
                let mut items = Vec::new();
                let mut visited = 0;
                let mut truncated = false;
                for entry in snapshot.edges().values() {
                    if entry.input.from != *from
                        || entry.recorded_revision > revision
                        || kind.is_some_and(|kind| kind != entry.input.kind)
                    {
                        continue;
                    }
                    let edge = &entry.input;
                    // Hidden edges contribute neither content nor counts.
                    if [edge.id, edge.to, edge.asserted_by]
                        .into_iter()
                        .any(|reference| !authorize(Action::ReadRecord, Target::Record(reference)))
                    {
                        continue;
                    }
                    if visited == *maximum_candidates {
                        truncated = true;
                        break;
                    }
                    visited += 1;
                    if items.len() == *maximum_results {
                        truncated = true;
                        break;
                    }
                    items.push(edge.clone());
                }
                Ok(ResearchReadOutput::Edges(ResearchReadResults {
                    knowledge_revision: revision,
                    items,
                    visited_candidates: visited,
                    truncated,
                }))
            }
            ResearchReadRequest::Search {
                scope,
                authority_generation,
                terms,
                knowledge,
                evaluated_at,
                maximum_candidates,
                maximum_results,
                maximum_output_bytes,
            } => {
                if *scope != snapshot.scope() {
                    return Err(ResearchReadError::NotFound);
                }
                snapshot.ensure_queryable(*authority_generation)?;
                snapshot.search(
                    terms,
                    *knowledge,
                    *evaluated_at,
                    *maximum_candidates,
                    *maximum_results,
                    *maximum_output_bytes,
                    authorize,
                )
            }
        }
    }
}

fn check_count(requested: usize, maximum: usize) -> Result<(), ResearchReadError> {
    if requested == 0 || requested > maximum {
        Err(ResearchReadError::ResourceLimit)
    } else {
        Ok(())
    }
}

fn check_output(requested: usize) -> Result<(), ResearchReadError> {
    check_count(requested, RESEARCH_PROFILE.maximum_query_output_bytes)
}

fn freshness(
    policy: Freshness,
    retrieved_at: UtcInstant,
    evaluated_at: UtcInstant,
) -> FreshnessState {
    if evaluated_at < retrieved_at {
        return FreshnessState::Unknown;
    }
    match policy {
        Freshness::Pinned => FreshnessState::Fresh,
        Freshness::MaxAge { seconds } => {
            // Both instants are within the admitted calendar range, so the difference fits.
            let age = evaluated_at.seconds().abs_diff(retrieved_at.seconds());
            if age > seconds {
                FreshnessState::Stale { age_seconds: age }
            } else {
                FreshnessState::Fresh
            }
        }
    }
}

fn source_view(
    id: SourceVersionId,
    entry: &ResearchSourceEntry,
    revision: CommitRevision,
    evaluated_at: UtcInstant,
) -> SourceVersionView {
    let input = &entry.input;
    SourceVersionView {
        id,
        locator_text: input.locator_text.clone(),
        version_label: input.version_label.clone(),
        retrieved_at: input.retrieved_at,
        outcome: input.outcome.clone(),
        retained_bytes: input
            .content
            .as_ref()
            .map(|content| content.blob.byte_len()),
        content_digest: input
            .content
            .as_ref()
            .map(|content| content.blob.content_digest()),
        recorded_revision: entry.recorded_revision,
        superseded_revision: entry
            .superseded_revision
            .filter(|superseded| *superseded <= revision),
        // Revocation applies to every later read, including historical views.
        revoked_revision: entry.revoked_revision,
        freshness: freshness(input.freshness, input.retrieved_at, evaluated_at),
    }
}

fn status_at(entry: &ResearchClaimEntry, revision: CommitRevision) -> ClaimStatus {
    match entry.status {
        ClaimStatus::Superseded(at) | ClaimStatus::Retracted(at) | ClaimStatus::Expired(at)
            if at <= revision =>
        {
            entry.status
        }
        _ => ClaimStatus::Active,
    }
}

fn claim_view_bytes(view: &ClaimView) -> usize {
    view.subject.len()
        + view.predicate.len()
        + view.value.len()
        + view
            .citations
            .iter()
            .map(|citation| 64 + citation.excerpt.as_ref().map_or(0, String::len))
            .sum::<usize>()
        + 128
}

impl ResearchState {
    fn ensure_queryable(&self, generation: u64) -> Result<(), ResearchReadError> {
        if !self.is_runtime_current() {
            return Err(ResearchReadError::StaleView);
        }
        if self.generation() != Some(generation) {
            return Err(ResearchReadError::StaleGeneration);
        }
        if !self.is_ready() {
            return Err(ResearchReadError::Rebuilding);
        }
        Ok(())
    }

    fn resolve(&self, knowledge: ResearchKnowledge) -> Result<CommitRevision, ResearchReadError> {
        let current = self
            .current_revision()
            .ok_or(ResearchReadError::HistoryUnavailable)?;
        let revision = match knowledge {
            ResearchKnowledge::Current => current,
            ResearchKnowledge::Revision(revision) => revision,
        };
        if revision > current || self.retained_from().is_none_or(|start| revision < start) {
            Err(ResearchReadError::HistoryUnavailable)
        } else {
            Ok(revision)
        }
    }

    fn visible_claim(
        &self,
        id: RecordRef,
        revision: CommitRevision,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Option<&ResearchClaimEntry> {
        let entry = self.claims().get(&id)?;
        (entry.recorded_revision <= revision && authorize(Action::ReadRecord, Target::Record(id)))
            .then_some(entry)
    }

    fn claim_view(
        &self,
        entry: &ResearchClaimEntry,
        revision: CommitRevision,
        evaluated_at: UtcInstant,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> ClaimView {
        let input: &ClaimInput = &entry.input;
        let mut citations = Vec::new();
        for citation in &input.citations {
            if let Some(view) = self.citation_view(citation, evaluated_at, authorize) {
                citations.push(view);
            }
        }
        let effective_support = if input.support == SupportKind::Unsupported {
            EffectiveSupport::RecordedUnsupported
        } else if citations.is_empty() {
            EffectiveSupport::NoVisibleCitation
        } else if let Some(latest) = citations
            .iter()
            .map(|citation| citation.revoked_revision)
            .collect::<Option<Vec<_>>>()
            .and_then(|revocations| revocations.into_iter().max())
        {
            EffectiveSupport::Revoked(latest)
        } else {
            EffectiveSupport::AsRecorded(input.support)
        };
        ClaimView {
            id: input.id,
            subject: input.subject.clone(),
            predicate: input.predicate.clone(),
            value: input.value.clone(),
            recorded_support: input.support,
            effective_support,
            status: status_at(entry, revision),
            recorded_revision: entry.recorded_revision,
            corrects: input.corrects,
            citations,
        }
    }

    fn citation_view(
        &self,
        citation: &CitationInput,
        evaluated_at: UtcInstant,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Option<CitationView> {
        if !authorize(Action::ReadRecord, Target::Record(citation.source.source)) {
            return None;
        }
        let source = self.sources().get(&citation.source)?;
        let revoked = source.revoked_revision;
        Some(CitationView {
            source: citation.source,
            locator: citation.locator,
            excerpt_digest: citation.excerpt_digest,
            excerpt: if revoked.is_some() {
                None
            } else {
                citation.excerpt.clone()
            },
            revoked_revision: revoked,
            freshness: freshness(
                source.input.freshness,
                source.input.retrieved_at,
                evaluated_at,
            ),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        terms: &[String],
        knowledge: ResearchKnowledge,
        evaluated_at: UtcInstant,
        maximum_candidates: usize,
        maximum_results: usize,
        maximum_output_bytes: usize,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<ResearchReadOutput, ResearchReadError> {
        if terms.is_empty() || terms.len() > MAXIMUM_RESEARCH_QUERY_TERMS {
            return Err(ResearchReadError::UnsupportedQuery);
        }
        if terms
            .iter()
            .any(|term| term.is_empty() || term.len() > MAXIMUM_RESEARCH_QUERY_TERM_BYTES)
        {
            return Err(ResearchReadError::UnsupportedQuery);
        }
        check_count(
            maximum_candidates,
            RESEARCH_PROFILE.maximum_query_candidates,
        )?;
        check_count(maximum_results, RESEARCH_PROFILE.maximum_query_results)?;
        check_output(maximum_output_bytes)?;
        let revision = self.resolve(knowledge)?;
        let terms: Vec<String> = terms.iter().map(|term| term.to_ascii_lowercase()).collect();
        let mut items = Vec::new();
        let mut visited = 0;
        let mut output = 0;
        let mut truncated = false;
        for entry in self.claims().values() {
            if entry.recorded_revision > revision
                || status_at(entry, revision) != ClaimStatus::Active
            {
                continue;
            }
            // Hidden claims contribute neither content nor counts.
            if !authorize(Action::ReadRecord, Target::Record(entry.input.id)) {
                continue;
            }
            if visited == maximum_candidates {
                truncated = true;
                break;
            }
            visited += 1;
            let view = self.claim_view(entry, revision, evaluated_at, authorize);
            let mut text = format!("{} {} {}", view.subject, view.predicate, view.value);
            for citation in &view.citations {
                if let Some(excerpt) = &citation.excerpt {
                    text.push(' ');
                    text.push_str(excerpt);
                }
            }
            let text = text.to_ascii_lowercase();
            if !terms.iter().all(|term| text.contains(term.as_str())) {
                continue;
            }
            let bytes = claim_view_bytes(&view);
            if items.len() == maximum_results || output + bytes > maximum_output_bytes {
                truncated = true;
                break;
            }
            output += bytes;
            items.push(view);
        }
        Ok(ResearchReadOutput::Search(ResearchReadResults {
            knowledge_revision: revision,
            items,
            visited_candidates: visited,
            truncated,
        }))
    }
}

#[cfg(test)]
mod tests;
