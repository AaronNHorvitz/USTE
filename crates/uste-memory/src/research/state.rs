//! `research-memory-v1` transaction codec and bounded reducer (DB-R02.3).
//!
//! The reducer admits research records through the ordinary journal/coordinator path. It checks
//! identity, reference closure, versions, lifecycle transitions, blob inventories and every frozen
//! budget before a change becomes visible. It never fetches, resolves, executes or authorizes
//! anything a record describes.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use uste_policy::{Action, AuthorizationRequirement, AuthorizationRequirements, Target};
use uste_storage::BlobInventory;
use uste_txn::{ApplyError, AuthorizedTransactionState, TransactionState};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordRef};

use super::{
    ArtifactInput, ClaimInput, EdgeInput, FetchOutcome, RESEARCH_PROFILE, ResearchCodecError,
    ResearchRecord, RetainedContent, SourceRecordInput, decode_research_record, encode_artifact,
    encode_claim, encode_edge, encode_research_record, encode_source, record,
};
use crate::{SourceLocator, SourceVersionId};

const MAGIC: [u8; 4] = *b"URST";
const FORMAT_MAJOR: u8 = 1;
const FORMAT_MINOR: u8 = 0;
/// Magic, major, minor, mutation tag, reserved byte, 32-byte scope and 8-byte generation.
pub const RESEARCH_TRANSACTION_HEADER_BYTES: usize = 48;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResearchMutation {
    BeginRebuild {
        next_generation: u64,
    },
    /// Admit one new record. A claim whose `corrects` names an active claim supersedes it.
    Put(Box<ResearchRecord>),
    RetractClaim {
        target: RecordRef,
    },
    ExpireClaim {
        target: RecordRef,
    },
    RevokeSource {
        source: SourceVersionId,
    },
    CompleteRebuild,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchTransaction {
    pub scope: NamespaceRef,
    pub generation: u64,
    pub mutation: ResearchMutation,
}

pub fn encode_research_transaction(
    transaction: &ResearchTransaction,
) -> Result<Vec<u8>, ResearchCodecError> {
    let mut output = Vec::new();
    output
        .try_reserve(RESEARCH_TRANSACTION_HEADER_BYTES + 64)
        .map_err(|_| ResearchCodecError::ResourceLimit)?;
    output.extend_from_slice(&MAGIC);
    output.extend_from_slice(&[
        FORMAT_MAJOR,
        FORMAT_MINOR,
        mutation_tag(&transaction.mutation),
        0,
    ]);
    output.extend_from_slice(transaction.scope.database().as_bytes());
    output.extend_from_slice(transaction.scope.namespace().as_bytes());
    output.extend_from_slice(&transaction.generation.to_be_bytes());
    match &transaction.mutation {
        ResearchMutation::BeginRebuild { next_generation } => {
            output.extend_from_slice(&next_generation.to_be_bytes());
        }
        ResearchMutation::Put(record) => {
            let encoded = encode_research_record(transaction.scope, record)?;
            let length =
                u32::try_from(encoded.len()).map_err(|_| ResearchCodecError::ResourceLimit)?;
            output.extend_from_slice(&length.to_be_bytes());
            output.extend_from_slice(&encoded);
        }
        ResearchMutation::RetractClaim { target } | ResearchMutation::ExpireClaim { target } => {
            same_scope(transaction.scope, *target)?;
            output.extend_from_slice(target.record().as_bytes());
        }
        ResearchMutation::RevokeSource { source } => {
            same_scope(transaction.scope, source.source)?;
            if source.version == 0 {
                return Err(ResearchCodecError::Invalid);
            }
            output.extend_from_slice(source.source.record().as_bytes());
            output.extend_from_slice(&source.version.to_be_bytes());
        }
        ResearchMutation::CompleteRebuild => {}
    }
    if output.len() > RESEARCH_PROFILE.maximum_request_bytes {
        return Err(ResearchCodecError::ResourceLimit);
    }
    Ok(output)
}

pub fn decode_research_transaction(
    input: &[u8],
) -> Result<ResearchTransaction, ResearchCodecError> {
    if input.len() > RESEARCH_PROFILE.maximum_request_bytes {
        return Err(ResearchCodecError::ResourceLimit);
    }
    if input.len() < RESEARCH_TRANSACTION_HEADER_BYTES {
        return Err(ResearchCodecError::Invalid);
    }
    if input[..4] != MAGIC || input[4] != FORMAT_MAJOR || input[5] != FORMAT_MINOR {
        return Err(ResearchCodecError::UnsupportedVersion);
    }
    let tag = input[6];
    if input[7] != 0 {
        return Err(ResearchCodecError::Invalid);
    }
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes(array(&input[8..24])?),
        NamespaceId::from_bytes(array(&input[24..40])?),
    );
    let generation = u64::from_be_bytes(array(&input[40..48])?);
    let body = &input[RESEARCH_TRANSACTION_HEADER_BYTES..];
    let mutation = match tag {
        1 => ResearchMutation::BeginRebuild {
            next_generation: u64::from_be_bytes(array(body)?),
        },
        2 => {
            let (length, record_bytes) = body
                .split_first_chunk::<4>()
                .ok_or(ResearchCodecError::Invalid)?;
            if usize::try_from(u32::from_be_bytes(*length)).ok() != Some(record_bytes.len()) {
                return Err(ResearchCodecError::Invalid);
            }
            let (record_scope, record) = decode_research_record(record_bytes)?;
            if record_scope != scope {
                return Err(ResearchCodecError::ScopeMismatch);
            }
            ResearchMutation::Put(Box::new(record))
        }
        3 => ResearchMutation::RetractClaim {
            target: record(scope, array(body)?),
        },
        4 => ResearchMutation::ExpireClaim {
            target: record(scope, array(body)?),
        },
        5 => {
            let bytes: [u8; 20] = array(body)?;
            let version = u32::from_be_bytes(array(&bytes[16..])?);
            if version == 0 {
                return Err(ResearchCodecError::Invalid);
            }
            ResearchMutation::RevokeSource {
                source: SourceVersionId {
                    source: record(scope, array(&bytes[..16])?),
                    version,
                },
            }
        }
        6 if body.is_empty() => ResearchMutation::CompleteRebuild,
        6 => return Err(ResearchCodecError::Invalid),
        _ => return Err(ResearchCodecError::UnsupportedVersion),
    };
    Ok(ResearchTransaction {
        scope,
        generation,
        mutation,
    })
}

fn array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], ResearchCodecError> {
    bytes.try_into().map_err(|_| ResearchCodecError::Invalid)
}

fn same_scope(scope: NamespaceRef, reference: RecordRef) -> Result<(), ResearchCodecError> {
    if reference.database() == scope.database() && reference.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(ResearchCodecError::ScopeMismatch)
    }
}

const fn mutation_tag(mutation: &ResearchMutation) -> u8 {
    match mutation {
        ResearchMutation::BeginRebuild { .. } => 1,
        ResearchMutation::Put(_) => 2,
        ResearchMutation::RetractClaim { .. } => 3,
        ResearchMutation::ExpireClaim { .. } => 4,
        ResearchMutation::RevokeSource { .. } => 5,
        ResearchMutation::CompleteRebuild => 6,
    }
}

const fn codec_apply_error(error: ResearchCodecError) -> ApplyError {
    match error {
        ResearchCodecError::ResourceLimit => ApplyError::ResourceLimit,
        ResearchCodecError::Invalid
        | ResearchCodecError::UnsupportedVersion
        | ResearchCodecError::ScopeMismatch => ApplyError::InvalidRequest,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    Active,
    Superseded(CommitRevision),
    Retracted(CommitRevision),
    Expired(CommitRevision),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchSourceEntry {
    pub input: SourceRecordInput,
    pub recorded_revision: CommitRevision,
    pub superseded_revision: Option<CommitRevision>,
    pub revoked_revision: Option<CommitRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchArtifactEntry {
    pub input: ArtifactInput,
    pub recorded_revision: CommitRevision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchClaimEntry {
    pub input: ClaimInput,
    pub recorded_revision: CommitRevision,
    pub status: ClaimStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchEdgeEntry {
    pub input: EdgeInput,
    pub recorded_revision: CommitRevision,
}

/// Bounded in-memory research reducer for one namespace. It is a rebuildable derived index,
/// never a permission or the sole source of truth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchState {
    scope: NamespaceRef,
    generation: Option<u64>,
    ready: bool,
    retained_from: Option<CommitRevision>,
    current_revision: Option<CommitRevision>,
    sources: BTreeMap<SourceVersionId, ResearchSourceEntry>,
    distinct_sources: usize,
    retained_bytes: u64,
    artifacts: BTreeMap<RecordRef, ResearchArtifactEntry>,
    claims: BTreeMap<RecordRef, ResearchClaimEntry>,
    edges: BTreeMap<RecordRef, ResearchEdgeEntry>,
    fan_out: BTreeMap<RecordRef, usize>,
}

impl ResearchState {
    #[must_use]
    pub const fn new(scope: NamespaceRef) -> Self {
        Self {
            scope,
            generation: None,
            ready: false,
            retained_from: None,
            current_revision: None,
            sources: BTreeMap::new(),
            distinct_sources: 0,
            retained_bytes: 0,
            artifacts: BTreeMap::new(),
            claims: BTreeMap::new(),
            edges: BTreeMap::new(),
            fan_out: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn generation(&self) -> Option<u64> {
        self.generation
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.ready
    }

    #[must_use]
    pub const fn current_revision(&self) -> Option<CommitRevision> {
        self.current_revision
    }

    #[must_use]
    pub const fn retained_from(&self) -> Option<CommitRevision> {
        self.retained_from
    }

    #[must_use]
    pub const fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    #[must_use]
    pub fn sources(&self) -> &BTreeMap<SourceVersionId, ResearchSourceEntry> {
        &self.sources
    }

    #[must_use]
    pub fn artifacts(&self) -> &BTreeMap<RecordRef, ResearchArtifactEntry> {
        &self.artifacts
    }

    #[must_use]
    pub fn claims(&self) -> &BTreeMap<RecordRef, ResearchClaimEntry> {
        &self.claims
    }

    #[must_use]
    pub fn edges(&self) -> &BTreeMap<RecordRef, ResearchEdgeEntry> {
        &self.edges
    }

    /// Canonical logical-state digest, equal for equal states however they were reached.
    #[must_use]
    pub fn state_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"uste-research-state-v1");
        digest.update(self.scope.database().as_bytes());
        digest.update(self.scope.namespace().as_bytes());
        digest.update(self.generation.unwrap_or(0).to_be_bytes());
        digest.update([u8::from(self.ready)]);
        for revision in [self.retained_from, self.current_revision] {
            digest.update(revision.map_or(0, CommitRevision::get).to_be_bytes());
        }
        digest.update(self.retained_bytes.to_be_bytes());
        let mut buffer = Vec::new();
        for entry in self.sources.values() {
            buffer.clear();
            // Entries were validated on admission, so re-encoding cannot fail.
            let _ = encode_source(&entry.input, &mut buffer);
            hash_block(&mut digest, 1, &buffer);
            for revision in [
                Some(entry.recorded_revision),
                entry.superseded_revision,
                entry.revoked_revision,
            ] {
                digest.update(revision.map_or(0, CommitRevision::get).to_be_bytes());
            }
        }
        for entry in self.artifacts.values() {
            buffer.clear();
            let _ = encode_artifact(&entry.input, &mut buffer);
            hash_block(&mut digest, 2, &buffer);
            digest.update(entry.recorded_revision.get().to_be_bytes());
        }
        for entry in self.claims.values() {
            buffer.clear();
            let _ = encode_claim(&entry.input, &mut buffer);
            hash_block(&mut digest, 3, &buffer);
            digest.update(entry.recorded_revision.get().to_be_bytes());
            let (tag, revision) = match entry.status {
                ClaimStatus::Active => (0_u8, 0),
                ClaimStatus::Superseded(revision) => (1, revision.get()),
                ClaimStatus::Retracted(revision) => (2, revision.get()),
                ClaimStatus::Expired(revision) => (3, revision.get()),
            };
            digest.update([tag]);
            digest.update(revision.to_be_bytes());
        }
        for entry in self.edges.values() {
            buffer.clear();
            encode_edge(&entry.input, &mut buffer);
            hash_block(&mut digest, 4, &buffer);
            digest.update(entry.recorded_revision.get().to_be_bytes());
        }
        digest.finalize().into()
    }

    fn latest_version(&self, source: RecordRef) -> Option<u32> {
        self.sources
            .range(
                SourceVersionId { source, version: 0 }..=SourceVersionId {
                    source,
                    version: u32::MAX,
                },
            )
            .next_back()
            .map(|(id, _)| id.version)
    }

    /// Record identities are unique across kinds within the namespace.
    fn occupied(&self, id: RecordRef) -> bool {
        self.latest_version(id).is_some()
            || self.artifacts.contains_key(&id)
            || self.claims.contains_key(&id)
            || self.edges.contains_key(&id)
    }

    fn require_generation(&self, generation: u64) -> Result<(), ApplyError> {
        if self.generation == Some(generation) {
            Ok(())
        } else {
            Err(ApplyError::SourceChanged)
        }
    }

    fn charge_retained(&mut self, content: Option<&RetainedContent>) -> Result<(), ApplyError> {
        let added = content.map_or(0, |content| content.blob.byte_len());
        let next = self
            .retained_bytes
            .checked_add(added)
            .ok_or(ApplyError::ResourceLimit)?;
        if next > RESEARCH_PROFILE.maximum_source_bytes {
            return Err(ApplyError::ResourceLimit);
        }
        self.retained_bytes = next;
        Ok(())
    }

    fn apply(
        &mut self,
        transaction: ResearchTransaction,
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<(), ApplyError> {
        if transaction.scope != self.scope || transaction.generation == 0 {
            return Err(ApplyError::InvalidRequest);
        }
        match transaction.mutation {
            ResearchMutation::BeginRebuild { next_generation } => {
                if inventory.is_some()
                    || transaction.generation != next_generation
                    || self
                        .generation
                        .is_some_and(|current| current.checked_add(1) != Some(next_generation))
                {
                    return Err(ApplyError::Conflict);
                }
                let scope = self.scope;
                *self = Self::new(scope);
                self.generation = Some(next_generation);
                self.retained_from = Some(revision);
            }
            ResearchMutation::Put(record) => {
                self.require_generation(transaction.generation)?;
                match *record {
                    ResearchRecord::Source(source) => {
                        self.put_source(source, inventory, revision)?;
                    }
                    ResearchRecord::Artifact(artifact) => {
                        self.put_artifact(artifact, inventory, revision)?;
                    }
                    ResearchRecord::Claim(claim) => {
                        require_no_inventory(inventory)?;
                        self.put_claim(claim, revision)?;
                    }
                    ResearchRecord::Edge(edge) => {
                        require_no_inventory(inventory)?;
                        self.put_edge(edge, revision)?;
                    }
                }
            }
            ResearchMutation::RetractClaim { target } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                self.end_claim(target, ClaimStatus::Retracted(revision))?;
            }
            ResearchMutation::ExpireClaim { target } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                self.end_claim(target, ClaimStatus::Expired(revision))?;
            }
            ResearchMutation::RevokeSource { source } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                let entry = self.sources.get_mut(&source).ok_or(ApplyError::Conflict)?;
                if entry.revoked_revision.is_some() {
                    return Err(ApplyError::Conflict);
                }
                entry.revoked_revision = Some(revision);
            }
            ResearchMutation::CompleteRebuild => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                if self.ready {
                    return Err(ApplyError::Conflict);
                }
                self.ready = true;
            }
        }
        self.current_revision = Some(revision);
        Ok(())
    }

    fn end_claim(&mut self, target: RecordRef, terminal: ClaimStatus) -> Result<(), ApplyError> {
        let claim = self.claims.get_mut(&target).ok_or(ApplyError::Conflict)?;
        if claim.status != ClaimStatus::Active {
            return Err(ApplyError::Conflict);
        }
        claim.status = terminal;
        Ok(())
    }

    fn put_source(
        &mut self,
        source: SourceRecordInput,
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<(), ApplyError> {
        let profile = RESEARCH_PROFILE;
        let latest = self.latest_version(source.id.source);
        if latest.is_none() && self.occupied(source.id.source) {
            return Err(ApplyError::Conflict);
        }
        let expected = latest
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ApplyError::ResourceLimit)?;
        if source.id.version != expected {
            return Err(ApplyError::Conflict);
        }
        if latest.is_none() && self.distinct_sources >= profile.maximum_sources {
            return Err(ApplyError::ResourceLimit);
        }
        if self.sources.len() >= profile.maximum_source_versions {
            return Err(ApplyError::ResourceLimit);
        }
        require_inventory(self.scope, source.content.as_ref(), inventory)?;
        self.charge_retained(source.content.as_ref())?;
        if let Some(previous) = latest {
            self.sources
                .get_mut(&SourceVersionId {
                    source: source.id.source,
                    version: previous,
                })
                .ok_or(ApplyError::Conflict)?
                .superseded_revision = Some(revision);
        } else {
            self.distinct_sources += 1;
        }
        self.sources.insert(
            source.id,
            ResearchSourceEntry {
                input: source,
                recorded_revision: revision,
                superseded_revision: None,
                revoked_revision: None,
            },
        );
        Ok(())
    }

    fn usable_source(&self, id: SourceVersionId) -> Result<&SourceRecordInput, ApplyError> {
        let entry = self.sources.get(&id).ok_or(ApplyError::Conflict)?;
        if entry.revoked_revision.is_some() {
            return Err(ApplyError::Conflict);
        }
        Ok(&entry.input)
    }

    fn put_artifact(
        &mut self,
        artifact: ArtifactInput,
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<(), ApplyError> {
        if self.occupied(artifact.id) {
            return Err(ApplyError::Conflict);
        }
        if self.artifacts.len() >= RESEARCH_PROFILE.maximum_artifacts {
            return Err(ApplyError::ResourceLimit);
        }
        for input in &artifact.inputs {
            self.usable_source(*input)?;
        }
        require_inventory(self.scope, artifact.content.as_ref(), inventory)?;
        self.charge_retained(artifact.content.as_ref())?;
        self.artifacts.insert(
            artifact.id,
            ResearchArtifactEntry {
                input: artifact,
                recorded_revision: revision,
            },
        );
        Ok(())
    }

    fn put_claim(&mut self, claim: ClaimInput, revision: CommitRevision) -> Result<(), ApplyError> {
        if self.occupied(claim.id) {
            return Err(ApplyError::Conflict);
        }
        if self.claims.len() >= RESEARCH_PROFILE.maximum_claims {
            return Err(ApplyError::ResourceLimit);
        }
        for citation in &claim.citations {
            let source = self.usable_source(citation.source)?;
            if matches!(source.outcome, FetchOutcome::Inaccessible { .. }) {
                return Err(ApplyError::Conflict);
            }
            let length = source
                .content
                .as_ref()
                .map_or(0, |content| content.blob.byte_len());
            let (_, end) = match citation.locator {
                SourceLocator::ByteRange { start, end }
                | SourceLocator::Utf8Lines { start, end, .. } => (start, end),
            };
            if end > length {
                return Err(ApplyError::InvalidRequest);
            }
        }
        if let Some(predecessor) = claim.corrects {
            let prior = self.claims.get(&predecessor).ok_or(ApplyError::Conflict)?;
            if prior.status != ClaimStatus::Active {
                return Err(ApplyError::Conflict);
            }
        }
        if let Some(predecessor) = claim.corrects {
            self.claims
                .get_mut(&predecessor)
                .ok_or(ApplyError::Conflict)?
                .status = ClaimStatus::Superseded(revision);
        }
        self.claims.insert(
            claim.id,
            ResearchClaimEntry {
                input: claim,
                recorded_revision: revision,
                status: ClaimStatus::Active,
            },
        );
        Ok(())
    }

    fn put_edge(&mut self, edge: EdgeInput, revision: CommitRevision) -> Result<(), ApplyError> {
        let profile = RESEARCH_PROFILE;
        if self.occupied(edge.id) || !self.occupied(edge.from) || !self.occupied(edge.to) {
            return Err(ApplyError::Conflict);
        }
        if !self.claims.contains_key(&edge.asserted_by)
            && !self.artifacts.contains_key(&edge.asserted_by)
        {
            return Err(ApplyError::Conflict);
        }
        if self.edges.len() >= profile.maximum_edges {
            return Err(ApplyError::ResourceLimit);
        }
        for endpoint in [edge.from, edge.to] {
            if self.fan_out.get(&endpoint).copied().unwrap_or(0) >= profile.maximum_edges_per_record
            {
                return Err(ApplyError::ResourceLimit);
            }
        }
        for endpoint in [edge.from, edge.to] {
            *self.fan_out.entry(endpoint).or_insert(0) += 1;
        }
        self.edges.insert(
            edge.id,
            ResearchEdgeEntry {
                input: edge,
                recorded_revision: revision,
            },
        );
        Ok(())
    }
}

fn hash_block(digest: &mut Sha256, kind: u8, bytes: &[u8]) {
    digest.update([kind]);
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(bytes);
}

fn require_no_inventory(inventory: Option<&BlobInventory>) -> Result<(), ApplyError> {
    if inventory.is_some() {
        Err(ApplyError::InvalidRequest)
    } else {
        Ok(())
    }
}

/// Retained content must arrive with exactly its own newly finalized blob; absent content must
/// arrive with no inventory.
fn require_inventory(
    scope: NamespaceRef,
    content: Option<&RetainedContent>,
    inventory: Option<&BlobInventory>,
) -> Result<(), ApplyError> {
    match (content, inventory) {
        (None, None) => Ok(()),
        (Some(content), Some(inventory))
            if inventory.scope() == scope && inventory.references() == [content.blob] =>
        {
            Ok(())
        }
        _ => Err(ApplyError::InvalidRequest),
    }
}

impl TransactionState for ResearchState {
    type Prepared = Self;
    type Snapshot = Self;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        let transaction =
            decode_research_transaction(canonical_request).map_err(codec_apply_error)?;
        let mut candidate = self.clone();
        candidate.apply(transaction, blob_inventory, revision)?;
        Ok(candidate)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.state_digest()
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        *self = prepared;
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }
}

impl AuthorizedTransactionState for ResearchState {
    fn authorization_requirements(
        canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        let transaction =
            decode_research_transaction(canonical_request).map_err(codec_apply_error)?;
        let requirement = |action, reference| AuthorizationRequirement {
            action,
            target: Target::Record(reference),
        };
        let mut requirements = Vec::new();
        match transaction.mutation {
            ResearchMutation::BeginRebuild { .. } | ResearchMutation::CompleteRebuild => {
                requirements.push(AuthorizationRequirement {
                    action: Action::ManageSchema,
                    target: Target::Namespace(transaction.scope),
                });
            }
            ResearchMutation::Put(record) => {
                match *record {
                    ResearchRecord::Source(source) => {
                        requirements.push(requirement(Action::Commit, source.id.source));
                    }
                    ResearchRecord::Artifact(artifact) => {
                        requirements.push(requirement(Action::Commit, artifact.id));
                        requirements.extend(
                            artifact
                                .inputs
                                .iter()
                                .map(|input| requirement(Action::ReadRecord, input.source)),
                        );
                    }
                    ResearchRecord::Claim(claim) => {
                        requirements.push(requirement(Action::Commit, claim.id));
                        requirements.extend(claim.citations.iter().map(|citation| {
                            requirement(Action::ReadRecord, citation.source.source)
                        }));
                        if let Some(predecessor) = claim.corrects {
                            requirements.push(requirement(Action::Commit, predecessor));
                        }
                    }
                    ResearchRecord::Edge(edge) => {
                        requirements.push(requirement(Action::Commit, edge.id));
                        for reference in [edge.from, edge.to, edge.asserted_by] {
                            requirements.push(requirement(Action::ReadRecord, reference));
                        }
                    }
                }
            }
            ResearchMutation::RetractClaim { target }
            | ResearchMutation::ExpireClaim { target } => {
                requirements.push(requirement(Action::Commit, target));
            }
            ResearchMutation::RevokeSource { source } => {
                requirements.push(requirement(Action::ManageRetention, source.source));
            }
        }
        AuthorizationRequirements::new(requirements).map_err(|_| ApplyError::ResourceLimit)
    }
}

#[cfg(test)]
mod tests;
