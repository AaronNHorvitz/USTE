//! Bounded source-backed memory profile built on USTE's journal, blob and policy boundaries.
//!
//! This crate is an experimental derived-index profile. It does not make USTE an authoritative
//! source store and does not provide physical-erasure, larger-than-memory or production claims.

#![forbid(unsafe_code)]

pub mod contract;
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
    ArtifactInput, CitationInput, CitationView, ClaimInput, ClaimStatus, ClaimView, EdgeInput,
    EdgeKind, EffectiveSupport, FetchOutcome, Freshness, FreshnessState,
    MAXIMUM_RESEARCH_QUERY_TERM_BYTES, MAXIMUM_RESEARCH_QUERY_TERMS, PILOT_MAPPING_LICENSE,
    PILOT_MAPPING_ROUTE, PilotMappedMutation, PilotMapping, PilotMappingContext, PilotMappingError,
    ProducerIdentity, RESEARCH_HEADER_BYTES, RESEARCH_PROFILE, RESEARCH_TRANSACTION_HEADER_BYTES,
    ResearchArtifactEntry, ResearchClaimEntry, ResearchCodecError, ResearchEdgeEntry,
    ResearchKnowledge, ResearchMutation, ResearchProfile, ResearchReadError, ResearchReadOutput,
    ResearchReadRequest, ResearchReadResults, ResearchRecord, ResearchSourceEntry, ResearchState,
    ResearchTransaction, RetainedContent, SourceKind, SourceRecordInput, SourceVersionView,
    SupportKind, decode_research_record, decode_research_transaction, encode_research_record,
    encode_research_transaction, map_memory_pilot, pilot_link_edge_id,
};
pub use state::{
    FactInput, FactRecord, FactTerminal, MemoryCodecError, MemoryMutation, MemoryState,
    MemoryTransaction, OPAQUE_MEDIA_TYPE, SourceLocator, SourceVersionId, SourceVersionInput,
    SourceVersionRecord, TEXT_MEDIA_TYPE, decode_transaction, encode_transaction,
};
