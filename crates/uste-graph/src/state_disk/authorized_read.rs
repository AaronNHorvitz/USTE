//! Current/historical point reads from an admitted ready root.

use super::*;
use crate::{GraphReadOutput, GraphReadRequest};
use uste_policy::{Action, AuthorizationRequirements, Target};
use uste_txn::{AuthorizedDiskReadState, AuthorizedReadState, DiskCommitCoordinator};

/// Trusted-adapter admission. Historical bytes include the 24-byte key.
#[derive(Clone, Copy, Debug)]
pub struct GraphDiskRecordReadLimits {
    pub current: IndexGetLimits,
    pub historical: IndexPredecessorLimits,
}

impl<F, W, E, I> AuthorizedDiskReadState<F, W, E, I> for GraphDiskLiveState
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type ReadRequest = GraphReadRequest;
    type ReadOutput = GraphReadOutput;
    type ReadError = GraphDiskError;
    type ReadLimits = GraphDiskRecordReadLimits;

    fn read_requirements(
        request: &GraphReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        <GraphState as AuthorizedReadState>::read_authorization_requirements(request)
    }

    fn read_disk_authorized(
        coordinator: &DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        request: &GraphReadRequest,
        limits: &GraphDiskRecordReadLimits,
        cache: &mut PageCache,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<GraphReadOutput, GraphDiskError> {
        let state = coordinator.state()?;
        let base = state
            .current_base()
            .ok_or(GraphDiskError::RootStateMismatch)?;
        let root = &base.root.root;
        if coordinator.checkpoint_anchor()? != Some((root.revision(), *root.certificate_digest())) {
            return Err(GraphDiskError::RootStateMismatch);
        }
        let record = match request {
            GraphReadRequest::Record { id } => {
                if !has_family(root, FAMILY_CURRENT_RECORD) {
                    None
                } else {
                    let (value, _) = coordinator.index_get_bounded(
                        filesystem,
                        root,
                        FAMILY_CURRENT_RECORD,
                        id.record().as_bytes(),
                        limits.current,
                        cache,
                    )?;
                    value
                        .map(|encoded| {
                            let record = decode_stored_record(&encoded)?;
                            if record.id() != *id || record.modified_revision() > root.revision() {
                                return Err(GraphDiskError::IndexCorrupt);
                            }
                            Ok(record)
                        })
                        .transpose()?
                }
            }
            GraphReadRequest::RecordAt { id, revision } => {
                if *revision > root.revision() {
                    return Err(crate::GraphError::UnknownReadView(*revision).into());
                }
                if !has_family(root, FAMILY_RECORD_HISTORY) {
                    None
                } else {
                    let found = coordinator.index_get_predecessor(
                        filesystem,
                        root,
                        FAMILY_RECORD_HISTORY,
                        id.record().as_bytes(),
                        &history_key(*id, *revision),
                        limits.historical,
                        cache,
                    )?;
                    found
                        .entry
                        .map(|entry| {
                            let record = decode_history_lookup(root, *id, entry)?;
                            if record.modified_revision() > *revision {
                                return Err(GraphDiskError::IndexCorrupt);
                            }
                            Ok(record)
                        })
                        .transpose()?
                }
            }
            GraphReadRequest::Adjacent { .. } | GraphReadRequest::SupportedBy { .. } => {
                return Err(GraphDiskError::UnsupportedRequest);
            }
        };
        Ok(GraphReadOutput::Record(crate::query::visible_record(
            record.as_ref(),
            authorize_candidate,
        )))
    }
}
