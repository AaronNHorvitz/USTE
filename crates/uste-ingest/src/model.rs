use core::{fmt, num::NonZeroU64};

use uste_storage::BlobReference;
use uste_types::{CommitRevision, NamespaceRef, RecordRef, SourceEventRef};

pub const MAX_ROWS_PER_BATCH: usize = 10_000;
pub const MAX_IMPORT_ROWS: u64 = 1_000_000_000;
pub const MAX_IMPORT_JOBS: usize = 10_000;
pub const MAX_IMPORT_BATCHES: usize = 1_000_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingProfile {
    TypedRecordsV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBinding {
    evidence: RecordRef,
    version: NonZeroU64,
    blob: BlobReference,
}

impl SourceBinding {
    pub fn new(
        evidence: RecordRef,
        version: u64,
        blob: BlobReference,
    ) -> Result<Self, ImportError> {
        if blob.scope().database() != evidence.database()
            || blob.scope().namespace() != evidence.namespace()
        {
            return Err(ImportError::ScopeMismatch);
        }
        Ok(Self {
            evidence,
            version: NonZeroU64::new(version).ok_or(ImportError::InvalidVersion)?,
            blob,
        })
    }

    #[must_use]
    pub const fn evidence(&self) -> RecordRef {
        self.evidence
    }
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version.get()
    }
    #[must_use]
    pub const fn blob(&self) -> BlobReference {
        self.blob
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappingBinding {
    evidence: RecordRef,
    version: NonZeroU64,
    blob: BlobReference,
    profile: MappingProfile,
}

impl MappingBinding {
    pub fn new(
        evidence: RecordRef,
        version: u64,
        blob: BlobReference,
        profile: MappingProfile,
    ) -> Result<Self, ImportError> {
        if blob.scope().database() != evidence.database()
            || blob.scope().namespace() != evidence.namespace()
        {
            return Err(ImportError::ScopeMismatch);
        }
        Ok(Self {
            evidence,
            version: NonZeroU64::new(version).ok_or(ImportError::InvalidVersion)?,
            blob,
            profile,
        })
    }

    #[must_use]
    pub const fn evidence(&self) -> RecordRef {
        self.evidence
    }
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version.get()
    }
    #[must_use]
    pub const fn blob(&self) -> BlobReference {
        self.blob
    }
    #[must_use]
    pub const fn profile(&self) -> MappingProfile {
        self.profile
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportCursor {
    next_row: u64,
}

impl ImportCursor {
    pub const START: Self = Self { next_row: 0 };

    pub const fn new(next_row: u64) -> Result<Self, ImportError> {
        if next_row <= MAX_IMPORT_ROWS {
            Ok(Self { next_row })
        } else {
            Err(ImportError::ResourceLimit)
        }
    }

    #[must_use]
    pub const fn next_row(self) -> u64 {
        self.next_row
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ImportBatchId {
    job: RecordRef,
    sequence: NonZeroU64,
}

impl ImportBatchId {
    pub fn new(job: RecordRef, sequence: u64) -> Result<Self, ImportError> {
        Ok(Self {
            job,
            sequence: NonZeroU64::new(sequence).ok_or(ImportError::InvalidVersion)?,
        })
    }

    #[must_use]
    pub const fn job(self) -> RecordRef {
        self.job
    }
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappedRowReceipt {
    source_event: SourceEventRef,
    declared_payload_digest: [u8; 32],
}

impl MappedRowReceipt {
    /// Construct a caller-declared payload commitment.
    ///
    /// T-49 retains this value for retry/audit identity but does not recompute it from the aggregate
    /// graph/spatial requests. T-54 must verify a canonical per-row effect envelope before using it
    /// as mapping-provenance evidence.
    #[must_use]
    pub const fn new(source_event: SourceEventRef, declared_payload_digest: [u8; 32]) -> Self {
        Self {
            source_event,
            declared_payload_digest,
        }
    }

    #[must_use]
    pub const fn source_event(self) -> SourceEventRef {
        self.source_event
    }
    #[must_use]
    pub const fn declared_payload_digest(self) -> [u8; 32] {
        self.declared_payload_digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportJobStatus {
    Open,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportCheckpoint {
    job: RecordRef,
    version: NonZeroU64,
    source: SourceBinding,
    mapping: MappingBinding,
    next_batch: NonZeroU64,
    cursor: ImportCursor,
    accepted_rows: u64,
    first_revision: CommitRevision,
    last_revision: CommitRevision,
    status: ImportJobStatus,
    chain_digest: [u8; 32],
}

impl ImportCheckpoint {
    #[must_use]
    pub const fn job(&self) -> RecordRef {
        self.job
    }
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version.get()
    }
    #[must_use]
    pub const fn source(&self) -> &SourceBinding {
        &self.source
    }
    #[must_use]
    pub const fn mapping(&self) -> &MappingBinding {
        &self.mapping
    }
    #[must_use]
    pub const fn next_batch(&self) -> u64 {
        self.next_batch.get()
    }
    #[must_use]
    pub const fn cursor(&self) -> ImportCursor {
        self.cursor
    }
    #[must_use]
    pub const fn accepted_rows(&self) -> u64 {
        self.accepted_rows
    }
    #[must_use]
    pub const fn first_revision(&self) -> CommitRevision {
        self.first_revision
    }
    #[must_use]
    pub const fn last_revision(&self) -> CommitRevision {
        self.last_revision
    }
    #[must_use]
    pub const fn status(&self) -> ImportJobStatus {
        self.status
    }
    #[must_use]
    pub const fn chain_digest(&self) -> [u8; 32] {
        self.chain_digest
    }

    pub(crate) fn started(
        job: RecordRef,
        source: SourceBinding,
        mapping: MappingBinding,
        revision: CommitRevision,
        chain_digest: [u8; 32],
    ) -> Self {
        Self {
            job,
            version: NonZeroU64::MIN,
            source,
            mapping,
            next_batch: NonZeroU64::MIN,
            cursor: ImportCursor::START,
            accepted_rows: 0,
            first_revision: revision,
            last_revision: revision,
            status: ImportJobStatus::Open,
            chain_digest,
        }
    }

    pub(crate) fn advanced(
        &self,
        rows: u64,
        revision: CommitRevision,
        completed: bool,
        chain_digest: [u8; 32],
    ) -> Result<Self, ImportError> {
        let accepted_rows = self
            .accepted_rows
            .checked_add(rows)
            .ok_or(ImportError::ResourceLimit)?;
        let next_row = self
            .cursor
            .next_row
            .checked_add(rows)
            .ok_or(ImportError::ResourceLimit)?;
        if accepted_rows > MAX_IMPORT_ROWS || next_row > MAX_IMPORT_ROWS {
            return Err(ImportError::ResourceLimit);
        }
        Ok(Self {
            job: self.job,
            version: NonZeroU64::new(
                self.version
                    .get()
                    .checked_add(1)
                    .ok_or(ImportError::VersionExhausted)?,
            )
            .ok_or(ImportError::VersionExhausted)?,
            source: self.source.clone(),
            mapping: self.mapping.clone(),
            next_batch: NonZeroU64::new(
                self.next_batch
                    .get()
                    .checked_add(1)
                    .ok_or(ImportError::VersionExhausted)?,
            )
            .ok_or(ImportError::VersionExhausted)?,
            cursor: ImportCursor::new(next_row)?,
            accepted_rows,
            first_revision: self.first_revision,
            last_revision: revision,
            status: if completed {
                ImportJobStatus::Completed
            } else {
                ImportJobStatus::Open
            },
            chain_digest,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        job: RecordRef,
        version: u64,
        source: SourceBinding,
        mapping: MappingBinding,
        next_batch: u64,
        cursor: ImportCursor,
        accepted_rows: u64,
        first_revision: CommitRevision,
        last_revision: CommitRevision,
        status: ImportJobStatus,
        chain_digest: [u8; 32],
    ) -> Result<Self, ImportError> {
        if accepted_rows > MAX_IMPORT_ROWS
            || accepted_rows != cursor.next_row()
            || first_revision > last_revision
        {
            return Err(ImportError::InvalidCheckpoint);
        }
        Ok(Self {
            job,
            version: NonZeroU64::new(version).ok_or(ImportError::InvalidCheckpoint)?,
            source,
            mapping,
            next_batch: NonZeroU64::new(next_batch).ok_or(ImportError::InvalidCheckpoint)?,
            cursor,
            accepted_rows,
            first_revision,
            last_revision,
            status,
            chain_digest,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportBatch {
    id: ImportBatchId,
    expected: ImportCheckpoint,
    start: ImportCursor,
    rows: Vec<MappedRowReceipt>,
    graph_request: Vec<u8>,
    spatial_request: Option<Vec<u8>>,
    final_batch: bool,
}

impl ImportBatch {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ImportBatchId,
        expected: ImportCheckpoint,
        start: ImportCursor,
        rows: Vec<MappedRowReceipt>,
        graph_request: Vec<u8>,
        spatial_request: Option<Vec<u8>>,
        final_batch: bool,
    ) -> Result<Self, ImportError> {
        if rows.is_empty() || rows.len() > MAX_ROWS_PER_BATCH {
            return Err(ImportError::ResourceLimit);
        }
        if id.job != expected.job || start != expected.cursor || graph_request.is_empty() {
            return Err(ImportError::InvalidBatch);
        }
        Ok(Self {
            id,
            expected,
            start,
            rows,
            graph_request,
            spatial_request,
            final_batch,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ImportBatchId {
        self.id
    }
    #[must_use]
    pub const fn expected(&self) -> &ImportCheckpoint {
        &self.expected
    }
    #[must_use]
    pub const fn start(&self) -> ImportCursor {
        self.start
    }
    #[must_use]
    pub fn rows(&self) -> &[MappedRowReceipt] {
        &self.rows
    }
    #[must_use]
    pub fn graph_request(&self) -> &[u8] {
        &self.graph_request
    }
    #[must_use]
    pub fn spatial_request(&self) -> Option<&[u8]> {
        self.spatial_request.as_deref()
    }
    #[must_use]
    pub const fn final_batch(&self) -> bool {
        self.final_batch
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportStart {
    job: RecordRef,
    source: SourceBinding,
    mapping: MappingBinding,
    graph_request: Vec<u8>,
}

impl ImportStart {
    pub fn new(
        job: RecordRef,
        source: SourceBinding,
        mapping: MappingBinding,
        graph_request: Vec<u8>,
    ) -> Result<Self, ImportError> {
        if graph_request.is_empty() {
            return Err(ImportError::InvalidBatch);
        }
        Ok(Self {
            job,
            source,
            mapping,
            graph_request,
        })
    }

    #[must_use]
    pub const fn job(&self) -> RecordRef {
        self.job
    }
    #[must_use]
    pub const fn source(&self) -> &SourceBinding {
        &self.source
    }
    #[must_use]
    pub const fn mapping(&self) -> &MappingBinding {
        &self.mapping
    }
    #[must_use]
    pub fn graph_request(&self) -> &[u8] {
        &self.graph_request
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportAction {
    StartJob(Box<ImportStart>),
    ApplyBatch(Box<ImportBatch>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngineTransaction {
    Graph {
        scope: NamespaceRef,
        graph_request: Vec<u8>,
    },
    Import {
        scope: NamespaceRef,
        action: Box<ImportAction>,
    },
}

impl EngineTransaction {
    #[must_use]
    pub fn graph(scope: NamespaceRef, graph_request: Vec<u8>) -> Self {
        Self::Graph {
            scope,
            graph_request,
        }
    }

    #[must_use]
    pub fn start_import(scope: NamespaceRef, start: ImportStart) -> Self {
        Self::Import {
            scope,
            action: Box::new(ImportAction::StartJob(Box::new(start))),
        }
    }

    #[must_use]
    pub fn apply_import_batch(scope: NamespaceRef, batch: ImportBatch) -> Self {
        Self::Import {
            scope,
            action: Box::new(ImportAction::ApplyBatch(Box::new(batch))),
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        match self {
            Self::Graph { scope, .. } | Self::Import { scope, .. } => *scope,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportBatchOutcome {
    GraphCommitted,
    JobStarted(ImportCheckpoint),
    BatchCommitted {
        id: ImportBatchId,
        revision: CommitRevision,
        accepted: u32,
        checkpoint: ImportCheckpoint,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportJob {
    pub(crate) checkpoint: ImportCheckpoint,
}

impl ImportJob {
    #[must_use]
    pub const fn checkpoint(&self) -> &ImportCheckpoint {
        &self.checkpoint
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportReadRequest {
    Job { id: RecordRef },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportReadOutput {
    Job(Option<ImportJob>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportError {
    InvalidEncoding,
    UnsupportedProfile,
    InvalidVersion,
    VersionExhausted,
    ScopeMismatch,
    InvalidBatch,
    InvalidCheckpoint,
    MissingJob,
    JobExists,
    JobCompleted,
    SourceChanged,
    BatchConflict,
    SourceEventConflict,
    MissingReference,
    WrongReferenceKind,
    InventoryMismatch,
    ResourceLimit,
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ImportError {}
