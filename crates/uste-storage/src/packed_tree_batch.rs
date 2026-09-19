//! Bounded private copy-on-write batches. No authorization, journal commit or root publication.
use crate::{
    FileSystem, IndexDelta,
    journal::StorageError,
    ordered_commitment::{self as logical, CommitmentContext, OrderedCommitment},
    packed_index_pack::{ImmutablePack, PackWriteLimits},
    packed_index_page::PackedPageContext,
    packed_tree_lookup::TreeReadContext,
    packed_tree_record::{ChildReference, PackedLocator},
};
use uste_crypto::{EntropySource, KeyVault};

mod emit;
mod plan;

pub const MAX_BATCH_DELTAS: usize = 512;
pub const MAX_BATCH_INPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_BATCH_DIRTY_NODES: usize = 65_536;
pub const MAX_BATCH_METADATA_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_BATCH_READ_PAGES: u64 =
    MAX_BATCH_DELTAS as u64 * (logical::MAX_BRANCH_BITS as u64 + 1);
pub const MAX_BATCH_READ_BYTES: u64 =
    MAX_BATCH_READ_PAGES * crate::packed_index_page::ENCODED_PAGE_BYTES as u64;

#[derive(Clone, Copy)]
pub struct TreeBatchLimits {
    pub maximum_deltas: usize,
    pub maximum_input_bytes: u64,
    pub maximum_dirty_nodes: usize,
    pub maximum_path_branches: u32,
    pub maximum_read_pages: u64,
    pub maximum_read_bytes: u64,
    pub pack: PackWriteLimits,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeBatchReport {
    pub input_bytes: u64,
    pub metadata_admitted_bytes: u64,
    pub read_pages: u64,
    pub read_bytes: u64,
    pub dirty_nodes: u64,
    pub written_nodes: u64,
    pub written_chunks: u64,
    pub written_pages: u64,
    pub written_payload_bytes: u64,
}

pub struct StagedTreeBatch {
    context: TreeReadContext,
    logical_root: OrderedCommitment,
    root: Option<ChildReference>,
    pack: Option<ImmutablePack>,
    report: TreeBatchReport,
}
impl StagedTreeBatch {
    pub fn context(&self) -> TreeReadContext {
        self.context
    }
    pub fn logical_root(&self) -> OrderedCommitment {
        self.logical_root
    }
    pub fn root(&self) -> Option<ChildReference> {
        self.root
    }
    pub fn pack(&self) -> Option<ImmutablePack> {
        self.pack
    }
    pub fn report(&self) -> TreeBatchReport {
        self.report
    }
}

#[derive(Clone, Copy)]
enum Source {
    Disk(PackedLocator),
    Dirty(usize),
}
#[derive(Clone, Copy)]
struct Link {
    claimed: OrderedCommitment,
    source: Source,
}
impl From<ChildReference> for Link {
    fn from(value: ChildReference) -> Self {
        Self {
            claimed: value.claimed,
            source: Source::Disk(value.location),
        }
    }
}
#[derive(Clone, Copy)]
enum DirtyNode {
    Leaf {
        delta: usize,
        value: logical::ValueCommitment,
    },
    Branch {
        bit: u32,
        left: Link,
        right: Link,
    },
}

struct Plan<'a> {
    deltas: &'a [IndexDelta],
    context: TreeReadContext,
    logical_context: CommitmentContext,
    root: Option<Link>,
    nodes: Vec<DirtyNode>,
    limits: TreeBatchLimits,
    report: TreeBatchReport,
}

#[allow(clippy::too_many_arguments)]
pub fn stage_batch<F: FileSystem, W, E: EntropySource, I: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    entropy: &mut I,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    deltas: &[IndexDelta],
    write: PackedPageContext,
    limits: TreeBatchLimits,
) -> Result<StagedTreeBatch, StorageError> {
    let (input_bytes, metadata_admitted_bytes) = admit(context, deltas, write, limits)?;
    let logical_context = CommitmentContext::new(context.scope, context.profile, context.family)
        .map_err(|_| StorageError::InvalidState)?;
    if root.is_none() != (expected == logical::empty_commitment(logical_context))
        || root.is_some() && expected.entries() == 0
    {
        return Err(StorageError::IntegrityFailure);
    }
    if let Some(location) = root {
        location.resolve(
            context.scope,
            context.profile,
            context.family,
            context.revision,
        )?;
    }
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(limits.maximum_dirty_nodes)
        .map_err(|_| StorageError::ResourceLimit)?;
    let mut plan = Plan {
        deltas,
        context,
        logical_context,
        root: root.map(|location| Link {
            claimed: expected,
            source: Source::Disk(location),
        }),
        nodes,
        limits,
        report: TreeBatchReport {
            input_bytes,
            metadata_admitted_bytes,
            ..TreeBatchReport::default()
        },
    };
    for index in 0..deltas.len() {
        plan.apply(filesystem, directory, vault, index)?;
    }
    plan.emit(filesystem, directory, vault, entropy, write)
}

fn admit(
    context: TreeReadContext,
    deltas: &[IndexDelta],
    write: PackedPageContext,
    limits: TreeBatchLimits,
) -> Result<(u64, u64), StorageError> {
    if context.family == 0
        || write.scope != context.scope
        || write.profile != context.profile
        || write.family != context.family
        || write.creation_revision < context.revision
        || write.object != [0; 16]
        || write.page != 0
    {
        return Err(StorageError::InvalidState);
    }
    if limits.maximum_deltas > MAX_BATCH_DELTAS
        || deltas.len() > limits.maximum_deltas
        || limits.maximum_input_bytes > MAX_BATCH_INPUT_BYTES
        || limits.maximum_dirty_nodes > MAX_BATCH_DIRTY_NODES
        || limits.maximum_path_branches > logical::MAX_BRANCH_BITS
        || limits.maximum_read_pages > MAX_BATCH_READ_PAGES
        || limits.maximum_read_bytes > MAX_BATCH_READ_BYTES
    {
        return Err(StorageError::ResourceLimit);
    }
    limits.pack.validate()?;
    let metadata = metadata_allowance(limits.maximum_dirty_nodes, limits.maximum_path_branches)?;
    let mut total = 0_u64;
    let mut previous: Option<&[u8]> = None;
    for delta in deltas {
        if previous.is_some_and(|key| key >= delta.key()) {
            return Err(StorageError::InvalidState);
        }
        previous = Some(delta.key());
        total = total
            .checked_add(delta.key().len() as u64)
            .and_then(|n| n.checked_add(delta.before().map_or(0, |value| value.len() as u64)))
            .and_then(|n| n.checked_add(delta.after().map_or(0, |value| value.len() as u64)))
            .filter(|n| *n <= limits.maximum_input_bytes)
            .ok_or(StorageError::ResourceLimit)?;
    }
    Ok((total, metadata))
}

fn metadata_allowance(nodes: usize, branches: u32) -> Result<u64, StorageError> {
    // Conservative simultaneous reservation accounting, not allocator or process RSS telemetry.
    let per_node = (size_of::<DirtyNode>()
        + size_of::<Option<ChildReference>>()
        + size_of::<u8>()
        + size_of::<usize>()
        + 2 * size_of::<(usize, bool)>()) as u64;
    let per_branch = (plan::FRAME_BYTES + size_of::<logical::BranchProof>()) as u64;
    (nodes as u64)
        .checked_mul(per_node)
        .and_then(|n| {
            (branches as u64)
                .checked_mul(per_branch)
                .and_then(|path| n.checked_add(path))
        })
        .and_then(|n| n.checked_add(logical::MAX_KEY_BYTES as u64 + 65_536))
        .filter(|n| *n <= MAX_BATCH_METADATA_BYTES)
        .ok_or(StorageError::ResourceLimit)
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn packed_tree_batch_metadata_geometry_fits_the_explicit_hard_budget() {
        assert!(
            metadata_allowance(MAX_BATCH_DIRTY_NODES, logical::MAX_BRANCH_BITS).unwrap()
                < MAX_BATCH_METADATA_BYTES
        );
        assert_eq!(
            metadata_allowance(usize::MAX, u32::MAX),
            Err(StorageError::ResourceLimit)
        );
    }

    #[test]
    fn packed_tree_batch_input_admission_is_exact_at_sixty_four_mib() {
        use uste_crypto::{KeyEpoch, WriterIncarnationId};
        use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};
        let context = TreeReadContext {
            scope: NamespaceRef::new(
                DatabaseId::from_bytes([1; 16]),
                NamespaceId::from_bytes([2; 16]),
            ),
            profile: [3; 32],
            family: 1,
            revision: CommitRevision::FIRST,
        };
        let write = PackedPageContext {
            scope: context.scope,
            profile: context.profile,
            family: context.family,
            creation_revision: context.revision,
            epoch: KeyEpoch::FIRST,
            writer: WriterIncarnationId::from_bytes([4; 16]),
            object: [0; 16],
            page: 0,
        };
        let limits = TreeBatchLimits {
            maximum_deltas: 2,
            maximum_input_bytes: MAX_BATCH_INPUT_BYTES,
            maximum_dirty_nodes: 10,
            maximum_path_branches: 10,
            maximum_read_pages: 10,
            maximum_read_bytes: 10_000,
            pack: PackWriteLimits {
                maximum_pages: 1,
                maximum_records: 1,
                maximum_payload_bytes: 1,
            },
        };
        let deltas = [
            IndexDelta::new(
                b"a".to_vec(),
                Some(vec![0; logical::MAX_VALUE_BYTES]),
                Some(vec![1; logical::MAX_VALUE_BYTES]),
            )
            .unwrap(),
            IndexDelta::new(
                b"b".to_vec(),
                Some(vec![0; logical::MAX_VALUE_BYTES]),
                Some(vec![1; logical::MAX_VALUE_BYTES - 2]),
            )
            .unwrap(),
        ];
        assert_eq!(
            admit(context, &deltas, write, limits).unwrap().0,
            MAX_BATCH_INPUT_BYTES
        );
        assert_eq!(
            admit(
                context,
                &deltas,
                write,
                TreeBatchLimits {
                    maximum_input_bytes: MAX_BATCH_INPUT_BYTES - 1,
                    ..limits
                }
            ),
            Err(StorageError::ResourceLimit)
        );
    }
}
