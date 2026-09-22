//! Opt-in maintenance lookup against an independently trusted canonical logical root.
//! This module does not authorize callers or admit a root against journal authority.
use crate::{
    FileSystem,
    journal::StorageError,
    ordered_commitment::{
        self as logical, BranchProof, CommitmentContext, CommitmentLimits, LeafProof,
        OrderedCommitment, ValueCommitment,
    },
    packed_index_pack::read_linked_record_page,
    packed_index_page::ENCODED_PAGE_BYTES,
    packed_page_cache::PackedPageCache,
    packed_tree_record::{MAX_CHUNK_DATA, PackedLocator, TreeNode, ValueChunk},
};
use uste_crypto::{EntropySource, KeyVault};
use uste_types::{CommitRevision, NamespaceRef};
use zeroize::Zeroizing;
mod read_page;
pub(crate) use read_page::ReadPage;

pub const MAX_LOOKUP_PAGES: u64 =
    logical::MAX_BRANCH_BITS as u64 + 1 + logical::MAX_VALUE_BYTES.div_ceil(MAX_CHUNK_DATA) as u64;
pub const MAX_LOOKUP_ENCODED_BYTES: u64 = MAX_LOOKUP_PAGES * ENCODED_PAGE_BYTES as u64;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub struct TreeReadContext {
    pub scope: NamespaceRef,
    pub profile: [u8; 32],
    pub family: u8,
    pub revision: CommitRevision,
}
#[derive(Clone, Copy)]
pub struct TreeLookupLimits {
    pub maximum_path_branches: u32,
    pub maximum_pages: u64,
    pub maximum_encoded_bytes: u64,
    pub maximum_value_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeLookupReport {
    /// Successful page proof-work units, including cache hits; not physical I/O.
    pub pages: u64,
    /// Encoded-byte proof-work units, including cache hits.
    pub encoded_bytes: u64,
    pub path_branches: u32,
    pub value_chunks: u32,
}
pub struct PackedLookupValue(Zeroizing<Vec<u8>>);
impl PackedLookupValue {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub(crate) fn into_vec(mut self) -> Vec<u8> {
        core::mem::take(&mut *self.0)
    }
    pub(crate) fn from_owned(bytes: Zeroizing<Vec<u8>>) -> Self {
        Self(bytes)
    }
}
impl core::fmt::Debug for PackedLookupValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PackedLookupValue([REDACTED])")
    }
}
pub struct TreeLookupResult {
    pub value: Option<PackedLookupValue>,
    pub report: TreeLookupReport,
}
pub struct MappedTreeLookupResult<T> {
    pub value: Option<T>,
    pub report: TreeLookupReport,
}

pub(crate) struct Reader<'a, F: FileSystem, W, E: EntropySource> {
    pub(crate) filesystem: &'a mut F,
    pub(crate) directory: &'a F::Directory,
    pub(crate) vault: &'a KeyVault<W, E>,
    pub(crate) context: TreeReadContext,
    pub(crate) limits: TreeLookupLimits,
    pub(crate) report: TreeLookupReport,
    pub(crate) cache: Option<&'a mut PackedPageCache>,
}
impl<F: FileSystem, W, E: EntropySource> Reader<'_, F, W, E> {
    pub(crate) fn page(&mut self, location: PackedLocator) -> Result<ReadPage, StorageError> {
        if self.report.pages >= self.limits.maximum_pages
            || ENCODED_PAGE_BYTES as u64
                > self.limits.maximum_encoded_bytes - self.report.encoded_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        let c = self.context;
        let physical = location.resolve(c.scope, c.profile, c.family, c.revision)?;
        if let Some(cache) = self.cache.as_deref_mut() {
            let page = cache.page(
                self.filesystem,
                self.directory,
                self.vault,
                physical,
                location.slot(),
            )?;
            self.report.pages += 1;
            self.report.encoded_bytes += ENCODED_PAGE_BYTES as u64;
            return Ok(ReadPage::Cached(page));
        }
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
        Ok(ReadPage::Owned(page))
    }

    pub(crate) fn value(
        &mut self,
        expected: ValueCommitment,
        mut next: Option<PackedLocator>,
    ) -> Result<PackedLookupValue, StorageError> {
        if expected.length > self.limits.maximum_value_bytes {
            return Err(StorageError::ResourceLimit);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        bytes
            .try_reserve_exact(expected.length as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
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
            bytes.extend(chunk.data);
            remaining -= chunk.data.len() as u64;
            next = chunk.next;
            self.report.value_chunks += 1;
        }
        if remaining != 0
            || logical::value_commitment(&bytes).map_err(|_| StorageError::IntegrityFailure)?
                != expected
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(PackedLookupValue(bytes))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn lookup<F: FileSystem, W, E: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    key: &[u8],
    limits: TreeLookupLimits,
) -> Result<TreeLookupResult, StorageError> {
    lookup_inner(
        filesystem, directory, vault, context, expected, root, key, limits, None, true,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lookup_cached<F: FileSystem, W, E: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    key: &[u8],
    limits: TreeLookupLimits,
    cache: &mut PackedPageCache,
) -> Result<TreeLookupResult, StorageError> {
    lookup_inner(
        filesystem,
        directory,
        vault,
        context,
        expected,
        root,
        key,
        limits,
        Some(cache),
        true,
    )
}

/// Cached lookup whose successful value is converted while its resident plaintext is borrowed.
/// The mapper's result cannot borrow from the cache. Proof-work, key/session checks and cache
/// admission are identical to [`lookup_cached`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn lookup_cached_with<F: FileSystem, W, E: EntropySource, R>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    key: &[u8],
    limits: TreeLookupLimits,
    cache: &mut PackedPageCache,
    map: impl FnOnce(&[u8]) -> R,
) -> Result<MappedTreeLookupResult<R>, StorageError> {
    validate_request(context, key, limits)?;
    CommitmentContext::new(context.scope, context.profile, context.family)
        .map_err(|_| StorageError::InvalidState)?;
    let mut map = Some(map);
    if let Some(location) = root
        && expected.entries() != 0
        && cache.has_lookup_cache()
    {
        let identity = crate::packed_page_cache::LookupIdentity::new(context, location, expected);
        if let Some((value, report)) =
            cache.lookup_get_with(vault, identity, key, limits, &mut map)?
        {
            return Ok(MappedTreeLookupResult {
                value: Some(value),
                report,
            });
        }
    }
    let result = lookup_inner(
        filesystem,
        directory,
        vault,
        context,
        expected,
        root,
        key,
        limits,
        Some(cache),
        false,
    )?;
    let map = map.ok_or(StorageError::InvalidState)?;
    Ok(MappedTreeLookupResult {
        value: result.value.map(|value| map(value.as_slice())),
        report: result.report,
    })
}

#[allow(clippy::too_many_arguments)]
fn lookup_inner<F: FileSystem, W, E: EntropySource>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    context: TreeReadContext,
    expected: OrderedCommitment,
    root: Option<PackedLocator>,
    key: &[u8],
    limits: TreeLookupLimits,
    cache: Option<&mut PackedPageCache>,
    probe_lookup_cache: bool,
) -> Result<TreeLookupResult, StorageError> {
    validate_request(context, key, limits)?;
    let logical_context = CommitmentContext::new(context.scope, context.profile, context.family)
        .map_err(|_| StorageError::InvalidState)?;
    let Some(mut location) = root else {
        if expected != logical::empty_commitment(logical_context) {
            return Err(StorageError::IntegrityFailure);
        }
        return Ok(TreeLookupResult {
            value: None,
            report: TreeLookupReport::default(),
        });
    };
    if expected.entries() == 0 {
        return Err(StorageError::IntegrityFailure);
    }
    let identity = cache
        .as_ref()
        .filter(|cache| cache.has_lookup_cache())
        .map(|_| crate::packed_page_cache::LookupIdentity::new(context, location, expected));
    let mut cache = cache;
    if probe_lookup_cache
        && let (Some(cache), Some(identity)) = (cache.as_deref_mut(), identity)
        && let Some((bytes, report)) = cache.lookup_get(vault, identity, key, limits)?
    {
        return Ok(TreeLookupResult {
            value: Some(PackedLookupValue(bytes)),
            report,
        });
    }
    let mut reader = Reader {
        filesystem,
        directory,
        vault,
        context,
        limits,
        report: TreeLookupReport::default(),
        cache,
    };
    let mut path: Vec<BranchProof> = Vec::new();
    path.try_reserve_exact(limits.maximum_path_branches as usize)
        .map_err(|_| StorageError::ResourceLimit)?;
    let mut claimed = expected;
    loop {
        let page = reader.page(location)?;
        let physical = location.resolve(
            context.scope,
            context.profile,
            context.family,
            context.revision,
        )?;
        let (node, commitment) = TreeNode::decode_committed(
            physical,
            page.record(location.slot())
                .ok_or(StorageError::IntegrityFailure)?,
        )?;
        if commitment != claimed {
            return Err(StorageError::IntegrityFailure);
        }
        match node {
            TreeNode::Branch { bit, left, right } => {
                if path.len() >= limits.maximum_path_branches as usize {
                    return Err(StorageError::ResourceLimit);
                }
                if path.last().is_some_and(|previous| previous.bit >= bit) {
                    return Err(StorageError::IntegrityFailure);
                }
                let (selected, sibling) = if logical::bit(key, bit) {
                    (right, left)
                } else {
                    (left, right)
                };
                if selected.claimed.entries() >= claimed.entries() {
                    return Err(StorageError::IntegrityFailure);
                }
                path.push(BranchProof {
                    bit,
                    sibling: sibling.claimed,
                });
                reader.report.path_branches += 1;
                location = selected.location;
                claimed = selected.claimed;
            }
            TreeNode::Leaf {
                key: leaf_key,
                value,
                first_chunk,
            } => {
                // Each node above matched the trusted root or its authenticated
                // parent's selected claim. Thus the terminal leaf is already
                // cryptographically bound to that root. Preserve admission and
                // leaf routing checks without rehashing the same complete path.
                logical::validate_lookup_route(
                    key,
                    LeafProof {
                        key: leaf_key,
                        value,
                    },
                    &path,
                    CommitmentLimits {
                        maximum_branches: limits.maximum_path_branches,
                        maximum_input_bytes: logical::MAX_PROOF_INPUT_BYTES,
                    },
                )
                .map_err(|_| StorageError::IntegrityFailure)?;
                let found = (leaf_key == key).then_some(value);
                // Keep the complete hash-fold verifier as a test-only cross-check
                // for every successful packed unit-test traversal.
                #[cfg(test)]
                assert!(
                    logical::verify_lookup(
                        logical_context,
                        expected,
                        key,
                        logical::LookupProof {
                            leaf: Some(LeafProof {
                                key: leaf_key,
                                value
                            }),
                            branches: &path,
                        },
                        CommitmentLimits {
                            maximum_branches: limits.maximum_path_branches,
                            maximum_input_bytes: logical::MAX_PROOF_INPUT_BYTES,
                        },
                    ) == Ok(found)
                );
                drop(page);
                drop(path);
                let value = match found {
                    None => None,
                    Some(value) => Some(reader.value(value, first_chunk)?),
                };
                if let (Some(cache), Some(value), Some(identity)) =
                    (reader.cache.as_deref_mut(), value.as_ref(), identity)
                {
                    cache.lookup_insert(vault, identity, key, value.as_slice(), reader.report)?;
                }
                return Ok(TreeLookupResult {
                    value,
                    report: reader.report,
                });
            }
        }
    }
}

fn validate_request(
    context: TreeReadContext,
    key: &[u8],
    limits: TreeLookupLimits,
) -> Result<(), StorageError> {
    if key.is_empty() || context.family == 0 {
        return Err(StorageError::InvalidState);
    }
    if key.len() > logical::MAX_KEY_BYTES
        || limits.maximum_path_branches > logical::MAX_BRANCH_BITS
        || limits.maximum_pages > MAX_LOOKUP_PAGES
        || limits.maximum_encoded_bytes > MAX_LOOKUP_ENCODED_BYTES
        || limits.maximum_value_bytes > logical::MAX_VALUE_BYTES as u64
    {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}
