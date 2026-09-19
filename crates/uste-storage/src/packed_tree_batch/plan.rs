use super::*;
use crate::{
    ordered_commitment::{
        BranchProof, CommitmentError, CommitmentLimits, LeafProof, LookupProof, ValueCommitment,
    },
    packed_index_pack::read_linked_record_page,
    packed_index_page::ENCODED_PAGE_BYTES,
    packed_tree_record::TreeNode,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
struct Frame {
    original: Link,
    bit: u32,
    left: Link,
    right: Link,
    rightward: bool,
}
pub(super) const FRAME_BYTES: usize = size_of::<Frame>();
struct Leaf {
    key: Zeroizing<Vec<u8>>,
    value: ValueCommitment,
}
enum Inspected {
    Leaf(Leaf),
    Branch { bit: u32, left: Link, right: Link },
}

fn copy_key(key: &[u8]) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    let mut out = Zeroizing::new(Vec::new());
    out.try_reserve_exact(key.len())
        .map_err(|_| StorageError::ResourceLimit)?;
    out.extend(key);
    Ok(out)
}
fn logical_error(error: CommitmentError) -> StorageError {
    match error {
        CommitmentError::Conflict => StorageError::InvalidState,
        CommitmentError::ResourceLimit => StorageError::ResourceLimit,
        CommitmentError::InvalidProof => StorageError::IntegrityFailure,
    }
}

impl Plan<'_> {
    fn inspect<F: FileSystem, W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
        link: Link,
    ) -> Result<Inspected, StorageError> {
        match link.source {
            Source::Dirty(index) => match *self
                .nodes
                .get(index)
                .ok_or(StorageError::IntegrityFailure)?
            {
                DirtyNode::Leaf { delta, value } => {
                    let delta = &self.deltas[delta];
                    let claimed = logical::leaf_commitment(
                        self.logical_context,
                        LeafProof {
                            key: delta.key(),
                            value,
                        },
                    )
                    .map_err(logical_error)?;
                    if claimed != link.claimed {
                        return Err(StorageError::IntegrityFailure);
                    }
                    Ok(Inspected::Leaf(Leaf {
                        key: copy_key(delta.key())?,
                        value,
                    }))
                }
                DirtyNode::Branch { bit, left, right } => {
                    let claimed = logical::branch_commitment(
                        self.logical_context,
                        bit,
                        left.claimed,
                        right.claimed,
                    )
                    .map_err(logical_error)?;
                    if claimed != link.claimed {
                        return Err(StorageError::IntegrityFailure);
                    }
                    Ok(Inspected::Branch { bit, left, right })
                }
            },
            Source::Disk(location) => {
                if self.report.read_pages >= self.limits.maximum_read_pages
                    || ENCODED_PAGE_BYTES as u64
                        > self.limits.maximum_read_bytes - self.report.read_bytes
                {
                    return Err(StorageError::ResourceLimit);
                }
                let c = self.context;
                let physical = location.resolve(c.scope, c.profile, c.family, c.revision)?;
                let (page, report) = read_linked_record_page(
                    filesystem,
                    directory,
                    vault,
                    physical,
                    location.slot(),
                    ENCODED_PAGE_BYTES as u64,
                )?;
                self.report.read_pages += report.pages;
                self.report.read_bytes += report.encoded_bytes;
                let node = TreeNode::decode(
                    physical,
                    page.record(location.slot())
                        .ok_or(StorageError::IntegrityFailure)?,
                )?;
                if node.commitment(physical)? != link.claimed {
                    return Err(StorageError::IntegrityFailure);
                }
                match node {
                    TreeNode::Leaf { key, value, .. } => Ok(Inspected::Leaf(Leaf {
                        key: copy_key(key)?,
                        value,
                    })),
                    TreeNode::Branch { bit, left, right } => Ok(Inspected::Branch {
                        bit,
                        left: left.into(),
                        right: right.into(),
                    }),
                }
            }
        }
    }

    fn dirty(
        &mut self,
        original: Option<Link>,
        node: DirtyNode,
        claimed: OrderedCommitment,
    ) -> Result<Link, StorageError> {
        let index = match original.map(|link| link.source) {
            Some(Source::Dirty(index)) => {
                self.nodes[index] = node;
                index
            }
            _ => {
                if self.nodes.len() >= self.limits.maximum_dirty_nodes {
                    return Err(StorageError::ResourceLimit);
                }
                let index = self.nodes.len();
                self.nodes.push(node);
                self.report.dirty_nodes += 1;
                index
            }
        };
        Ok(Link {
            claimed,
            source: Source::Dirty(index),
        })
    }
    fn branch(
        &mut self,
        original: Option<Link>,
        bit: u32,
        left: Link,
        right: Link,
    ) -> Result<Link, StorageError> {
        let claimed =
            logical::branch_commitment(self.logical_context, bit, left.claimed, right.claimed)
                .map_err(logical_error)?;
        self.dirty(original, DirtyNode::Branch { bit, left, right }, claimed)
    }

    pub(super) fn apply<F: FileSystem, W, E: EntropySource>(
        &mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
        index: usize,
    ) -> Result<(), StorageError> {
        let delta = &self.deltas[index];
        let expected = self.root.map_or_else(
            || logical::empty_commitment(self.logical_context),
            |link| link.claimed,
        );
        let mut frames: Vec<Frame> = Vec::new();
        frames
            .try_reserve_exact(self.limits.maximum_path_branches as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut current = self.root;
        let mut terminal = None;
        while let Some(link) = current {
            match self.inspect(filesystem, directory, vault, link)? {
                Inspected::Leaf(leaf) => {
                    terminal = Some(leaf);
                    break;
                }
                Inspected::Branch { bit, left, right } => {
                    if frames.len() >= self.limits.maximum_path_branches as usize {
                        return Err(StorageError::ResourceLimit);
                    }
                    if frames.last().is_some_and(|frame| frame.bit >= bit) {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let rightward = logical::bit(delta.key(), bit);
                    let child = if rightward { right } else { left };
                    if child.claimed.entries() >= link.claimed.entries() {
                        return Err(StorageError::IntegrityFailure);
                    }
                    frames.push(Frame {
                        original: link,
                        bit,
                        left,
                        right,
                        rightward,
                    });
                    current = Some(child);
                }
            }
        }
        let mut proof = Vec::new();
        proof
            .try_reserve_exact(frames.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        proof.extend(frames.iter().map(|frame| BranchProof {
            bit: frame.bit,
            sibling: if frame.rightward {
                frame.left.claimed
            } else {
                frame.right.claimed
            },
        }));
        let logical_root = logical::apply_delta(
            self.logical_context,
            expected,
            delta.key(),
            delta.before(),
            delta.after(),
            LookupProof {
                leaf: terminal.as_ref().map(|leaf| LeafProof {
                    key: &leaf.key,
                    value: leaf.value,
                }),
                branches: &proof,
            },
            CommitmentLimits {
                maximum_branches: self.limits.maximum_path_branches,
                maximum_input_bytes: logical::MAX_PROOF_INPUT_BYTES,
            },
        )
        .map_err(logical_error)?;
        if delta.before() == delta.after() {
            return Ok(());
        }
        let present = terminal
            .as_ref()
            .is_some_and(|leaf| leaf.key.as_slice() == delta.key());
        let mut keep = frames.len();
        let mut updated = if let Some(value) = delta.after() {
            let value = logical::value_commitment(value).map_err(logical_error)?;
            let claimed = logical::leaf_commitment(
                self.logical_context,
                LeafProof {
                    key: delta.key(),
                    value,
                },
            )
            .map_err(logical_error)?;
            let new = self.dirty(
                None,
                DirtyNode::Leaf {
                    delta: index,
                    value,
                },
                claimed,
            )?;
            if present || terminal.is_none() {
                Some(new)
            } else {
                let split = logical::first_difference(
                    delta.key(),
                    &terminal.as_ref().ok_or(StorageError::IntegrityFailure)?.key,
                )
                .ok_or(StorageError::IntegrityFailure)?;
                keep = frames.partition_point(|frame| frame.bit < split);
                let old = frames
                    .get(keep)
                    .map(|frame| frame.original)
                    .or(current)
                    .ok_or(StorageError::IntegrityFailure)?;
                let (left, right) = if logical::bit(delta.key(), split) {
                    (old, new)
                } else {
                    (new, old)
                };
                Some(self.branch(None, split, left, right)?)
            }
        } else {
            if !present {
                return Err(StorageError::IntegrityFailure);
            }
            match frames.last() {
                None => None,
                Some(last) => {
                    keep -= 1;
                    Some(if last.rightward {
                        last.left
                    } else {
                        last.right
                    })
                }
            }
        };
        for frame in frames[..keep].iter().rev() {
            let child = updated.ok_or(StorageError::IntegrityFailure)?;
            let (left, right) = if frame.rightward {
                (frame.left, child)
            } else {
                (child, frame.right)
            };
            updated = Some(self.branch(Some(frame.original), frame.bit, left, right)?);
        }
        let actual = updated.map_or_else(
            || logical::empty_commitment(self.logical_context),
            |link| link.claimed,
        );
        if actual != logical_root {
            return Err(StorageError::IntegrityFailure);
        }
        self.root = updated;
        Ok(())
    }
}
