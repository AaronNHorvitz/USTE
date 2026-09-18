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
    GraphIndexCacheReport, disk_adjacent_ids, disk_record, disk_supported_ids,
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
    MAX_TRANSACTION_REFERENCES, MAX_TRAVERSAL_RESULTS, MAX_TRAVERSAL_VISITS, PreparedGraph,
    PreparedGraphView,
};
pub use state_disk::{
    DerivedGraphStateRoot, DiskPreparedGraph, GRAPH_STATE_PROFILE_V1, GraphDiskBase,
    GraphDiskBaseAdmissionLimits, GraphDiskBaseAdmissionReport, GraphDiskPreparationLimits,
    GraphDiskPreparationReport, GraphDiskPreparationView, GraphRecoverySeedReport,
    GraphStateDeltaLimits, GraphStateLoadLimits, GraphStateLoadReport, GraphStateRootCandidate,
    GraphStateRootDelta, GraphStateRootMergeLimits, GraphStateRootMergeReport,
    admit_graph_disk_base_candidate, admit_graph_disk_base_candidate_for_recovery,
    commit_graph_disk_prepared, load_graph_disk_preparation_view, load_graph_state_root_candidates,
    load_graph_state_root_candidates_for_recovery, load_graph_state_roots,
    prepare_graph_state_root_delta, prepare_graph_state_root_delta_from_disk,
    publish_graph_state_root, publish_graph_state_root_delta, reconstruct_graph_recovery_seed,
    reconstruct_graph_state_candidate, reconstruct_graph_state_candidate_for_recovery,
    scrub_graph_state_root,
};
