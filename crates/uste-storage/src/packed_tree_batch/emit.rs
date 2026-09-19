use super::*;
use crate::{
    packed_index_pack::ImmutablePackWriter,
    packed_index_page::PackedRecordKind,
    packed_tree_record::{MAX_CHUNK_DATA, TreeNode, ValueChunk},
};

fn resolve(
    link: Link,
    locations: &[Option<ChildReference>],
) -> Result<ChildReference, StorageError> {
    match link.source {
        Source::Disk(location) => Ok(ChildReference {
            location,
            claimed: link.claimed,
        }),
        Source::Dirty(index) => {
            let reference = locations
                .get(index)
                .and_then(|value| *value)
                .ok_or(StorageError::IntegrityFailure)?;
            if reference.claimed != link.claimed {
                return Err(StorageError::IntegrityFailure);
            }
            Ok(reference)
        }
    }
}

impl Plan<'_> {
    fn reachable_order(&self, root: usize) -> Result<Vec<usize>, StorageError> {
        let mut order = Vec::new();
        order
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(self.nodes.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        colors.resize(self.nodes.len(), 0_u8);
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(self.nodes.len() * 2)
            .map_err(|_| StorageError::ResourceLimit)?;
        stack.push((root, false));
        while let Some((index, expanded)) = stack.pop() {
            let node = self
                .nodes
                .get(index)
                .ok_or(StorageError::IntegrityFailure)?;
            if expanded {
                if colors[index] != 1 {
                    return Err(StorageError::IntegrityFailure);
                }
                colors[index] = 2;
                order.push(index);
                continue;
            }
            // A dirty node has one parent; refuse cycles or aliasing rather than looping.
            if colors[index] != 0 {
                return Err(StorageError::IntegrityFailure);
            }
            colors[index] = 1;
            stack.push((index, true));
            if let DirtyNode::Branch { left, right, .. } = node {
                for child in [right, left] {
                    if let Source::Dirty(child) = child.source {
                        if stack.len() >= self.nodes.len() * 2 {
                            return Err(StorageError::ResourceLimit);
                        }
                        stack.push((child, false));
                    }
                }
            }
        }
        Ok(order)
    }

    pub(super) fn emit<F: FileSystem, W, E: EntropySource, I: EntropySource>(
        mut self,
        filesystem: &mut F,
        directory: &F::Directory,
        vault: &mut KeyVault<W, E>,
        entropy: &mut I,
        write: PackedPageContext,
    ) -> Result<StagedTreeBatch, StorageError> {
        let logical_root = self.root.map_or_else(
            || logical::empty_commitment(self.logical_context),
            |link| link.claimed,
        );
        let mut pack = None;
        let root = match self.root {
            None => None,
            Some(
                link @ Link {
                    source: Source::Disk(_),
                    ..
                },
            ) => Some(resolve(link, &[])?),
            Some(
                link @ Link {
                    source: Source::Dirty(index),
                    ..
                },
            ) => {
                let order = self.reachable_order(index)?;
                let mut locations = Vec::new();
                locations
                    .try_reserve_exact(self.nodes.len())
                    .map_err(|_| StorageError::ResourceLimit)?;
                locations.resize(self.nodes.len(), None);
                let mut writer = ImmutablePackWriter::create(
                    filesystem,
                    directory,
                    write,
                    self.limits.pack,
                    entropy,
                )?;
                for index in order {
                    let node = match self.nodes[index] {
                        DirtyNode::Branch { bit, left, right } => TreeNode::Branch {
                            bit,
                            left: resolve(left, &locations)?,
                            right: resolve(right, &locations)?,
                        },
                        DirtyNode::Leaf { delta, value } => {
                            let delta = &self.deltas[delta];
                            let data = delta.after().ok_or(StorageError::IntegrityFailure)?;
                            let mut next = None;
                            for (chunk, bytes) in data.chunks(MAX_CHUNK_DATA).enumerate().rev() {
                                let encoded = ValueChunk {
                                    remaining: (data.len() - chunk * MAX_CHUNK_DATA) as u32,
                                    next,
                                    data: bytes,
                                }
                                .encode(writer.context())?;
                                let address = writer.append(
                                    filesystem,
                                    vault,
                                    PackedRecordKind::ValueChunk,
                                    &encoded,
                                )?;
                                next = Some(PackedLocator::from_address(address));
                                self.report.written_chunks += 1;
                            }
                            TreeNode::Leaf {
                                key: delta.key(),
                                value,
                                first_chunk: next,
                            }
                        }
                    };
                    let claimed = node.commitment(writer.context())?;
                    let encoded = node.encode(writer.context())?;
                    let address =
                        writer.append(filesystem, vault, PackedRecordKind::TreeNode, &encoded)?;
                    locations[index] = Some(ChildReference {
                        location: PackedLocator::from_address(address),
                        claimed,
                    });
                    self.report.written_nodes += 1;
                }
                let root = resolve(link, &locations)?;
                let finished = writer.finish(filesystem, vault)?;
                self.report.written_pages = finished.pages();
                self.report.written_payload_bytes = finished.payload_bytes();
                pack = Some(finished);
                Some(root)
            }
        };
        Ok(StagedTreeBatch {
            context: TreeReadContext {
                revision: write.creation_revision,
                ..self.context
            },
            logical_root,
            root,
            pack,
            report: self.report,
        })
    }
}
