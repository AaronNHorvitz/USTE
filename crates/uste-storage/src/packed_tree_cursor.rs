//! In-process range cursor over a caller-admitted canonical root; no authorization or publication.
use crate::{
    FileSystem, IndexScanEntry,
    journal::StorageError,
    ordered_commitment::{
        self as logical, BranchProof, CommitmentContext, CommitmentLimits, LeafProof, LookupProof,
        OrderedCommitment, ValueCommitment,
    },
    packed_index_page::ENCODED_PAGE_BYTES,
    packed_page_cache::PackedPageCache,
    packed_tree_lookup::{
        PackedLookupValue, Reader, TreeLookupLimits, TreeLookupReport, TreeReadContext,
    },
    packed_tree_record::{ChildReference, PackedLocator, TreeNode},
    packed_tree_validation::{MAX_VALIDATION_LOGICAL_BYTES, MAX_VALIDATION_PAGES},
};
use uste_crypto::{EntropySource, KeyVault};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_CURSOR_CANDIDATES: u64 = logical::MAX_ENTRIES + 2;
pub const MAX_CURSOR_PAGES: u64 = MAX_VALIDATION_PAGES + logical::MAX_BRANCH_BITS as u64 + 1;
pub const MAX_CURSOR_ENCODED_BYTES: u64 = MAX_CURSOR_PAGES * ENCODED_PAGE_BYTES as u64;
pub const MAX_CURSOR_METADATA_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct TreeCursorLimits {
    pub maximum_path_branches: u32,
    pub maximum_candidates: u64,
    pub maximum_returned_bytes: u64,
    pub maximum_pages: u64,
    pub maximum_encoded_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeCursorReport {
    /// Every reached leaf, including the initial seek probe and exclusive boundary witness.
    pub candidates: u64,
    pub returned_entries: u64,
    pub returned_bytes: u64,
    /// Successful page proof-work units, including cache hits; not physical I/O.
    pub pages: u64,
    /// Encoded-byte proof-work units, including cache hits.
    pub encoded_bytes: u64,
    /// Greatest authenticated branch depth reached by any candidate.
    pub path_branches: u32,
    pub value_chunks: u64,
}
pub struct PackedCursorEntry {
    key: Zeroizing<Vec<u8>>,
    value: PackedLookupValue,
}
impl PackedCursorEntry {
    pub fn key(&self) -> &[u8] {
        &self.key
    }
    pub fn value(&self) -> &[u8] {
        self.value.as_slice()
    }
    /// Transfer the already-owned cursor buffers without allocating or copying them again.
    pub fn into_scan_entry(mut self) -> IndexScanEntry {
        IndexScanEntry {
            key: core::mem::take(&mut *self.key),
            value: self.value.into_vec(),
        }
    }
}
impl core::fmt::Debug for PackedCursorEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PackedCursorEntry([REDACTED])")
    }
}
struct Frame {
    original: ChildReference,
    left: ChildReference,
    right: ChildReference,
    bit: u32,
    rightward: bool,
}
struct Leaf {
    key: Zeroizing<Vec<u8>>,
    value: ValueCommitment,
    first_chunk: Option<PackedLocator>,
}
pub struct PackedTreeCursor {
    context: TreeReadContext,
    logical_context: CommitmentContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    lower: Zeroizing<Vec<u8>>,
    upper: Option<Zeroizing<Vec<u8>>>,
    previous: Zeroizing<Vec<u8>>,
    path: Vec<Frame>,
    proof: Vec<BranchProof>,
    limits: TreeCursorLimits,
    report: TreeCursorReport,
    started: bool,
    done: bool,
    failed: bool,
    reverse: bool,
}
fn copy_key(key: &[u8]) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    let mut out = Zeroizing::new(Vec::new());
    out.try_reserve_exact(key.len())
        .map_err(|_| StorageError::ResourceLimit)?;
    out.extend(key);
    Ok(out)
}

fn metadata_allowance(branches: u32) -> Result<u64, StorageError> {
    (branches as u64)
        .checked_mul((size_of::<Frame>() + size_of::<BranchProof>()) as u64)
        .and_then(|bytes| bytes.checked_add(4 * logical::MAX_KEY_BYTES as u64 + 65_536))
        .filter(|bytes| *bytes <= MAX_CURSOR_METADATA_BYTES)
        .ok_or(StorageError::ResourceLimit)
}
impl PackedTreeCursor {
    pub fn new(
        context: TreeReadContext,
        expected: OrderedCommitment,
        root: Option<PackedLocator>,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<Self, StorageError> {
        Self::with_direction(context, expected, root, lower, upper, limits, false)
    }
    /// Descending traversal over `(lower, upper]`; absent upper means the greatest key.
    pub fn new_reverse(
        context: TreeReadContext,
        expected: OrderedCommitment,
        root: Option<PackedLocator>,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<Self, StorageError> {
        Self::with_direction(context, expected, root, lower, upper, limits, true)
    }
    fn with_direction(
        context: TreeReadContext,
        expected: OrderedCommitment,
        root: Option<PackedLocator>,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
    ) -> Result<Self, StorageError> {
        let logical_context =
            CommitmentContext::new(context.scope, context.profile, context.family)
                .map_err(|_| StorageError::InvalidState)?;
        if upper.is_some_and(|end| lower > end) {
            return Err(StorageError::InvalidState);
        }
        if lower.len() > logical::MAX_KEY_BYTES
            || upper.is_some_and(|end| end.len() > logical::MAX_KEY_BYTES)
            || limits.maximum_path_branches > logical::MAX_BRANCH_BITS
            || limits.maximum_candidates > MAX_CURSOR_CANDIDATES
            || limits.maximum_returned_bytes > MAX_VALIDATION_LOGICAL_BYTES
            || limits.maximum_pages > MAX_CURSOR_PAGES
            || limits.maximum_encoded_bytes > MAX_CURSOR_ENCODED_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
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
        metadata_allowance(limits.maximum_path_branches)?;
        let mut path = Vec::new();
        path.try_reserve_exact(limits.maximum_path_branches as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut proof = Vec::new();
        proof
            .try_reserve_exact(limits.maximum_path_branches as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut previous = Zeroizing::new(Vec::new());
        previous
            .try_reserve_exact(logical::MAX_KEY_BYTES)
            .map_err(|_| StorageError::ResourceLimit)?;
        Ok(Self {
            context,
            logical_context,
            expected,
            root,
            lower: copy_key(lower)?,
            upper: upper.map(copy_key).transpose()?,
            previous,
            path,
            proof,
            limits,
            report: TreeCursorReport::default(),
            started: false,
            done: root.is_none() || upper == Some(lower),
            failed: false,
            reverse,
        })
    }
    pub fn report(&self) -> TreeCursorReport {
        self.report
    }

    /// Errors permanently poison this cursor. End-of-range is sticky and does no further I/O.
    pub fn next<F: FileSystem, W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        self.next_inner(filesystem, directory, vault, None)
    }

    pub(crate) fn next_cached<F: FileSystem, W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
        cache: &mut PackedPageCache,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        self.next_inner(filesystem, directory, vault, Some(cache))
    }

    fn next_inner<F: FileSystem, W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
        cache: Option<&mut PackedPageCache>,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        if self.failed {
            return Err(StorageError::NeedsRecovery);
        }
        if self.done {
            return Ok(None);
        }
        let mut reader = Reader {
            filesystem,
            directory,
            vault,
            context: self.context,
            limits: TreeLookupLimits {
                maximum_path_branches: self.limits.maximum_path_branches,
                maximum_pages: self.limits.maximum_pages - self.report.pages,
                maximum_encoded_bytes: self.limits.maximum_encoded_bytes
                    - self.report.encoded_bytes,
                maximum_value_bytes: logical::MAX_VALUE_BYTES as u64,
            },
            report: TreeLookupReport::default(),
            cache,
        };
        let result = self.step(&mut reader);
        self.report.pages += reader.report.pages;
        self.report.encoded_bytes += reader.report.encoded_bytes;
        self.report.value_chunks += reader.report.value_chunks as u64;
        if result.is_err() {
            self.failed = true;
            self.path.clear();
            self.proof.clear();
            self.previous.as_mut_slice().zeroize();
            self.previous.clear();
        }
        result
    }

    fn descend<F: FileSystem, W, E: EntropySource>(
        &mut self,
        reader: &mut Reader<'_, F, W, E>,
        mut link: ChildReference,
        seek: bool,
    ) -> Result<Leaf, StorageError> {
        loop {
            if link.claimed.entries() == 1
                && self.report.candidates >= self.limits.maximum_candidates
            {
                return Err(StorageError::ResourceLimit);
            }
            let page = reader.page(link.location)?;
            let c = self.context;
            let physical = link
                .location
                .resolve(c.scope, c.profile, c.family, c.revision)?;
            let (node, commitment) = TreeNode::decode_committed(
                physical,
                page.record(link.location.slot())
                    .ok_or(StorageError::IntegrityFailure)?,
            )?;
            if commitment != link.claimed {
                return Err(StorageError::IntegrityFailure);
            }
            match node {
                TreeNode::Branch { bit, left, right } => {
                    if self.path.len() >= self.limits.maximum_path_branches as usize {
                        return Err(StorageError::ResourceLimit);
                    }
                    if self.path.last().is_some_and(|parent| parent.bit >= bit)
                        || left.claimed.entries() >= link.claimed.entries()
                        || right.claimed.entries() >= link.claimed.entries()
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let rightward = if seek {
                        logical::bit(self.seek_bound().ok_or(StorageError::InvalidState)?, bit)
                    } else {
                        self.reverse
                    };
                    self.path.push(Frame {
                        original: link,
                        left,
                        right,
                        bit,
                        rightward,
                    });
                    self.report.path_branches = self.report.path_branches.max(
                        self.path
                            .len()
                            .try_into()
                            .map_err(|_| StorageError::ResourceLimit)?,
                    );
                    link = if rightward { right } else { left };
                }
                TreeNode::Leaf {
                    key,
                    value,
                    first_chunk,
                } => {
                    if self.report.candidates >= self.limits.maximum_candidates {
                        return Err(StorageError::ResourceLimit);
                    }
                    self.report.candidates += 1;
                    if self
                        .path
                        .iter()
                        .any(|frame| logical::bit(key, frame.bit) != frame.rightward)
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                    self.proof.clear();
                    self.proof.extend(self.path.iter().map(|frame| BranchProof {
                        bit: frame.bit,
                        sibling: if frame.rightward {
                            frame.left.claimed
                        } else {
                            frame.right.claimed
                        },
                    }));
                    let found = logical::verify_lookup(
                        self.logical_context,
                        self.expected,
                        key,
                        LookupProof {
                            leaf: Some(LeafProof { key, value }),
                            branches: &self.proof,
                        },
                        CommitmentLimits {
                            maximum_branches: self.limits.maximum_path_branches,
                            maximum_input_bytes: logical::MAX_PROOF_INPUT_BYTES,
                        },
                    )
                    .map_err(|_| StorageError::IntegrityFailure)?;
                    if found != Some(value) {
                        return Err(StorageError::IntegrityFailure);
                    }
                    return Ok(Leaf {
                        key: copy_key(key)?,
                        value,
                        first_chunk,
                    });
                }
            }
        }
    }
    fn advance<F: FileSystem, W, E: EntropySource>(
        &mut self,
        reader: &mut Reader<'_, F, W, E>,
    ) -> Result<Option<Leaf>, StorageError> {
        while let Some(frame) = self.path.last_mut() {
            if frame.rightward != self.reverse {
                self.path.pop();
            } else {
                frame.rightward = !self.reverse;
                let next = if self.reverse {
                    frame.left
                } else {
                    frame.right
                };
                return self.descend(reader, next, false).map(Some);
            }
        }
        Ok(None)
    }
    fn seek_bound(&self) -> Option<&[u8]> {
        if self.reverse {
            self.upper.as_ref().map(|key| key.as_slice())
        } else {
            (!self.lower.is_empty()).then_some(self.lower.as_slice())
        }
    }
    fn seek<F: FileSystem, W, E: EntropySource>(
        &mut self,
        reader: &mut Reader<'_, F, W, E>,
    ) -> Result<Option<Leaf>, StorageError> {
        self.started = true;
        let root = ChildReference {
            location: self.root.ok_or(StorageError::IntegrityFailure)?,
            claimed: self.expected,
        };
        let leaf = self.descend(reader, root, self.seek_bound().is_some())?;
        let Some(bound) = self.seek_bound() else {
            return Ok(Some(leaf));
        };
        if leaf.key.as_slice() == bound {
            return Ok(Some(leaf));
        }
        let split =
            logical::first_difference(&leaf.key, bound).ok_or(StorageError::IntegrityFailure)?;
        let keep = self.path.partition_point(|frame| frame.bit < split);
        let skip = if self.reverse {
            leaf.key.as_slice() > bound
        } else {
            leaf.key.as_slice() < bound
        };
        if skip {
            self.path.truncate(keep);
            self.advance(reader)
        } else if let Some(frame) = self.path.get(keep) {
            let subtree = frame.original;
            self.path.truncate(keep);
            self.descend(reader, subtree, false).map(Some)
        } else {
            Ok(Some(leaf))
        }
    }
    fn step<F: FileSystem, W, E: EntropySource>(
        &mut self,
        reader: &mut Reader<'_, F, W, E>,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        let leaf = if self.started {
            self.advance(reader)?
        } else {
            self.seek(reader)?
        };
        let Some(leaf) = leaf else {
            self.done = true;
            return Ok(None);
        };
        let key = leaf.key.as_slice();
        let invalid = if self.reverse {
            self.upper.as_ref().is_some_and(|end| key > end.as_slice())
                || (!self.previous.is_empty() && key >= self.previous.as_slice())
        } else {
            key < self.lower.as_slice()
                || (!self.previous.is_empty() && key <= self.previous.as_slice())
        };
        if invalid {
            return Err(StorageError::IntegrityFailure);
        }
        let ended = if self.reverse {
            key <= self.lower.as_slice()
        } else {
            self.upper.as_ref().is_some_and(|end| key >= end.as_slice())
        };
        if ended {
            self.done = true;
            return Ok(None);
        }
        let returned = self
            .report
            .returned_bytes
            .checked_add(leaf.key.len() as u64 + leaf.value.length)
            .filter(|bytes| *bytes <= self.limits.maximum_returned_bytes)
            .ok_or(StorageError::ResourceLimit)?;
        let value = reader.value(leaf.value, leaf.first_chunk)?;
        self.previous.as_mut_slice().zeroize();
        self.previous.clear();
        self.previous.extend(&*leaf.key);
        self.report.returned_entries += 1;
        self.report.returned_bytes = returned;
        Ok(Some(PackedCursorEntry {
            key: leaf.key,
            value,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_tree_cursor_metadata_reservations_fit_hard_ceiling() {
        assert!(metadata_allowance(logical::MAX_BRANCH_BITS).unwrap() < MAX_CURSOR_METADATA_BYTES);
        assert_eq!(
            metadata_allowance(u32::MAX),
            Err(StorageError::ResourceLimit)
        );
    }
}
