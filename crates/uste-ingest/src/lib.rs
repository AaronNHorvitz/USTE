//! Capability-free R1 import contracts and one atomic graph/spatial reducer.
//!
//! CSV/JSON parsing, filesystem access, mapping execution and CLI orchestration remain T-54.

#![forbid(unsafe_code)]

mod codec;
mod model;
mod state;

pub use codec::{decode_transaction, encode_transaction};
pub use model::{
    EngineTransaction, ImportAction, ImportBatch, ImportBatchId, ImportBatchOutcome,
    ImportCheckpoint, ImportCursor, ImportError, ImportJob, ImportJobStatus, ImportReadOutput,
    ImportReadRequest, ImportStart, MAX_IMPORT_BATCHES, MAX_IMPORT_JOBS, MAX_IMPORT_ROWS,
    MAX_ROWS_PER_BATCH, MappedRowReceipt, MappingBinding, MappingProfile, SourceBinding,
};
pub use state::{EngineSnapshot, ImportPreview, IngestState, PreparedIngest};
