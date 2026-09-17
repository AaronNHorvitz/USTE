//! Transactional, evidence-backed graph records and rebuildable in-memory indexes.

#![forbid(unsafe_code)]

mod codec;
mod disk;
mod query;
mod record;
mod state;
mod state_disk;

pub use codec::{
    GraphCodecError, decode_stored_record, decode_transaction, encode_stored_record,
    encode_transaction,
};
pub use disk::{
    AuthorizedGraphIndex, CurrentGraphIndexRoot, GRAPH_INDEX_PROFILE_V1, GraphDiskError,
    GraphDiskReadContext, disk_adjacent_ids, disk_record, disk_supported_ids,
    load_current_graph_index_roots, publish_current_graph_index, scrub_current_graph_index,
};
pub use query::{GraphNeighbor, GraphReadOutput, GraphReadRequest};
pub use record::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, DurablePolicyMutation,
    EntityLifecycle, EntityRecord, EvidenceRecord, Expected, GraphTransaction, IntervalBound,
    NewAssertion, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation, Predicate, Record,
    RecordVersion, RecordVersionError, RelationshipRecord, ValidTime,
};
pub use state::{
    AdjacencyDirection, GraphError, GraphSnapshot, GraphState, MAX_TRANSACTION_OPERATIONS,
    MAX_TRANSACTION_REFERENCES, MAX_TRAVERSAL_RESULTS, MAX_TRAVERSAL_VISITS,
};
pub use state_disk::{
    DerivedGraphStateRoot, GRAPH_STATE_PROFILE_V1, GraphStateLoadLimits, GraphStateLoadReport,
    GraphStateRootCandidate, load_graph_state_root_candidates, load_graph_state_roots,
    publish_graph_state_root, reconstruct_graph_state_candidate, scrub_graph_state_root,
};
