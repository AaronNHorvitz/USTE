//! Complete bounded structural/content validation, without journal or authorization authority.
use crate::{
    FileSystem,
    journal::StorageError,
    ordered_commitment::{self as logical, CommitmentContext, OrderedCommitment, ValueStream},
    packed_index_pack::read_linked_record_page,
    packed_index_page::{ENCODED_PAGE_BYTES, PackedPage},
    packed_tree_lookup::TreeReadContext,
    packed_tree_record::{ChildReference, MAX_CHUNK_DATA, PackedLocator, TreeNode, ValueChunk},
};
use uste_crypto::{EntropySource, KeyVault};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_VALIDATION_NODES: u64 = 2 * logical::MAX_ENTRIES - 1;
pub const MAX_VALIDATION_LOGICAL_BYTES: u64 =
    logical::MAX_ENTRIES * (logical::MAX_KEY_BYTES + logical::MAX_VALUE_BYTES) as u64;
pub const MAX_VALIDATION_PAGES: u64 = MAX_VALIDATION_NODES
    + logical::MAX_ENTRIES * logical::MAX_VALUE_BYTES.div_ceil(MAX_CHUNK_DATA) as u64;
pub const MAX_VALIDATION_ENCODED_BYTES: u64 = MAX_VALIDATION_PAGES * ENCODED_PAGE_BYTES as u64;

#[derive(Clone, Copy)]
pub struct TreeValidationLimits {
    pub maximum_path_branches: u32,
    pub maximum_nodes: u64,
    pub maximum_logical_bytes: u64,
    pub maximum_pages: u64,
    pub maximum_encoded_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeValidationReport {
    pub nodes: u64,
    pub entries: u64,
    pub logical_bytes: u64,
    pub pages: u64,
    pub encoded_bytes: u64,
    pub value_chunks: u64,
    pub maximum_depth: u32,
}

/// A completed read, not protection against later file changes or a domain-admission receipt.
pub struct ValidatedPackedTree {
    context: TreeReadContext,
    commitment: OrderedCommitment,
    root: Option<PackedLocator>,
    report: TreeValidationReport,
}
impl ValidatedPackedTree {
    pub fn context(&self) -> TreeReadContext {
        self.context
    }
    pub fn commitment(&self) -> OrderedCommitment {
        self.commitment
    }
    pub fn root(&self) -> Option<PackedLocator> {
        self.root
    }
    pub fn report(&self) -> TreeValidationReport {
        self.report
    }
}

struct Frame {
    bit: u32,
    right: ChildReference,
    visiting_right: bool,
}
struct Reader<'a, F: FileSystem, W, E: EntropySource> {
    filesystem: &'a mut F,
    directory: &'a F::Directory,
    vault: &'a KeyVault<W, E>,
    context: TreeReadContext,
    limits: TreeValidationLimits,
    report: TreeValidationReport,
}
impl<F: FileSystem, W, E: EntropySource> Reader<'_, F, W, E> {
    fn page(&mut self, location: PackedLocator) -> Result<PackedPage, StorageError> {
        if self.report.pages >= self.limits.maximum_pages
            || ENCODED_PAGE_BYTES as u64
                > self.limits.maximum_encoded_bytes - self.report.encoded_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        let c = self.context;
        let physical = location.resolve(c.scope, c.profile, c.family, c.revision)?;
        let (page, report) = read_linked_record_page(
            self.filesystem,
            self.directory,
            self.vault,
            physical,
            location.slot(),
            ENCODED_PAGE_BYTES as u64,
        )?;
        self.report.pages += report.pages;
        self.report.encoded_bytes += report.encoded_bytes;
        Ok(page)
    }

    fn value(
        &mut self,
        expected: logical::ValueCommitment,
        mut next: Option<PackedLocator>,
    ) -> Result<(), StorageError> {
        let mut hash =
            ValueStream::new(expected.length).map_err(|_| StorageError::IntegrityFailure)?;
        let mut remaining = expected.length;
        while let Some(location) = next {
            let page = self.page(location)?;
            let c = self.context;
            let physical = location.resolve(c.scope, c.profile, c.family, c.revision)?;
            let chunk = ValueChunk::decode(
                physical,
                page.record(location.slot())
                    .ok_or(StorageError::IntegrityFailure)?,
            )?;
            if chunk.remaining as u64 != remaining {
                return Err(StorageError::IntegrityFailure);
            }
            hash.update(chunk.data)
                .map_err(|_| StorageError::IntegrityFailure)?;
            remaining -= chunk.data.len() as u64;
            next = chunk.next;
            self.report.value_chunks += 1;
        }
        if remaining != 0 || hash.finish().map_err(|_| StorageError::IntegrityFailure)? != expected
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn validate_tree<F: FileSystem, W, E: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    limits: TreeValidationLimits,
) -> Result<ValidatedPackedTree, StorageError> {
    let logical_context = CommitmentContext::new(context.scope, context.profile, context.family)
        .map_err(|_| StorageError::InvalidState)?;
    if limits.maximum_path_branches > logical::MAX_BRANCH_BITS
        || limits.maximum_nodes > MAX_VALIDATION_NODES
        || limits.maximum_logical_bytes > MAX_VALIDATION_LOGICAL_BYTES
        || limits.maximum_pages > MAX_VALIDATION_PAGES
        || limits.maximum_encoded_bytes > MAX_VALIDATION_ENCODED_BYTES
        || expected.entries().saturating_mul(2).saturating_sub(1) > limits.maximum_nodes
        || expected.logical_bytes() > limits.maximum_logical_bytes
    {
        return Err(StorageError::ResourceLimit);
    }
    if root.is_none() != (expected == logical::empty_commitment(logical_context))
        || root.is_some() && expected.entries() == 0
    {
        return Err(StorageError::IntegrityFailure);
    }
    let mut path: Vec<Frame> = Vec::new();
    path.try_reserve_exact(limits.maximum_path_branches as usize)
        .map_err(|_| StorageError::ResourceLimit)?;
    let mut previous = Zeroizing::new(Vec::new());
    previous
        .try_reserve_exact(logical::MAX_KEY_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    let mut reader = Reader {
        filesystem,
        directory,
        vault,
        context,
        limits,
        report: TreeValidationReport::default(),
    };
    let mut current = root.map(|location| ChildReference {
        location,
        claimed: expected,
    });
    let mut boundary = None;
    while let Some(link) = current {
        if reader.report.nodes >= limits.maximum_nodes {
            return Err(StorageError::ResourceLimit);
        }
        let page = reader.page(link.location)?;
        let physical = link.location.resolve(
            context.scope,
            context.profile,
            context.family,
            context.revision,
        )?;
        let node = TreeNode::decode(
            physical,
            page.record(link.location.slot())
                .ok_or(StorageError::IntegrityFailure)?,
        )?;
        if node.commitment(physical)? != link.claimed {
            return Err(StorageError::IntegrityFailure);
        }
        reader.report.nodes += 1;
        match node {
            TreeNode::Branch { bit, left, right } => {
                if path.len() >= limits.maximum_path_branches as usize {
                    return Err(StorageError::ResourceLimit);
                }
                if path.last().is_some_and(|parent| parent.bit >= bit)
                    || left.claimed.entries() >= link.claimed.entries()
                    || right.claimed.entries() >= link.claimed.entries()
                {
                    return Err(StorageError::IntegrityFailure);
                }
                path.push(Frame {
                    bit,
                    right,
                    visiting_right: false,
                });
                reader.report.maximum_depth = reader.report.maximum_depth.max(path.len() as u32);
                current = Some(left);
            }
            TreeNode::Leaf {
                key,
                value,
                first_chunk,
            } => {
                if path
                    .iter()
                    .any(|frame| logical::bit(key, frame.bit) != frame.visiting_right)
                    || (!previous.is_empty()
                        && (previous.as_slice() >= key
                            || logical::first_difference(&previous, key) != boundary))
                {
                    return Err(StorageError::IntegrityFailure);
                }
                reader.report.logical_bytes = reader
                    .report
                    .logical_bytes
                    .checked_add(key.len() as u64 + value.length)
                    .filter(|bytes| *bytes <= limits.maximum_logical_bytes)
                    .ok_or(StorageError::ResourceLimit)?;
                reader.report.entries += 1;
                previous.as_mut_slice().zeroize();
                previous.clear();
                previous.extend(key);
                drop(page);
                reader.value(value, first_chunk)?;
                current = None;
                while let Some(frame) = path.last_mut() {
                    if frame.visiting_right {
                        path.pop();
                    } else {
                        frame.visiting_right = true;
                        boundary = Some(frame.bit);
                        current = Some(frame.right);
                        break;
                    }
                }
            }
        }
    }
    if reader.report.entries != expected.entries()
        || reader.report.logical_bytes != expected.logical_bytes()
        || reader.report.nodes != expected.entries().saturating_mul(2).saturating_sub(1)
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(ValidatedPackedTree {
        context,
        commitment: expected,
        root,
        report: reader.report,
    })
}
