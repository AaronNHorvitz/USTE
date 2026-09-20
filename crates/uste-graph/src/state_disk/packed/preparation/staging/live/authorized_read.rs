use super::*;
mod expansion;
use crate::{GraphReadOutput, GraphReadRequest};
pub use expansion::PackedGraphExpansionLimits;
use uste_policy::{Action, AuthorizationRequirements, Target};
use uste_storage::packed_page_cache::PackedPageCache;
use uste_storage::packed_tree_lookup::TreeLookupLimits;
use uste_txn::{AuthorizedPackedReadState, AuthorizedReadState};

#[derive(Clone, Copy)]
pub struct PackedGraphReadLimits {
    pub current: TreeLookupLimits,
    /// Candidate/key/value work includes the historical key's 24 bytes.
    pub historical: TreeCursorLimits,
    /// None deliberately restricts the facade to point and historical reads.
    pub expansion: Option<PackedGraphExpansionLimits>,
}
impl<F, W, E, I> AuthorizedPackedReadState<F, W, E, I> for GraphPackedLiveState
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type ReadRequest = GraphReadRequest;
    type ReadOutput = GraphReadOutput;
    type ReadError = GraphDiskError;
    type ReadLimits = PackedGraphReadLimits;
    fn read_requirements(
        request: &GraphReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        <GraphState as AuthorizedReadState>::read_authorization_requirements(request)
    }
    fn read_packed_authorized(
        coordinator: &PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        request: &GraphReadRequest,
        limits: &PackedGraphReadLimits,
        cache: Option<&mut PackedPageCache>,
        authorize: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<GraphReadOutput, GraphDiskError> {
        let base = coordinator
            .state()?
            .current_base()
            .ok_or(GraphDiskError::RootStateMismatch)?;
        let reader = coordinator.packed_index_reader()?;
        if reader.anchor() != base.anchor {
            return Err(GraphDiskError::RootStateMismatch);
        }
        let record = match request {
            GraphReadRequest::Record { id } => {
                let tree = &base.trees[usize::from(FAMILY_CURRENT_RECORD - 1)];
                let key = id.record();
                let found = match cache {
                    Some(cache) => {
                        reader.get_cached(fs, tree, key.as_bytes(), limits.current, cache)
                    }
                    None => reader.get(fs, tree, key.as_bytes(), limits.current),
                }?;
                found
                    .value
                    .map(|value| {
                        let record = decode_stored_record(value.as_slice())?;
                        if record.id() != *id || record.modified_revision() > base.anchor.0 {
                            return Err(GraphDiskError::IndexCorrupt);
                        }
                        Ok(record)
                    })
                    .transpose()?
            }
            GraphReadRequest::RecordAt { id, revision } => {
                if *revision > base.anchor.0 {
                    return Err(crate::GraphError::UnknownReadView(*revision).into());
                }
                let mut cursor = reader.reverse_cursor(
                    &base.trees[usize::from(FAMILY_RECORD_HISTORY - 1)],
                    id.record().as_bytes(),
                    Some(&history_key(*id, *revision)),
                    limits.historical,
                )?;
                let next = match cache {
                    Some(cache) => reader.next_cached(fs, &mut cursor, cache),
                    None => reader.next(fs, &mut cursor),
                }?;
                next.map(|entry| {
                    if entry.key().len() != 24 || entry.key()[..16] != *id.record().as_bytes() {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    let selected = CommitRevision::new(
                        read_u64_be(&entry.key()[16..]).map_err(GraphDiskError::Storage)?,
                    )
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                    let record = decode_stored_record(entry.value())?;
                    if record.id() != *id
                        || record.modified_revision() != selected
                        || selected > *revision
                    {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    Ok(record)
                })
                .transpose()?
            }
            GraphReadRequest::Adjacent { .. } | GraphReadRequest::SupportedBy { .. } => {
                return expansion::read(
                    &reader,
                    fs,
                    base,
                    request,
                    limits.expansion.ok_or(GraphDiskError::UnsupportedRequest)?,
                    cache,
                    authorize,
                );
            }
        };
        Ok(GraphReadOutput::Record(crate::query::visible_record(
            record.as_ref(),
            authorize,
        )))
    }
}
