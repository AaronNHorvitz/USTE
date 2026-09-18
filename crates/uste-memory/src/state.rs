//! Durable bounded source/fact reducer for `memory-pilot-v1`.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use sha2::{Digest, Sha256};
use uste_policy::{Action, AuthorizationRequirement, AuthorizationRequirements, Target};
use uste_storage::{BlobId, BlobInventory, BlobReference};
use uste_txn::{ApplyError, AuthorizedTransactionState, TransactionState};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant,
};

use crate::PILOT_PROFILE;

const MAGIC: [u8; 4] = *b"UMEM";
const FORMAT_MAJOR: u8 = 1;
const FORMAT_MINOR: u8 = 0;
const HEADER_BYTES: usize = 48;

pub const TEXT_MEDIA_TYPE: &str = "text/plain; charset=utf-8";
pub const OPAQUE_MEDIA_TYPE: &str = "application/octet-stream";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceVersionId {
    pub source: RecordRef,
    pub version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLocator {
    ByteRange {
        start: u64,
        end: u64,
    },
    Utf8Lines {
        start: u64,
        end: u64,
        first_line: u32,
        last_line: u32,
    },
}

impl SourceLocator {
    #[must_use]
    pub const fn byte_range(self) -> (u64, u64) {
        match self {
            Self::ByteRange { start, end } | Self::Utf8Lines { start, end, .. } => (start, end),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceVersionInput {
    pub id: SourceVersionId,
    pub blob: BlobReference,
    pub media_type: String,
    /// Exact trusted-consumer-supplied UTF-8 bytes. `None` means opaque-only storage.
    pub exact_utf8: Option<String>,
    pub source_event_time: Option<UtcInstant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactInput {
    pub id: RecordRef,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    /// Explicit scoped links used by the pilot's bounded one-hop graph query.
    pub links: Vec<RecordRef>,
    pub source: SourceVersionId,
    pub locator: SourceLocator,
    pub source_event_time: Option<UtcInstant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryMutation {
    BeginRebuild {
        next_generation: u64,
    },
    PutSource(SourceVersionInput),
    PutFact(FactInput),
    CorrectFact {
        target: RecordRef,
        replacement: FactInput,
    },
    RetractFact {
        target: RecordRef,
    },
    RevokeSource {
        source: SourceVersionId,
    },
    CompleteRebuild,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryTransaction {
    pub scope: NamespaceRef,
    pub generation: u64,
    pub mutation: MemoryMutation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceVersionRecord {
    pub input: SourceVersionInput,
    pub recorded_revision: CommitRevision,
    pub superseded_revision: Option<CommitRevision>,
    pub revoked_revision: Option<CommitRevision>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FactTerminal {
    Superseded(CommitRevision),
    Retracted(CommitRevision),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactRecord {
    pub input: FactInput,
    pub recorded_revision: CommitRevision,
    pub corrects: Option<RecordRef>,
    pub terminal: Option<FactTerminal>,
}

#[derive(Clone, Debug)]
pub struct MemoryState {
    scope: NamespaceRef,
    generation: Option<u64>,
    ready: bool,
    retained_from: Option<CommitRevision>,
    current_revision: Option<CommitRevision>,
    sources: BTreeMap<SourceVersionId, SourceVersionRecord>,
    facts: BTreeMap<RecordRef, FactRecord>,
    source_bytes: u64,
    logical_bytes: usize,
    /// Process-local freshness epoch shared by snapshots; zero means no committed revision.
    runtime_revision: Arc<AtomicU64>,
}

impl MemoryState {
    #[must_use]
    pub fn new(scope: NamespaceRef) -> Self {
        Self {
            scope,
            generation: None,
            ready: false,
            retained_from: None,
            current_revision: None,
            sources: BTreeMap::new(),
            facts: BTreeMap::new(),
            source_bytes: 0,
            logical_bytes: 0,
            runtime_revision: Arc::new(AtomicU64::new(0)),
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
    pub fn sources(&self) -> &BTreeMap<SourceVersionId, SourceVersionRecord> {
        &self.sources
    }

    #[must_use]
    pub fn facts(&self) -> &BTreeMap<RecordRef, FactRecord> {
        &self.facts
    }

    #[must_use]
    pub const fn source_bytes(&self) -> u64 {
        self.source_bytes
    }

    #[must_use]
    pub const fn logical_bytes(&self) -> usize {
        self.logical_bytes
    }

    pub(crate) fn is_runtime_current(&self) -> bool {
        self.runtime_revision.load(Ordering::Acquire)
            == self.current_revision.map_or(0, CommitRevision::get)
    }

    fn apply(
        &mut self,
        transaction: MemoryTransaction,
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<(), ApplyError> {
        if transaction.scope != self.scope
            || revision.get() > PILOT_PROFILE.maximum_commits
            || transaction.generation == 0
        {
            return Err(ApplyError::InvalidRequest);
        }
        match transaction.mutation {
            MemoryMutation::BeginRebuild { next_generation } => {
                if inventory.is_some()
                    || transaction.generation != next_generation
                    || next_generation == 0
                    || self
                        .generation
                        .is_some_and(|current| current.checked_add(1) != Some(next_generation))
                {
                    return Err(ApplyError::Conflict);
                }
                self.generation = Some(next_generation);
                self.ready = false;
                self.retained_from = Some(revision);
                self.sources.clear();
                self.facts.clear();
                self.source_bytes = 0;
                self.logical_bytes = 0;
            }
            MemoryMutation::PutSource(source) => {
                self.require_generation(transaction.generation)?;
                validate_source(&source, self.scope, inventory)?;
                if self.sources.contains_key(&source.id)
                    || self.sources.len() >= PILOT_PROFILE.maximum_source_versions
                {
                    return Err(if self.sources.contains_key(&source.id) {
                        ApplyError::Conflict
                    } else {
                        ApplyError::ResourceLimit
                    });
                }
                let expected_version = self
                    .sources
                    .keys()
                    .filter(|id| id.source == source.id.source)
                    .map(|id| id.version)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(ApplyError::ResourceLimit)?;
                if source.id.version != expected_version {
                    return Err(ApplyError::Conflict);
                }
                let next_source_bytes = self
                    .source_bytes
                    .checked_add(source.blob.byte_len())
                    .ok_or(ApplyError::ResourceLimit)?;
                if next_source_bytes > PILOT_PROFILE.maximum_source_bytes {
                    return Err(ApplyError::ResourceLimit);
                }
                let added = source_logical_bytes(&source)?;
                self.charge_logical(added)?;
                self.source_bytes = next_source_bytes;
                if source.id.version > 1 {
                    let previous = SourceVersionId {
                        source: source.id.source,
                        version: source.id.version - 1,
                    };
                    self.sources
                        .get_mut(&previous)
                        .ok_or(ApplyError::Conflict)?
                        .superseded_revision = Some(revision);
                }
                self.sources.insert(
                    source.id,
                    SourceVersionRecord {
                        input: source,
                        recorded_revision: revision,
                        superseded_revision: None,
                        revoked_revision: None,
                    },
                );
            }
            MemoryMutation::PutFact(fact) => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                self.insert_fact(fact, None, revision)?;
            }
            MemoryMutation::CorrectFact {
                target,
                replacement,
            } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                require_scope(target, self.scope)?;
                let prior = self.facts.get(&target).ok_or(ApplyError::Conflict)?;
                if prior.terminal.is_some() {
                    return Err(ApplyError::Conflict);
                }
                self.validate_fact(&replacement)?;
                if replacement.id == target || self.facts.contains_key(&replacement.id) {
                    return Err(ApplyError::Conflict);
                }
                if self.facts.len() >= PILOT_PROFILE.maximum_facts {
                    return Err(ApplyError::ResourceLimit);
                }
                self.charge_logical(fact_logical_bytes(&replacement)?)?;
                self.facts
                    .get_mut(&target)
                    .ok_or(ApplyError::Conflict)?
                    .terminal = Some(FactTerminal::Superseded(revision));
                self.facts.insert(
                    replacement.id,
                    FactRecord {
                        input: replacement,
                        recorded_revision: revision,
                        corrects: Some(target),
                        terminal: None,
                    },
                );
            }
            MemoryMutation::RetractFact { target } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                require_scope(target, self.scope)?;
                let fact = self.facts.get_mut(&target).ok_or(ApplyError::Conflict)?;
                if fact.terminal.is_some() {
                    return Err(ApplyError::Conflict);
                }
                fact.terminal = Some(FactTerminal::Retracted(revision));
            }
            MemoryMutation::RevokeSource { source } => {
                self.require_generation(transaction.generation)?;
                require_no_inventory(inventory)?;
                require_scope(source.source, self.scope)?;
                let source = self.sources.get_mut(&source).ok_or(ApplyError::Conflict)?;
                if source.revoked_revision.is_some() {
                    return Err(ApplyError::Conflict);
                }
                source.revoked_revision = Some(revision);
            }
            MemoryMutation::CompleteRebuild => {
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

    fn require_generation(&self, generation: u64) -> Result<(), ApplyError> {
        if self.generation == Some(generation) {
            Ok(())
        } else {
            Err(ApplyError::SourceChanged)
        }
    }

    fn insert_fact(
        &mut self,
        fact: FactInput,
        corrects: Option<RecordRef>,
        revision: CommitRevision,
    ) -> Result<(), ApplyError> {
        self.validate_fact(&fact)?;
        if self.facts.contains_key(&fact.id) {
            return Err(ApplyError::Conflict);
        }
        if self.facts.len() >= PILOT_PROFILE.maximum_facts {
            return Err(ApplyError::ResourceLimit);
        }
        self.charge_logical(fact_logical_bytes(&fact)?)?;
        self.facts.insert(
            fact.id,
            FactRecord {
                input: fact,
                recorded_revision: revision,
                corrects,
                terminal: None,
            },
        );
        Ok(())
    }

    fn validate_fact(&self, fact: &FactInput) -> Result<(), ApplyError> {
        require_scope(fact.id, self.scope)?;
        require_scope(fact.source.source, self.scope)?;
        for field in [&fact.subject, &fact.predicate, &fact.value] {
            if field.is_empty() || field.len() > PILOT_PROFILE.maximum_fact_field_bytes {
                return Err(ApplyError::ResourceLimit);
            }
        }
        if fact.links.len() > PILOT_PROFILE.maximum_fact_links {
            return Err(ApplyError::ResourceLimit);
        }
        let mut previous = None;
        for link in &fact.links {
            require_scope(*link, self.scope)?;
            if previous.is_some_and(|value| value >= *link) || !self.facts.contains_key(link) {
                return Err(ApplyError::Conflict);
            }
            previous = Some(*link);
        }
        let source = self.sources.get(&fact.source).ok_or(ApplyError::Conflict)?;
        if source.revoked_revision.is_some() {
            return Err(ApplyError::Conflict);
        }
        validate_locator(fact.locator, &source.input)
    }

    fn charge_logical(&mut self, added: usize) -> Result<(), ApplyError> {
        let projected = self
            .logical_bytes
            .checked_add(added)
            .ok_or(ApplyError::ResourceLimit)?;
        if projected > PILOT_PROFILE.maximum_logical_state_bytes {
            return Err(ApplyError::ResourceLimit);
        }
        self.logical_bytes = projected;
        Ok(())
    }

    fn digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"uste-memory-state-v1");
        digest.update(self.scope.database().as_bytes());
        digest.update(self.scope.namespace().as_bytes());
        digest.update(self.generation.unwrap_or(0).to_be_bytes());
        digest.update([u8::from(self.ready)]);
        digest.update(
            self.retained_from
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        digest.update(
            self.current_revision
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        digest.update(self.source_bytes.to_be_bytes());
        digest.update(
            u64::try_from(self.logical_bytes)
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for (id, source) in &self.sources {
            hash_source_id(&mut digest, *id);
            hash_source(&mut digest, &source.input);
            digest.update(source.recorded_revision.get().to_be_bytes());
            digest.update(
                source
                    .superseded_revision
                    .map_or(0, CommitRevision::get)
                    .to_be_bytes(),
            );
            digest.update(
                source
                    .revoked_revision
                    .map_or(0, CommitRevision::get)
                    .to_be_bytes(),
            );
        }
        for (id, fact) in &self.facts {
            digest.update(id.record().as_bytes());
            hash_fact(&mut digest, &fact.input);
            digest.update(fact.recorded_revision.get().to_be_bytes());
            match fact.corrects {
                Some(record) => {
                    digest.update([1]);
                    digest.update(record.record().as_bytes());
                }
                None => digest.update([0]),
            }
            match fact.terminal {
                None => digest.update([0]),
                Some(FactTerminal::Superseded(revision)) => {
                    digest.update([1]);
                    digest.update(revision.get().to_be_bytes());
                }
                Some(FactTerminal::Retracted(revision)) => {
                    digest.update([2]);
                    digest.update(revision.get().to_be_bytes());
                }
            }
        }
        digest.finalize().into()
    }
}

impl TransactionState for MemoryState {
    type Prepared = Self;
    type Snapshot = Self;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if canonical_request.len() > PILOT_PROFILE.maximum_request_bytes {
            return Err(ApplyError::ResourceLimit);
        }
        let transaction = decode_transaction(canonical_request).map_err(MemoryCodecError::apply)?;
        let mut candidate = self.clone();
        candidate.apply(transaction, blob_inventory, revision)?;
        Ok(candidate)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.digest()
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        *self = prepared;
        self.runtime_revision.store(
            self.current_revision.map_or(0, CommitRevision::get),
            Ordering::Release,
        );
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }
}

impl AuthorizedTransactionState for MemoryState {
    fn authorization_requirements(
        canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        if canonical_request.len() > PILOT_PROFILE.maximum_request_bytes {
            return Err(ApplyError::ResourceLimit);
        }
        let transaction = decode_transaction(canonical_request).map_err(MemoryCodecError::apply)?;
        let namespace = Target::Namespace(transaction.scope);
        let mut requirements = Vec::new();
        match transaction.mutation {
            MemoryMutation::BeginRebuild { .. } | MemoryMutation::CompleteRebuild => {
                requirements.push(AuthorizationRequirement {
                    action: Action::ManageSchema,
                    target: namespace,
                });
            }
            MemoryMutation::PutSource(source) => requirements.push(AuthorizationRequirement {
                action: Action::Commit,
                target: Target::Record(source.id.source),
            }),
            MemoryMutation::PutFact(fact) => {
                requirements.extend(fact_requirements(&fact));
            }
            MemoryMutation::CorrectFact {
                target,
                replacement,
            } => {
                requirements.push(AuthorizationRequirement {
                    action: Action::Commit,
                    target: Target::Record(target),
                });
                requirements.extend(fact_requirements(&replacement));
            }
            MemoryMutation::RetractFact { target } => {
                requirements.push(AuthorizationRequirement {
                    action: Action::Commit,
                    target: Target::Record(target),
                });
            }
            MemoryMutation::RevokeSource { source } => {
                requirements.push(AuthorizationRequirement {
                    action: Action::ManageRetention,
                    target: Target::Record(source.source),
                });
            }
        }
        AuthorizationRequirements::new(requirements).map_err(|_| ApplyError::ResourceLimit)
    }
}

fn fact_requirements(fact: &FactInput) -> Vec<AuthorizationRequirement> {
    let mut requirements = Vec::with_capacity(2 + fact.links.len());
    requirements.push(AuthorizationRequirement {
        action: Action::Commit,
        target: Target::Record(fact.id),
    });
    requirements.push(AuthorizationRequirement {
        action: Action::ReadRecord,
        target: Target::Record(fact.source.source),
    });
    requirements.extend(fact.links.iter().map(|link| AuthorizationRequirement {
        action: Action::ReadRecord,
        target: Target::Record(*link),
    }));
    requirements
}

fn validate_source(
    source: &SourceVersionInput,
    scope: NamespaceRef,
    inventory: Option<&BlobInventory>,
) -> Result<(), ApplyError> {
    require_scope(source.id.source, scope)?;
    let supported_representation = match &source.exact_utf8 {
        Some(_) => source.media_type == TEXT_MEDIA_TYPE,
        None => source.media_type == OPAQUE_MEDIA_TYPE,
    };
    if source.id.version == 0
        || source.blob.scope() != scope
        || source.blob.byte_len() > PILOT_PROFILE.maximum_source_bytes_per_version
        || !supported_representation
    {
        return Err(ApplyError::InvalidRequest);
    }
    let inventory = inventory.ok_or(ApplyError::InvalidRequest)?;
    if inventory.scope() != scope || inventory.references() != [source.blob] {
        return Err(ApplyError::InvalidRequest);
    }
    if let Some(text) = &source.exact_utf8
        && (text.len() > PILOT_PROFILE.maximum_text_bytes_per_version
            || u64::try_from(text.len()).ok() != Some(source.blob.byte_len())
            || <[u8; 32]>::from(Sha256::digest(text.as_bytes())) != source.blob.content_digest())
    {
        return Err(ApplyError::SourceChanged);
    }
    Ok(())
}

fn validate_locator(locator: SourceLocator, source: &SourceVersionInput) -> Result<(), ApplyError> {
    let (start, end) = locator.byte_range();
    if start > end || end > source.blob.byte_len() {
        return Err(ApplyError::InvalidRequest);
    }
    match locator {
        SourceLocator::ByteRange { .. } => Ok(()),
        SourceLocator::Utf8Lines {
            first_line,
            last_line,
            ..
        } => {
            let text = source
                .exact_utf8
                .as_ref()
                .ok_or(ApplyError::InvalidRequest)?;
            let start = usize::try_from(start).map_err(|_| ApplyError::ResourceLimit)?;
            let end = usize::try_from(end).map_err(|_| ApplyError::ResourceLimit)?;
            if first_line == 0
                || last_line < first_line
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return Err(ApplyError::InvalidRequest);
            }
            let actual_first = line_at(text.as_bytes(), start)?;
            let actual_last = if start == end {
                actual_first
            } else {
                line_at(text.as_bytes(), end - 1)?
            };
            if actual_first != first_line || actual_last != last_line {
                return Err(ApplyError::InvalidRequest);
            }
            Ok(())
        }
    }
}

fn line_at(bytes: &[u8], offset: usize) -> Result<u32, ApplyError> {
    let newlines = bytes
        .get(..offset)
        .ok_or(ApplyError::InvalidRequest)?
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    u32::try_from(newlines)
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or(ApplyError::ResourceLimit)
}

fn require_no_inventory(inventory: Option<&BlobInventory>) -> Result<(), ApplyError> {
    if inventory.is_none() {
        Ok(())
    } else {
        Err(ApplyError::InvalidRequest)
    }
}

fn require_scope(record: RecordRef, scope: NamespaceRef) -> Result<(), ApplyError> {
    if record.database() == scope.database() && record.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(ApplyError::InvalidRequest)
    }
}

fn source_logical_bytes(source: &SourceVersionInput) -> Result<usize, ApplyError> {
    160_usize
        .checked_add(source.media_type.len())
        .and_then(|value| value.checked_add(source.exact_utf8.as_ref().map_or(0, String::len)))
        .ok_or(ApplyError::ResourceLimit)
}

fn fact_logical_bytes(fact: &FactInput) -> Result<usize, ApplyError> {
    192_usize
        .checked_add(fact.subject.len())
        .and_then(|value| value.checked_add(fact.predicate.len()))
        .and_then(|value| value.checked_add(fact.value.len()))
        .and_then(|value| value.checked_add(fact.links.len().checked_mul(16)?))
        .ok_or(ApplyError::ResourceLimit)
}

fn hash_source_id(digest: &mut Sha256, id: SourceVersionId) {
    digest.update(id.source.record().as_bytes());
    digest.update(id.version.to_be_bytes());
}

fn hash_source(digest: &mut Sha256, source: &SourceVersionInput) {
    hash_source_id(digest, source.id);
    digest.update(source.blob.id().as_bytes());
    digest.update(source.blob.byte_len().to_be_bytes());
    digest.update(source.blob.chunk_count().to_be_bytes());
    digest.update(source.blob.content_digest());
    hash_bytes(digest, source.media_type.as_bytes());
    match &source.exact_utf8 {
        Some(text) => {
            digest.update([1]);
            hash_bytes(digest, text.as_bytes());
        }
        None => digest.update([0]),
    }
    hash_instant(digest, source.source_event_time);
}

fn hash_fact(digest: &mut Sha256, fact: &FactInput) {
    digest.update(fact.id.record().as_bytes());
    hash_bytes(digest, fact.subject.as_bytes());
    hash_bytes(digest, fact.predicate.as_bytes());
    hash_bytes(digest, fact.value.as_bytes());
    digest.update(
        u64::try_from(fact.links.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for link in &fact.links {
        digest.update(link.record().as_bytes());
    }
    hash_source_id(digest, fact.source);
    hash_locator(digest, fact.locator);
    hash_instant(digest, fact.source_event_time);
}

fn hash_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(bytes);
}

fn hash_instant(digest: &mut Sha256, instant: Option<UtcInstant>) {
    match instant {
        Some(value) => {
            digest.update([1]);
            digest.update(value.seconds().to_be_bytes());
            digest.update(value.nanoseconds().to_be_bytes());
        }
        None => digest.update([0]),
    }
}

fn hash_locator(digest: &mut Sha256, locator: SourceLocator) {
    match locator {
        SourceLocator::ByteRange { start, end } => {
            digest.update([1]);
            digest.update(start.to_be_bytes());
            digest.update(end.to_be_bytes());
        }
        SourceLocator::Utf8Lines {
            start,
            end,
            first_line,
            last_line,
        } => {
            digest.update([2]);
            digest.update(start.to_be_bytes());
            digest.update(end.to_be_bytes());
            digest.update(first_line.to_be_bytes());
            digest.update(last_line.to_be_bytes());
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCodecError {
    Invalid,
    ResourceLimit,
    UnsupportedVersion,
}

impl MemoryCodecError {
    const fn apply(self) -> ApplyError {
        match self {
            Self::ResourceLimit => ApplyError::ResourceLimit,
            Self::Invalid | Self::UnsupportedVersion => ApplyError::InvalidRequest,
        }
    }
}

pub fn encode_transaction(transaction: &MemoryTransaction) -> Result<Vec<u8>, MemoryCodecError> {
    let mut output = Vec::new();
    output
        .try_reserve(PILOT_PROFILE.maximum_request_bytes.min(4096))
        .map_err(|_| MemoryCodecError::ResourceLimit)?;
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
    encode_mutation(&transaction.mutation, &mut output)?;
    if output.len() > PILOT_PROFILE.maximum_request_bytes {
        Err(MemoryCodecError::ResourceLimit)
    } else {
        Ok(output)
    }
}

pub fn decode_transaction(input: &[u8]) -> Result<MemoryTransaction, MemoryCodecError> {
    if input.len() > PILOT_PROFILE.maximum_request_bytes {
        return Err(MemoryCodecError::ResourceLimit);
    }
    let mut cursor = Cursor::new(input);
    if cursor.take(4)? != MAGIC {
        return Err(MemoryCodecError::Invalid);
    }
    if cursor.byte()? != FORMAT_MAJOR || cursor.byte()? != FORMAT_MINOR {
        return Err(MemoryCodecError::UnsupportedVersion);
    }
    let tag = cursor.byte()?;
    if cursor.byte()? != 0 {
        return Err(MemoryCodecError::Invalid);
    }
    let database = DatabaseId::from_bytes(cursor.array()?);
    let namespace = NamespaceId::from_bytes(cursor.array()?);
    let generation = cursor.u64()?;
    let scope = NamespaceRef::new(database, namespace);
    let mutation = decode_mutation(tag, scope, &mut cursor)?;
    if cursor.remaining() != 0 || input.len() < HEADER_BYTES {
        return Err(MemoryCodecError::Invalid);
    }
    Ok(MemoryTransaction {
        scope,
        generation,
        mutation,
    })
}

fn mutation_tag(mutation: &MemoryMutation) -> u8 {
    match mutation {
        MemoryMutation::BeginRebuild { .. } => 1,
        MemoryMutation::PutSource(_) => 2,
        MemoryMutation::PutFact(_) => 3,
        MemoryMutation::CorrectFact { .. } => 4,
        MemoryMutation::RetractFact { .. } => 5,
        MemoryMutation::RevokeSource { .. } => 6,
        MemoryMutation::CompleteRebuild => 7,
    }
}

fn encode_mutation(
    mutation: &MemoryMutation,
    output: &mut Vec<u8>,
) -> Result<(), MemoryCodecError> {
    match mutation {
        MemoryMutation::BeginRebuild { next_generation } => {
            output.extend_from_slice(&next_generation.to_be_bytes());
        }
        MemoryMutation::PutSource(source) => encode_source(source, output)?,
        MemoryMutation::PutFact(fact) => encode_fact(fact, output)?,
        MemoryMutation::CorrectFact {
            target,
            replacement,
        } => {
            output.extend_from_slice(target.record().as_bytes());
            encode_fact(replacement, output)?;
        }
        MemoryMutation::RetractFact { target } => {
            output.extend_from_slice(target.record().as_bytes());
        }
        MemoryMutation::RevokeSource { source } => encode_source_id(*source, output),
        MemoryMutation::CompleteRebuild => {}
    }
    Ok(())
}

fn decode_mutation(
    tag: u8,
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<MemoryMutation, MemoryCodecError> {
    Ok(match tag {
        1 => MemoryMutation::BeginRebuild {
            next_generation: cursor.u64()?,
        },
        2 => MemoryMutation::PutSource(decode_source(scope, cursor)?),
        3 => MemoryMutation::PutFact(decode_fact(scope, cursor)?),
        4 => MemoryMutation::CorrectFact {
            target: record(scope, cursor.array()?),
            replacement: decode_fact(scope, cursor)?,
        },
        5 => MemoryMutation::RetractFact {
            target: record(scope, cursor.array()?),
        },
        6 => MemoryMutation::RevokeSource {
            source: decode_source_id(scope, cursor)?,
        },
        7 => MemoryMutation::CompleteRebuild,
        _ => return Err(MemoryCodecError::Invalid),
    })
}

fn encode_source(
    source: &SourceVersionInput,
    output: &mut Vec<u8>,
) -> Result<(), MemoryCodecError> {
    encode_source_id(source.id, output);
    output.extend_from_slice(&source.blob.id().as_bytes());
    output.extend_from_slice(&source.blob.byte_len().to_be_bytes());
    output.extend_from_slice(&source.blob.chunk_count().to_be_bytes());
    output.extend_from_slice(&source.blob.content_digest());
    encode_string(&source.media_type, output)?;
    encode_optional_string(source.exact_utf8.as_deref(), output)?;
    encode_instant(source.source_event_time, output);
    Ok(())
}

fn decode_source(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<SourceVersionInput, MemoryCodecError> {
    let id = decode_source_id(scope, cursor)?;
    let blob_id = BlobId::from_bytes(cursor.array()?);
    let byte_len = cursor.u64()?;
    let chunk_count = cursor.u32()?;
    let content_digest = cursor.array()?;
    let blob = BlobReference::new(scope, blob_id, byte_len, chunk_count, content_digest)
        .map_err(|_| MemoryCodecError::Invalid)?;
    Ok(SourceVersionInput {
        id,
        blob,
        media_type: cursor.string(256)?,
        exact_utf8: cursor.optional_string(PILOT_PROFILE.maximum_text_bytes_per_version)?,
        source_event_time: cursor.instant()?,
    })
}

fn encode_fact(fact: &FactInput, output: &mut Vec<u8>) -> Result<(), MemoryCodecError> {
    output.extend_from_slice(fact.id.record().as_bytes());
    encode_string(&fact.subject, output)?;
    encode_string(&fact.predicate, output)?;
    encode_string(&fact.value, output)?;
    output.extend_from_slice(
        &u32::try_from(fact.links.len())
            .map_err(|_| MemoryCodecError::ResourceLimit)?
            .to_be_bytes(),
    );
    for link in &fact.links {
        output.extend_from_slice(link.record().as_bytes());
    }
    encode_source_id(fact.source, output);
    encode_locator(fact.locator, output);
    encode_instant(fact.source_event_time, output);
    Ok(())
}

fn decode_fact(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<FactInput, MemoryCodecError> {
    let id = record(scope, cursor.array()?);
    let subject = cursor.string(PILOT_PROFILE.maximum_fact_field_bytes)?;
    let predicate = cursor.string(PILOT_PROFILE.maximum_fact_field_bytes)?;
    let value = cursor.string(PILOT_PROFILE.maximum_fact_field_bytes)?;
    let link_count = usize::try_from(cursor.u32()?).map_err(|_| MemoryCodecError::ResourceLimit)?;
    if link_count > PILOT_PROFILE.maximum_fact_links {
        return Err(MemoryCodecError::ResourceLimit);
    }
    let mut links = Vec::new();
    links
        .try_reserve_exact(link_count)
        .map_err(|_| MemoryCodecError::ResourceLimit)?;
    for _ in 0..link_count {
        links.push(record(scope, cursor.array()?));
    }
    Ok(FactInput {
        id,
        subject,
        predicate,
        value,
        links,
        source: decode_source_id(scope, cursor)?,
        locator: cursor.locator()?,
        source_event_time: cursor.instant()?,
    })
}

fn encode_source_id(id: SourceVersionId, output: &mut Vec<u8>) {
    output.extend_from_slice(id.source.record().as_bytes());
    output.extend_from_slice(&id.version.to_be_bytes());
}

fn decode_source_id(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<SourceVersionId, MemoryCodecError> {
    Ok(SourceVersionId {
        source: record(scope, cursor.array()?),
        version: cursor.u32()?,
    })
}

fn encode_locator(locator: SourceLocator, output: &mut Vec<u8>) {
    match locator {
        SourceLocator::ByteRange { start, end } => {
            output.push(1);
            output.extend_from_slice(&start.to_be_bytes());
            output.extend_from_slice(&end.to_be_bytes());
        }
        SourceLocator::Utf8Lines {
            start,
            end,
            first_line,
            last_line,
        } => {
            output.push(2);
            output.extend_from_slice(&start.to_be_bytes());
            output.extend_from_slice(&end.to_be_bytes());
            output.extend_from_slice(&first_line.to_be_bytes());
            output.extend_from_slice(&last_line.to_be_bytes());
        }
    }
}

fn encode_instant(instant: Option<UtcInstant>, output: &mut Vec<u8>) {
    match instant {
        Some(instant) => {
            output.push(1);
            output.extend_from_slice(&instant.seconds().to_be_bytes());
            output.extend_from_slice(&instant.nanoseconds().to_be_bytes());
        }
        None => output.push(0),
    }
}

fn encode_string(value: &str, output: &mut Vec<u8>) -> Result<(), MemoryCodecError> {
    let length = u32::try_from(value.len()).map_err(|_| MemoryCodecError::ResourceLimit)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn encode_optional_string(
    value: Option<&str>,
    output: &mut Vec<u8>,
) -> Result<(), MemoryCodecError> {
    match value {
        Some(value) => {
            output.push(1);
            encode_string(value, output)
        }
        None => {
            output.push(0);
            Ok(())
        }
    }
}

const fn record(scope: NamespaceRef, id: [u8; 16]) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(id),
    )
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    const fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.offset)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], MemoryCodecError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(MemoryCodecError::ResourceLimit)?;
        let bytes = self
            .input
            .get(self.offset..end)
            .ok_or(MemoryCodecError::Invalid)?;
        self.offset = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], MemoryCodecError> {
        self.take(N)?
            .try_into()
            .map_err(|_| MemoryCodecError::Invalid)
    }

    fn byte(&mut self) -> Result<u8, MemoryCodecError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, MemoryCodecError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, MemoryCodecError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn string(&mut self, maximum: usize) -> Result<String, MemoryCodecError> {
        let length = usize::try_from(self.u32()?).map_err(|_| MemoryCodecError::ResourceLimit)?;
        if length > maximum {
            return Err(MemoryCodecError::ResourceLimit);
        }
        let bytes = self.take(length)?;
        let text = core::str::from_utf8(bytes).map_err(|_| MemoryCodecError::Invalid)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(length)
            .map_err(|_| MemoryCodecError::ResourceLimit)?;
        owned.push_str(text);
        Ok(owned)
    }

    fn optional_string(&mut self, maximum: usize) -> Result<Option<String>, MemoryCodecError> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.string(maximum).map(Some),
            _ => Err(MemoryCodecError::Invalid),
        }
    }

    fn instant(&mut self) -> Result<Option<UtcInstant>, MemoryCodecError> {
        match self.byte()? {
            0 => Ok(None),
            1 => UtcInstant::new(i64::from_be_bytes(self.array()?), self.u32()?)
                .map(Some)
                .map_err(|_| MemoryCodecError::Invalid),
            _ => Err(MemoryCodecError::Invalid),
        }
    }

    fn locator(&mut self) -> Result<SourceLocator, MemoryCodecError> {
        let tag = self.byte()?;
        let start = self.u64()?;
        let end = self.u64()?;
        match tag {
            1 => Ok(SourceLocator::ByteRange { start, end }),
            2 => Ok(SourceLocator::Utf8Lines {
                start,
                end,
                first_line: self.u32()?,
                last_line: self.u32()?,
            }),
            _ => Err(MemoryCodecError::Invalid),
        }
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use uste_storage::{BlobId, BlobInventory, BlobReference};
    use uste_txn::{ApplyError, TransactionState};
    use uste_types::{
        CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant,
    };

    use super::{
        FactInput, FactTerminal, MemoryMutation, MemoryState, MemoryTransaction, SourceLocator,
        SourceVersionId, SourceVersionInput, decode_transaction, encode_transaction,
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

    fn source(text: &str) -> SourceVersionInput {
        SourceVersionInput {
            id: SourceVersionId {
                source: record(10),
                version: 1,
            },
            blob: BlobReference::new(
                scope(),
                BlobId::from_bytes([11; 16]),
                u64::try_from(text.len()).unwrap(),
                1,
                Sha256::digest(text.as_bytes()).into(),
            )
            .unwrap(),
            media_type: "text/plain; charset=utf-8".to_owned(),
            exact_utf8: Some(text.to_owned()),
            source_event_time: Some(UtcInstant::new(1_700_000_000, 0).unwrap()),
        }
    }

    fn fact(id: u8, source: SourceVersionId, value: &str) -> FactInput {
        FactInput {
            id: record(id),
            subject: "project".to_owned(),
            predicate: "status".to_owned(),
            value: value.to_owned(),
            links: Vec::new(),
            source,
            locator: SourceLocator::Utf8Lines {
                start: 0,
                end: 5,
                first_line: 1,
                last_line: 1,
            },
            source_event_time: Some(UtcInstant::new(1_700_000_001, 0).unwrap()),
        }
    }

    fn publish(
        state: &mut MemoryState,
        revision: u64,
        mutation: MemoryMutation,
        inventory: Option<&BlobInventory>,
    ) -> Result<(), ApplyError> {
        let encoded = encode_transaction(&MemoryTransaction {
            scope: scope(),
            generation: 1,
            mutation,
        })
        .unwrap();
        let prepared =
            state.prepare(&encoded, inventory, CommitRevision::new(revision).unwrap())?;
        state.publish(prepared);
        Ok(())
    }

    #[test]
    fn source_fact_correction_and_rebuild_state_are_bounded_and_exact() {
        let mut state = MemoryState::new(scope());
        publish(
            &mut state,
            1,
            MemoryMutation::BeginRebuild { next_generation: 1 },
            None,
        )
        .unwrap();
        assert!(!state.is_ready());
        let source = source("alpha\nbeta\n");
        let inventory = BlobInventory::new(scope(), [source.blob]).unwrap();
        publish(
            &mut state,
            2,
            MemoryMutation::PutSource(source.clone()),
            Some(&inventory),
        )
        .unwrap();
        publish(
            &mut state,
            3,
            MemoryMutation::PutFact(fact(20, source.id, "alpha")),
            None,
        )
        .unwrap();
        publish(
            &mut state,
            4,
            MemoryMutation::CorrectFact {
                target: record(20),
                replacement: fact(21, source.id, "beta"),
            },
            None,
        )
        .unwrap();
        publish(&mut state, 5, MemoryMutation::CompleteRebuild, None).unwrap();
        assert!(state.is_ready());
        assert_eq!(state.sources().len(), 1);
        assert_eq!(state.facts().len(), 2);
        assert_eq!(
            state.facts()[&record(20)].terminal,
            Some(FactTerminal::Superseded(CommitRevision::new(4).unwrap()))
        );
        assert_eq!(state.facts()[&record(21)].corrects, Some(record(20)));
        assert_eq!(state.source_bytes(), 11);
        assert!(state.logical_bytes() > 0);

        let wrong_generation = encode_transaction(&MemoryTransaction {
            scope: scope(),
            generation: 2,
            mutation: MemoryMutation::PutFact(fact(22, source.id, "stale")),
        })
        .unwrap();
        assert_eq!(
            state
                .prepare(&wrong_generation, None, CommitRevision::new(6).unwrap())
                .unwrap_err(),
            ApplyError::SourceChanged
        );
    }

    #[test]
    fn changed_text_and_malformed_line_locator_fail_before_publication() {
        let mut state = MemoryState::new(scope());
        publish(
            &mut state,
            1,
            MemoryMutation::BeginRebuild { next_generation: 1 },
            None,
        )
        .unwrap();
        let mut changed = source("alpha\nbeta\n");
        changed.exact_utf8 = Some("changed".to_owned());
        let inventory = BlobInventory::new(scope(), [changed.blob]).unwrap();
        assert_eq!(
            publish(
                &mut state,
                2,
                MemoryMutation::PutSource(changed),
                Some(&inventory)
            ),
            Err(ApplyError::SourceChanged)
        );
        assert!(state.sources().is_empty());

        let mut unknown = source("alpha\nbeta\n");
        unknown.media_type = "text/markdown".to_owned();
        let inventory = BlobInventory::new(scope(), [unknown.blob]).unwrap();
        assert_eq!(
            publish(
                &mut state,
                2,
                MemoryMutation::PutSource(unknown),
                Some(&inventory)
            ),
            Err(ApplyError::InvalidRequest)
        );
        assert!(state.sources().is_empty());

        let source = source("alpha\nbeta\n");
        let inventory = BlobInventory::new(scope(), [source.blob]).unwrap();
        publish(
            &mut state,
            2,
            MemoryMutation::PutSource(source.clone()),
            Some(&inventory),
        )
        .unwrap();
        let mut malformed = fact(20, source.id, "alpha");
        malformed.locator = SourceLocator::Utf8Lines {
            start: 0,
            end: 7,
            first_line: 2,
            last_line: 2,
        };
        assert_eq!(
            publish(&mut state, 3, MemoryMutation::PutFact(malformed), None),
            Err(ApplyError::InvalidRequest)
        );
        assert!(state.facts().is_empty());
    }

    #[test]
    fn transaction_codec_round_trips_every_variant_and_rejects_all_truncations() {
        let source = source("alpha\nbeta\n");
        let transactions = [
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::BeginRebuild { next_generation: 1 },
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::PutSource(source.clone()),
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::PutFact(fact(20, source.id, "alpha")),
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::CorrectFact {
                    target: record(20),
                    replacement: fact(21, source.id, "beta"),
                },
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::RetractFact { target: record(20) },
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::RevokeSource { source: source.id },
            },
            MemoryTransaction {
                scope: scope(),
                generation: 1,
                mutation: MemoryMutation::CompleteRebuild,
            },
        ];
        for transaction in transactions {
            let encoded = encode_transaction(&transaction).unwrap();
            assert_eq!(decode_transaction(&encoded), Ok(transaction));
            for end in 0..encoded.len() {
                assert!(decode_transaction(&encoded[..end]).is_err());
            }
            let mut trailing = encoded;
            trailing.push(0);
            assert!(decode_transaction(&trailing).is_err());
        }
    }
}
