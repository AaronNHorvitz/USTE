//! `research-memory-v1` record contract and canonical codec (DB-R02.2).
//!
//! These records let a consumer retain what it learned from web research, documentation packs
//! and repositories with exact provenance. USTE validates and stores them; it never fetches,
//! resolves, searches, executes or authorizes anything described by a record. See
//! `docs/research-memory-records.md` for the contract this module implements.

use uste_storage::{BlobId, BlobReference};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant};

use crate::{SourceLocator, SourceVersionId};

const MAGIC: [u8; 4] = *b"URSM";
const FORMAT_MAJOR: u8 = 1;
const FORMAT_MINOR: u8 = 0;
/// Magic, major, minor, kind, reserved byte and the 32-byte namespace scope.
pub const RESEARCH_HEADER_BYTES: usize = 40;

/// Frozen per-namespace limits for `research-memory-v1`. Values change only by a versioned
/// decision; requests can never raise them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResearchProfile {
    pub profile_name: &'static str,
    pub schema_version: u16,
    pub maximum_sources: usize,
    pub maximum_source_versions: usize,
    pub maximum_source_bytes: u64,
    pub maximum_source_bytes_per_version: u64,
    pub maximum_claims: usize,
    pub maximum_edges: usize,
    pub maximum_citations_per_claim: usize,
    pub maximum_edges_per_record: usize,
    pub maximum_artifact_inputs: usize,
    pub maximum_claim_field_bytes: usize,
    pub maximum_excerpt_bytes: usize,
    pub maximum_locator_text_bytes: usize,
    pub maximum_label_bytes: usize,
    pub maximum_reason_bytes: usize,
    pub maximum_media_type_bytes: usize,
    pub maximum_request_bytes: usize,
    pub maximum_query_candidates: usize,
    pub maximum_query_results: usize,
    pub maximum_query_output_bytes: usize,
}

pub const RESEARCH_PROFILE: ResearchProfile = ResearchProfile {
    profile_name: "research-memory-v1",
    schema_version: 1,
    maximum_sources: 65_536,
    maximum_source_versions: 262_144,
    maximum_source_bytes: 4 * 1024 * 1024 * 1024,
    maximum_source_bytes_per_version: 16 * 1024 * 1024,
    maximum_claims: 1_048_576,
    maximum_edges: 4_194_304,
    maximum_citations_per_claim: 16,
    maximum_edges_per_record: 256,
    maximum_artifact_inputs: 16,
    maximum_claim_field_bytes: 4 * 1024,
    maximum_excerpt_bytes: 4 * 1024,
    maximum_locator_text_bytes: 2 * 1024,
    maximum_label_bytes: 256,
    maximum_reason_bytes: 1024,
    maximum_media_type_bytes: 256,
    maximum_request_bytes: 1024 * 1024,
    maximum_query_candidates: 65_536,
    maximum_query_results: 256,
    maximum_query_output_bytes: 1024 * 1024,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResearchCodecError {
    /// Malformed, non-canonical or semantically inconsistent bytes or input.
    Invalid,
    /// A frozen profile limit would be exceeded.
    ResourceLimit,
    /// Unknown magic, format major or minor version.
    UnsupportedVersion,
    /// A reference, blob or input names another namespace.
    ScopeMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    WebPage,
    DocPackPage,
    RepositoryFile,
    RepositoryManifest,
    SuppliedDocument,
}

/// Consumer-reported acquisition result. Storage never retries or refetches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchOutcome {
    Complete,
    Partial { reason: String },
    Truncated { limit_bytes: u64 },
    Inaccessible { reason: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Freshness {
    /// Versioned content that does not expire, such as a documentation pack release.
    Pinned,
    /// Stale once `retrieved_at + seconds` has passed at the query's evaluation instant.
    MaxAge { seconds: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedContent {
    pub blob: BlobReference,
    pub media_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRecordInput {
    pub id: SourceVersionId,
    pub kind: SourceKind,
    /// URL, pack-relative path or repository-relative path. An attribute, never an identity.
    pub locator_text: String,
    /// Documentation version, repository commit or `unversioned`.
    pub version_label: String,
    pub retrieved_at: UtcInstant,
    /// Opaque consumer run identity; not an authority.
    pub run_identity: [u8; 16],
    pub route_label: String,
    pub outcome: FetchOutcome,
    /// Retained bytes; absent exactly when the outcome is `Inaccessible`.
    pub content: Option<RetainedContent>,
    pub license_label: String,
    pub redistributable: bool,
    pub freshness: Freshness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProducerIdentity {
    pub name: String,
    pub revision: String,
    pub configuration_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactInput {
    pub id: RecordRef,
    pub inputs: Vec<SourceVersionId>,
    pub producer: ProducerIdentity,
    pub content: Option<RetainedContent>,
    pub complete_coverage: bool,
    pub limitations: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportKind {
    DirectQuote,
    Paraphrase,
    Inference,
    /// No resolvable citation; returned with this label, never as supported.
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitationInput {
    pub source: SourceVersionId,
    pub locator: SourceLocator,
    /// SHA-256 of the exact cited excerpt bytes.
    pub excerpt_digest: [u8; 32],
    /// Optional bounded exact excerpt; when present its SHA-256 must equal `excerpt_digest`.
    pub excerpt: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimInput {
    pub id: RecordRef,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub support: SupportKind,
    pub citations: Vec<CitationInput>,
    pub valid_from: Option<UtcInstant>,
    pub valid_until: Option<UtcInstant>,
    /// Predecessor claim when this version corrects an earlier one.
    pub corrects: Option<RecordRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeKind {
    Supports,
    Contradicts,
    Corrects,
    DerivedFrom,
    DependsOn,
    Documents,
    Related,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeInput {
    pub id: RecordRef,
    pub kind: EdgeKind,
    pub from: RecordRef,
    pub to: RecordRef,
    /// The claim or artifact that asserts this relation.
    pub asserted_by: RecordRef,
    pub valid_from: Option<UtcInstant>,
    pub valid_until: Option<UtcInstant>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResearchRecord {
    Source(SourceRecordInput),
    Artifact(ArtifactInput),
    Claim(ClaimInput),
    Edge(EdgeInput),
}

impl ResearchRecord {
    const fn kind_tag(&self) -> u8 {
        match self {
            Self::Source(_) => 1,
            Self::Artifact(_) => 2,
            Self::Claim(_) => 3,
            Self::Edge(_) => 4,
        }
    }

    /// Validate structure, scope and every frozen limit without storage access.
    pub fn validate(&self, scope: NamespaceRef) -> Result<(), ResearchCodecError> {
        match self {
            Self::Source(source) => validate_source(scope, source),
            Self::Artifact(artifact) => validate_artifact(scope, artifact),
            Self::Claim(claim) => validate_claim(scope, claim),
            Self::Edge(edge) => validate_edge(scope, edge),
        }
    }
}

/// Encode one validated record in its single canonical byte form.
pub fn encode_research_record(
    scope: NamespaceRef,
    record: &ResearchRecord,
) -> Result<Vec<u8>, ResearchCodecError> {
    record.validate(scope)?;
    let mut output = Vec::new();
    output
        .try_reserve(4096)
        .map_err(|_| ResearchCodecError::ResourceLimit)?;
    output.extend_from_slice(&MAGIC);
    output.extend_from_slice(&[FORMAT_MAJOR, FORMAT_MINOR, record.kind_tag(), 0]);
    output.extend_from_slice(scope.database().as_bytes());
    output.extend_from_slice(scope.namespace().as_bytes());
    match record {
        ResearchRecord::Source(source) => encode_source(source, &mut output)?,
        ResearchRecord::Artifact(artifact) => encode_artifact(artifact, &mut output)?,
        ResearchRecord::Claim(claim) => encode_claim(claim, &mut output)?,
        ResearchRecord::Edge(edge) => encode_edge(edge, &mut output),
    }
    if output.len() > RESEARCH_PROFILE.maximum_request_bytes {
        return Err(ResearchCodecError::ResourceLimit);
    }
    Ok(output)
}

/// Decode and validate one canonical record. Any byte sequence that decodes is exactly the
/// encoding of the returned record; everything else fails closed.
pub fn decode_research_record(
    input: &[u8],
) -> Result<(NamespaceRef, ResearchRecord), ResearchCodecError> {
    if input.len() > RESEARCH_PROFILE.maximum_request_bytes {
        return Err(ResearchCodecError::ResourceLimit);
    }
    let mut cursor = Cursor::new(input);
    if cursor.take(4)? != MAGIC {
        return Err(ResearchCodecError::UnsupportedVersion);
    }
    if cursor.byte()? != FORMAT_MAJOR || cursor.byte()? != FORMAT_MINOR {
        return Err(ResearchCodecError::UnsupportedVersion);
    }
    let kind = cursor.byte()?;
    if cursor.byte()? != 0 {
        return Err(ResearchCodecError::Invalid);
    }
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes(cursor.array()?),
        NamespaceId::from_bytes(cursor.array()?),
    );
    let record = match kind {
        1 => ResearchRecord::Source(decode_source(scope, &mut cursor)?),
        2 => ResearchRecord::Artifact(decode_artifact(scope, &mut cursor)?),
        3 => ResearchRecord::Claim(decode_claim(scope, &mut cursor)?),
        4 => ResearchRecord::Edge(decode_edge(scope, &mut cursor)?),
        _ => return Err(ResearchCodecError::UnsupportedVersion),
    };
    if cursor.remaining() != 0 {
        return Err(ResearchCodecError::Invalid);
    }
    record.validate(scope)?;
    Ok((scope, record))
}

fn same_scope(scope: NamespaceRef, record: RecordRef) -> Result<(), ResearchCodecError> {
    if record.database() == scope.database() && record.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(ResearchCodecError::ScopeMismatch)
    }
}

fn bounded_text(value: &str, maximum: usize, allow_empty: bool) -> Result<(), ResearchCodecError> {
    if value.len() > maximum {
        return Err(ResearchCodecError::ResourceLimit);
    }
    if value.is_empty() && !allow_empty {
        return Err(ResearchCodecError::Invalid);
    }
    Ok(())
}

fn validate_interval(
    from: Option<UtcInstant>,
    until: Option<UtcInstant>,
) -> Result<(), ResearchCodecError> {
    match (from, until) {
        (Some(from), Some(until)) if from >= until => Err(ResearchCodecError::Invalid),
        _ => Ok(()),
    }
}

fn validate_source_id(scope: NamespaceRef, id: SourceVersionId) -> Result<(), ResearchCodecError> {
    same_scope(scope, id.source)?;
    if id.version == 0 {
        return Err(ResearchCodecError::Invalid);
    }
    Ok(())
}

fn validate_content(
    scope: NamespaceRef,
    content: &RetainedContent,
) -> Result<(), ResearchCodecError> {
    let blob_scope = content.blob.scope();
    if blob_scope.database() != scope.database() || blob_scope.namespace() != scope.namespace() {
        return Err(ResearchCodecError::ScopeMismatch);
    }
    if content.blob.byte_len() > RESEARCH_PROFILE.maximum_source_bytes_per_version {
        return Err(ResearchCodecError::ResourceLimit);
    }
    bounded_text(
        &content.media_type,
        RESEARCH_PROFILE.maximum_media_type_bytes,
        false,
    )
}

fn validate_source(
    scope: NamespaceRef,
    source: &SourceRecordInput,
) -> Result<(), ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    validate_source_id(scope, source.id)?;
    bounded_text(
        &source.locator_text,
        profile.maximum_locator_text_bytes,
        false,
    )?;
    bounded_text(&source.version_label, profile.maximum_label_bytes, false)?;
    bounded_text(&source.route_label, profile.maximum_label_bytes, false)?;
    bounded_text(&source.license_label, profile.maximum_label_bytes, false)?;
    match (&source.outcome, &source.content) {
        (FetchOutcome::Inaccessible { reason }, None) => {
            bounded_text(reason, profile.maximum_reason_bytes, false)?;
        }
        (FetchOutcome::Inaccessible { .. }, Some(_)) | (_, None) => {
            return Err(ResearchCodecError::Invalid);
        }
        (FetchOutcome::Complete, Some(content)) => validate_content(scope, content)?,
        (FetchOutcome::Partial { reason }, Some(content)) => {
            bounded_text(reason, profile.maximum_reason_bytes, false)?;
            validate_content(scope, content)?;
        }
        (FetchOutcome::Truncated { limit_bytes }, Some(content)) => {
            validate_content(scope, content)?;
            if *limit_bytes == 0 || content.blob.byte_len() > *limit_bytes {
                return Err(ResearchCodecError::Invalid);
            }
        }
    }
    if matches!(source.freshness, Freshness::MaxAge { seconds: 0 }) {
        return Err(ResearchCodecError::Invalid);
    }
    Ok(())
}

fn validate_artifact(
    scope: NamespaceRef,
    artifact: &ArtifactInput,
) -> Result<(), ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    same_scope(scope, artifact.id)?;
    if artifact.inputs.is_empty() {
        return Err(ResearchCodecError::Invalid);
    }
    if artifact.inputs.len() > profile.maximum_artifact_inputs {
        return Err(ResearchCodecError::ResourceLimit);
    }
    for (index, input) in artifact.inputs.iter().enumerate() {
        validate_source_id(scope, *input)?;
        // Inputs are strictly ascending, which makes them duplicate-free and canonical.
        if index > 0 && artifact.inputs[index - 1] >= *input {
            return Err(ResearchCodecError::Invalid);
        }
    }
    bounded_text(&artifact.producer.name, profile.maximum_label_bytes, false)?;
    bounded_text(
        &artifact.producer.revision,
        profile.maximum_label_bytes,
        false,
    )?;
    if let Some(content) = &artifact.content {
        validate_content(scope, content)?;
    }
    bounded_text(&artifact.limitations, profile.maximum_reason_bytes, true)
}

fn validate_locator(locator: SourceLocator) -> Result<(), ResearchCodecError> {
    match locator {
        SourceLocator::ByteRange { start, end } if start < end => Ok(()),
        SourceLocator::Utf8Lines {
            start,
            end,
            first_line,
            last_line,
        } if start < end && first_line >= 1 && first_line <= last_line => Ok(()),
        _ => Err(ResearchCodecError::Invalid),
    }
}

fn validate_claim(scope: NamespaceRef, claim: &ClaimInput) -> Result<(), ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    same_scope(scope, claim.id)?;
    bounded_text(&claim.subject, profile.maximum_claim_field_bytes, false)?;
    bounded_text(&claim.predicate, profile.maximum_claim_field_bytes, false)?;
    bounded_text(&claim.value, profile.maximum_claim_field_bytes, true)?;
    if claim.citations.len() > profile.maximum_citations_per_claim {
        return Err(ResearchCodecError::ResourceLimit);
    }
    // An unsupported claim has no citation; every other support kind needs at least one.
    if (claim.support == SupportKind::Unsupported) != claim.citations.is_empty() {
        return Err(ResearchCodecError::Invalid);
    }
    for (index, citation) in claim.citations.iter().enumerate() {
        validate_source_id(scope, citation.source)?;
        validate_locator(citation.locator)?;
        if let Some(excerpt) = &citation.excerpt {
            bounded_text(excerpt, profile.maximum_excerpt_bytes, false)?;
            if sha256(excerpt.as_bytes()) != citation.excerpt_digest {
                return Err(ResearchCodecError::Invalid);
            }
        }
        if claim.citations[..index]
            .iter()
            .any(|earlier| earlier.source == citation.source && earlier.locator == citation.locator)
        {
            return Err(ResearchCodecError::Invalid);
        }
    }
    validate_interval(claim.valid_from, claim.valid_until)?;
    if let Some(predecessor) = claim.corrects {
        same_scope(scope, predecessor)?;
        if predecessor == claim.id {
            return Err(ResearchCodecError::Invalid);
        }
    }
    Ok(())
}

fn validate_edge(scope: NamespaceRef, edge: &EdgeInput) -> Result<(), ResearchCodecError> {
    for reference in [edge.id, edge.from, edge.to, edge.asserted_by] {
        same_scope(scope, reference)?;
    }
    if edge.from == edge.to {
        return Err(ResearchCodecError::Invalid);
    }
    validate_interval(edge.valid_from, edge.valid_until)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn encode_string(value: &str, output: &mut Vec<u8>) -> Result<(), ResearchCodecError> {
    let length = u32::try_from(value.len()).map_err(|_| ResearchCodecError::ResourceLimit)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
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

fn encode_source_id(id: SourceVersionId, output: &mut Vec<u8>) {
    output.extend_from_slice(id.source.record().as_bytes());
    output.extend_from_slice(&id.version.to_be_bytes());
}

fn encode_content(
    content: Option<&RetainedContent>,
    output: &mut Vec<u8>,
) -> Result<(), ResearchCodecError> {
    match content {
        Some(content) => {
            output.push(1);
            output.extend_from_slice(&content.blob.id().as_bytes());
            output.extend_from_slice(&content.blob.byte_len().to_be_bytes());
            output.extend_from_slice(&content.blob.chunk_count().to_be_bytes());
            output.extend_from_slice(&content.blob.content_digest());
            encode_string(&content.media_type, output)
        }
        None => {
            output.push(0);
            Ok(())
        }
    }
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

fn encode_count(count: usize, output: &mut Vec<u8>) -> Result<(), ResearchCodecError> {
    let count = u16::try_from(count).map_err(|_| ResearchCodecError::ResourceLimit)?;
    output.extend_from_slice(&count.to_be_bytes());
    Ok(())
}

fn encode_source(
    source: &SourceRecordInput,
    output: &mut Vec<u8>,
) -> Result<(), ResearchCodecError> {
    encode_source_id(source.id, output);
    output.push(match source.kind {
        SourceKind::WebPage => 1,
        SourceKind::DocPackPage => 2,
        SourceKind::RepositoryFile => 3,
        SourceKind::RepositoryManifest => 4,
        SourceKind::SuppliedDocument => 5,
    });
    encode_string(&source.locator_text, output)?;
    encode_string(&source.version_label, output)?;
    encode_instant(Some(source.retrieved_at), output);
    output.extend_from_slice(&source.run_identity);
    encode_string(&source.route_label, output)?;
    match &source.outcome {
        FetchOutcome::Complete => output.push(1),
        FetchOutcome::Partial { reason } => {
            output.push(2);
            encode_string(reason, output)?;
        }
        FetchOutcome::Truncated { limit_bytes } => {
            output.push(3);
            output.extend_from_slice(&limit_bytes.to_be_bytes());
        }
        FetchOutcome::Inaccessible { reason } => {
            output.push(4);
            encode_string(reason, output)?;
        }
    }
    encode_content(source.content.as_ref(), output)?;
    encode_string(&source.license_label, output)?;
    output.push(u8::from(source.redistributable));
    match source.freshness {
        Freshness::Pinned => output.push(1),
        Freshness::MaxAge { seconds } => {
            output.push(2);
            output.extend_from_slice(&seconds.to_be_bytes());
        }
    }
    Ok(())
}

fn encode_artifact(
    artifact: &ArtifactInput,
    output: &mut Vec<u8>,
) -> Result<(), ResearchCodecError> {
    output.extend_from_slice(artifact.id.record().as_bytes());
    encode_count(artifact.inputs.len(), output)?;
    for input in &artifact.inputs {
        encode_source_id(*input, output);
    }
    encode_string(&artifact.producer.name, output)?;
    encode_string(&artifact.producer.revision, output)?;
    output.extend_from_slice(&artifact.producer.configuration_digest);
    encode_content(artifact.content.as_ref(), output)?;
    output.push(u8::from(artifact.complete_coverage));
    encode_string(&artifact.limitations, output)
}

fn encode_claim(claim: &ClaimInput, output: &mut Vec<u8>) -> Result<(), ResearchCodecError> {
    output.extend_from_slice(claim.id.record().as_bytes());
    encode_string(&claim.subject, output)?;
    encode_string(&claim.predicate, output)?;
    encode_string(&claim.value, output)?;
    output.push(match claim.support {
        SupportKind::DirectQuote => 1,
        SupportKind::Paraphrase => 2,
        SupportKind::Inference => 3,
        SupportKind::Unsupported => 4,
    });
    encode_count(claim.citations.len(), output)?;
    for citation in &claim.citations {
        encode_source_id(citation.source, output);
        encode_locator(citation.locator, output);
        output.extend_from_slice(&citation.excerpt_digest);
        match &citation.excerpt {
            Some(excerpt) => {
                output.push(1);
                encode_string(excerpt, output)?;
            }
            None => output.push(0),
        }
    }
    encode_instant(claim.valid_from, output);
    encode_instant(claim.valid_until, output);
    match claim.corrects {
        Some(predecessor) => {
            output.push(1);
            output.extend_from_slice(predecessor.record().as_bytes());
        }
        None => output.push(0),
    }
    Ok(())
}

fn encode_edge(edge: &EdgeInput, output: &mut Vec<u8>) {
    output.extend_from_slice(edge.id.record().as_bytes());
    output.push(match edge.kind {
        EdgeKind::Supports => 1,
        EdgeKind::Contradicts => 2,
        EdgeKind::Corrects => 3,
        EdgeKind::DerivedFrom => 4,
        EdgeKind::DependsOn => 5,
        EdgeKind::Documents => 6,
        EdgeKind::Related => 7,
    });
    output.extend_from_slice(edge.from.record().as_bytes());
    output.extend_from_slice(edge.to.record().as_bytes());
    output.extend_from_slice(edge.asserted_by.record().as_bytes());
    encode_instant(edge.valid_from, output);
    encode_instant(edge.valid_until, output);
}

const fn record(scope: NamespaceRef, id: [u8; 16]) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(id),
    )
}

fn decode_source(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<SourceRecordInput, ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    let id = cursor.source_id(scope)?;
    let kind = match cursor.byte()? {
        1 => SourceKind::WebPage,
        2 => SourceKind::DocPackPage,
        3 => SourceKind::RepositoryFile,
        4 => SourceKind::RepositoryManifest,
        5 => SourceKind::SuppliedDocument,
        _ => return Err(ResearchCodecError::Invalid),
    };
    let locator_text = cursor.string(profile.maximum_locator_text_bytes)?;
    let version_label = cursor.string(profile.maximum_label_bytes)?;
    let retrieved_at = cursor.instant()?.ok_or(ResearchCodecError::Invalid)?;
    let run_identity = cursor.array()?;
    let route_label = cursor.string(profile.maximum_label_bytes)?;
    let outcome = match cursor.byte()? {
        1 => FetchOutcome::Complete,
        2 => FetchOutcome::Partial {
            reason: cursor.string(profile.maximum_reason_bytes)?,
        },
        3 => FetchOutcome::Truncated {
            limit_bytes: cursor.u64()?,
        },
        4 => FetchOutcome::Inaccessible {
            reason: cursor.string(profile.maximum_reason_bytes)?,
        },
        _ => return Err(ResearchCodecError::Invalid),
    };
    let content = cursor.content(scope)?;
    let license_label = cursor.string(profile.maximum_label_bytes)?;
    let redistributable = cursor.flag()?;
    let freshness = match cursor.byte()? {
        1 => Freshness::Pinned,
        2 => Freshness::MaxAge {
            seconds: cursor.u64()?,
        },
        _ => return Err(ResearchCodecError::Invalid),
    };
    Ok(SourceRecordInput {
        id,
        kind,
        locator_text,
        version_label,
        retrieved_at,
        run_identity,
        route_label,
        outcome,
        content,
        license_label,
        redistributable,
        freshness,
    })
}

fn decode_artifact(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<ArtifactInput, ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    let id = record(scope, cursor.array()?);
    let count = cursor.count(profile.maximum_artifact_inputs)?;
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(count)
        .map_err(|_| ResearchCodecError::ResourceLimit)?;
    for _ in 0..count {
        inputs.push(cursor.source_id(scope)?);
    }
    let producer = ProducerIdentity {
        name: cursor.string(profile.maximum_label_bytes)?,
        revision: cursor.string(profile.maximum_label_bytes)?,
        configuration_digest: cursor.array()?,
    };
    Ok(ArtifactInput {
        id,
        inputs,
        producer,
        content: cursor.content(scope)?,
        complete_coverage: cursor.flag()?,
        limitations: cursor.string(profile.maximum_reason_bytes)?,
    })
}

fn decode_claim(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<ClaimInput, ResearchCodecError> {
    let profile = RESEARCH_PROFILE;
    let id = record(scope, cursor.array()?);
    let subject = cursor.string(profile.maximum_claim_field_bytes)?;
    let predicate = cursor.string(profile.maximum_claim_field_bytes)?;
    let value = cursor.string(profile.maximum_claim_field_bytes)?;
    let support = match cursor.byte()? {
        1 => SupportKind::DirectQuote,
        2 => SupportKind::Paraphrase,
        3 => SupportKind::Inference,
        4 => SupportKind::Unsupported,
        _ => return Err(ResearchCodecError::Invalid),
    };
    let count = cursor.count(profile.maximum_citations_per_claim)?;
    let mut citations = Vec::new();
    citations
        .try_reserve_exact(count)
        .map_err(|_| ResearchCodecError::ResourceLimit)?;
    for _ in 0..count {
        let source = cursor.source_id(scope)?;
        let locator = cursor.locator()?;
        let excerpt_digest = cursor.array()?;
        let excerpt = if cursor.flag()? {
            Some(cursor.string(profile.maximum_excerpt_bytes)?)
        } else {
            None
        };
        citations.push(CitationInput {
            source,
            locator,
            excerpt_digest,
            excerpt,
        });
    }
    let valid_from = cursor.instant()?;
    let valid_until = cursor.instant()?;
    let corrects = if cursor.flag()? {
        Some(record(scope, cursor.array()?))
    } else {
        None
    };
    Ok(ClaimInput {
        id,
        subject,
        predicate,
        value,
        support,
        citations,
        valid_from,
        valid_until,
        corrects,
    })
}

fn decode_edge(
    scope: NamespaceRef,
    cursor: &mut Cursor<'_>,
) -> Result<EdgeInput, ResearchCodecError> {
    let id = record(scope, cursor.array()?);
    let kind = match cursor.byte()? {
        1 => EdgeKind::Supports,
        2 => EdgeKind::Contradicts,
        3 => EdgeKind::Corrects,
        4 => EdgeKind::DerivedFrom,
        5 => EdgeKind::DependsOn,
        6 => EdgeKind::Documents,
        7 => EdgeKind::Related,
        _ => return Err(ResearchCodecError::Invalid),
    };
    Ok(EdgeInput {
        id,
        kind,
        from: record(scope, cursor.array()?),
        to: record(scope, cursor.array()?),
        asserted_by: record(scope, cursor.array()?),
        valid_from: cursor.instant()?,
        valid_until: cursor.instant()?,
    })
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

    fn take(&mut self, count: usize) -> Result<&'a [u8], ResearchCodecError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ResearchCodecError::ResourceLimit)?;
        let bytes = self
            .input
            .get(self.offset..end)
            .ok_or(ResearchCodecError::Invalid)?;
        self.offset = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ResearchCodecError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ResearchCodecError::Invalid)
    }

    fn byte(&mut self) -> Result<u8, ResearchCodecError> {
        Ok(self.array::<1>()?[0])
    }

    /// Strict boolean or presence byte: exactly 0 or 1.
    fn flag(&mut self) -> Result<bool, ResearchCodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ResearchCodecError::Invalid),
        }
    }

    fn u32(&mut self) -> Result<u32, ResearchCodecError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ResearchCodecError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn count(&mut self, maximum: usize) -> Result<usize, ResearchCodecError> {
        let count = usize::from(u16::from_be_bytes(self.array()?));
        if count > maximum {
            return Err(ResearchCodecError::ResourceLimit);
        }
        Ok(count)
    }

    fn string(&mut self, maximum: usize) -> Result<String, ResearchCodecError> {
        let length = usize::try_from(self.u32()?).map_err(|_| ResearchCodecError::ResourceLimit)?;
        if length > maximum {
            return Err(ResearchCodecError::ResourceLimit);
        }
        let text =
            core::str::from_utf8(self.take(length)?).map_err(|_| ResearchCodecError::Invalid)?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(length)
            .map_err(|_| ResearchCodecError::ResourceLimit)?;
        owned.push_str(text);
        Ok(owned)
    }

    fn instant(&mut self) -> Result<Option<UtcInstant>, ResearchCodecError> {
        if !self.flag()? {
            return Ok(None);
        }
        UtcInstant::new(i64::from_be_bytes(self.array()?), self.u32()?)
            .map(Some)
            .map_err(|_| ResearchCodecError::Invalid)
    }

    fn source_id(&mut self, scope: NamespaceRef) -> Result<SourceVersionId, ResearchCodecError> {
        Ok(SourceVersionId {
            source: record(scope, self.array()?),
            version: self.u32()?,
        })
    }

    fn locator(&mut self) -> Result<SourceLocator, ResearchCodecError> {
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
            _ => Err(ResearchCodecError::Invalid),
        }
    }

    fn content(
        &mut self,
        scope: NamespaceRef,
    ) -> Result<Option<RetainedContent>, ResearchCodecError> {
        if !self.flag()? {
            return Ok(None);
        }
        let id = BlobId::from_bytes(self.array()?);
        let byte_len = self.u64()?;
        let chunk_count = self.u32()?;
        let content_digest = self.array()?;
        let blob = BlobReference::new(scope, id, byte_len, chunk_count, content_digest)
            .map_err(|_| ResearchCodecError::Invalid)?;
        Ok(Some(RetainedContent {
            blob,
            media_type: self.string(RESEARCH_PROFILE.maximum_media_type_bytes)?,
        }))
    }
}

#[cfg(test)]
mod tests;
