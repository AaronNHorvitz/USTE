//! Bounded source-backed memory profile built on USTE's journal, blob and policy boundaries.
//!
//! This crate is an experimental derived-index profile. It does not make USTE an authoritative
//! source store and does not provide physical-erasure, larger-than-memory or production claims.

#![forbid(unsafe_code)]

mod profile;
mod query;
mod research;
mod state;

pub use profile::{PILOT_PROFILE, PilotProfile, PilotProfileError};
pub use query::{
    Citation, EventTimeFilter, FactView, KnowledgeAt, MemoryReadError, MemoryReadOutput,
    MemoryReadRequest, SearchResults,
};
pub use research::{
    ArtifactInput, CitationInput, ClaimInput, ClaimStatus, EdgeInput, EdgeKind, FetchOutcome,
    Freshness, ProducerIdentity, RESEARCH_HEADER_BYTES, RESEARCH_PROFILE,
    RESEARCH_TRANSACTION_HEADER_BYTES, ResearchArtifactEntry, ResearchClaimEntry,
    ResearchCodecError, ResearchEdgeEntry, ResearchMutation, ResearchProfile, ResearchRecord,
    ResearchSourceEntry, ResearchState, ResearchTransaction, RetainedContent, SourceKind,
    SourceRecordInput, SupportKind, decode_research_record, decode_research_transaction,
    encode_research_record, encode_research_transaction,
};
pub use state::{
    FactInput, FactRecord, FactTerminal, MemoryCodecError, MemoryMutation, MemoryState,
    MemoryTransaction, OPAQUE_MEDIA_TYPE, SourceLocator, SourceVersionId, SourceVersionInput,
    SourceVersionRecord, TEXT_MEDIA_TYPE, decode_transaction, encode_transaction,
};
