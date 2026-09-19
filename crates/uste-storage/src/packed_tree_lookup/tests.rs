use super::*;
use crate::{
    AdapterErrorKind, EntryName,
    fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation},
    memory::MemoryFileSystem,
    packed_index_pack::{ImmutablePackWriter, PackWriteLimits},
    packed_index_page::{PackedPageBuilder, PackedPageContext, PackedRecordKind},
    packed_tree_record::ChildReference,
    read_exact_at, write_all_at,
};
use std::collections::BTreeMap;
use uste_crypto::{
    CryptoError, EntropyFailure, KeyAdapter, KeyEpoch, SecretKeyMaterial, WriterIncarnationId,
};
use uste_types::{DatabaseId, NamespaceId};

mod batch;
mod corruption;
mod cursor;
mod validation;

struct Entropy(u64);
impl EntropySource for Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        for (i, part) in out.chunks_mut(8).enumerate() {
            part.copy_from_slice(&(self.0 + i as u64).to_be_bytes()[..part.len()]);
        }
        Ok(())
    }
}
struct Adapter;
impl KeyAdapter for Adapter {
    type Envelope = [u8; 32];
    fn wrap(
        &mut self,
        _: DatabaseId,
        key: &SecretKeyMaterial,
        _: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(*key.expose_to_adapter())
    }
    fn unwrap(
        &mut self,
        _: DatabaseId,
        wrapped: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*wrapped))
    }
}
fn context() -> TreeReadContext {
    TreeReadContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        profile: [3; 32],
        family: 4,
        revision: CommitRevision::new(9).unwrap(),
    }
}
fn limits() -> TreeLookupLimits {
    TreeLookupLimits {
        maximum_path_branches: 64,
        maximum_pages: MAX_LOOKUP_PAGES,
        maximum_encoded_bytes: MAX_LOOKUP_ENCODED_BYTES,
        maximum_value_bytes: logical::MAX_VALUE_BYTES as u64,
    }
}
fn logical_context() -> CommitmentContext {
    let c = context();
    CommitmentContext::new(c.scope, c.profile, c.family).unwrap()
}
fn key_bits(key: &[u8]) -> Vec<bool> {
    let mut result = Vec::new();
    for byte in key {
        result.push(true);
        for shift in (0..8).rev() {
            result.push((byte >> shift) & 1 != 0);
        }
    }
    result.push(false);
    result
}

struct Fixture {
    fs: MemoryFileSystem,
    vault: KeyVault<[u8; 32], Entropy>,
    root: ChildReference,
    values: BTreeMap<Vec<u8>, Vec<u8>>,
    leaves: BTreeMap<Vec<u8>, PackedLocator>,
}

fn fixture(value_bytes: usize) -> Fixture {
    let c = context();
    let mut fs = MemoryFileSystem::default();
    let directory = fs.root();
    let mut vault = KeyVault::create(c.scope.database(), &mut Adapter, Entropy(10)).unwrap();
    let physical = PackedPageContext {
        scope: c.scope,
        profile: c.profile,
        family: c.family,
        creation_revision: c.revision,
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([5; 16]),
        object: [0; 16],
        page: 0,
    };
    let mut writer = ImmutablePackWriter::create(
        &mut fs,
        &directory,
        physical,
        PackWriteLimits {
            maximum_pages: 2048,
            maximum_records: 4096,
            maximum_payload_bytes: 32 * 1024 * 1024,
        },
        &mut Entropy(1000),
    )
    .unwrap();
    let mut values = BTreeMap::new();
    for key in [
        vec![0],
        vec![0, 0],
        vec![0, 255],
        b"a".to_vec(),
        b"ab".to_vec(),
        b"b".to_vec(),
        vec![255],
    ] {
        let value = if key == b"b" {
            (0..value_bytes).map(|i| (i % 251) as u8).collect()
        } else if key == [0] {
            Vec::new()
        } else {
            key.clone()
        };
        values.insert(key, value);
    }
    let mut leaves = BTreeMap::new();
    let root = build(
        &mut fs,
        &mut vault,
        &mut writer,
        &values.iter().collect::<Vec<_>>(),
        &mut leaves,
    );
    writer.finish(&mut fs, &mut vault).unwrap();
    fs.restart().unwrap();
    Fixture {
        fs,
        vault,
        root,
        values,
        leaves,
    }
}

// Independent sorted-key partitioning, not the production routing/first-difference helper.
fn build(
    fs: &mut MemoryFileSystem,
    vault: &mut KeyVault<[u8; 32], Entropy>,
    writer: &mut ImmutablePackWriter<MemoryFileSystem>,
    entries: &[(&Vec<u8>, &Vec<u8>)],
    leaves: &mut BTreeMap<Vec<u8>, PackedLocator>,
) -> ChildReference {
    let node = if entries.len() == 1 {
        let (key, value) = entries[0];
        let mut next = None;
        for (i, data) in value.chunks(MAX_CHUNK_DATA).enumerate().rev() {
            let chunk = ValueChunk {
                remaining: (value.len() - i * MAX_CHUNK_DATA) as u32,
                next,
                data,
            };
            let bytes = chunk.encode(writer.context()).unwrap();
            next = Some(PackedLocator::from_address(
                writer
                    .append(fs, vault, PackedRecordKind::ValueChunk, &bytes)
                    .unwrap(),
            ));
        }
        TreeNode::Leaf {
            key,
            value: logical::value_commitment(value).unwrap(),
            first_chunk: next,
        }
    } else {
        let a = key_bits(entries[0].0);
        let b = key_bits(entries[entries.len() - 1].0);
        let bit = (0..a.len().max(b.len()))
            .find(|i| a.get(*i).copied().unwrap_or(false) != b.get(*i).copied().unwrap_or(false))
            .unwrap();
        let at = entries
            .iter()
            .position(|(key, _)| key_bits(key).get(bit).copied().unwrap_or(false))
            .unwrap();
        assert!(at > 0);
        let left = build(fs, vault, writer, &entries[..at], leaves);
        let right = build(fs, vault, writer, &entries[at..], leaves);
        TreeNode::Branch {
            bit: bit as u32,
            left,
            right,
        }
    };
    let claimed = node.commitment(writer.context()).unwrap();
    let bytes = node.encode(writer.context()).unwrap();
    let location = PackedLocator::from_address(
        writer
            .append(fs, vault, PackedRecordKind::TreeNode, &bytes)
            .unwrap(),
    );
    if entries.len() == 1 {
        leaves.insert(entries[0].0.clone(), location);
    }
    ChildReference { location, claimed }
}

fn query<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    root: ChildReference,
    key: &[u8],
    limits: TreeLookupLimits,
) -> Result<TreeLookupResult, StorageError> {
    lookup(
        fs,
        &fs.root(),
        vault,
        context(),
        root.claimed,
        Some(root.location),
        key,
        limits,
    )
}

#[test]
fn packed_tree_lookup_matches_sorted_reference_membership_absence_empty_and_multichunk() {
    let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
    for key in f
        .values
        .keys()
        .chain([b"aa".to_vec(), vec![1], vec![255, 0]].iter())
    {
        let result = query(&mut f.fs, &f.vault, f.root, key, limits()).unwrap();
        assert_eq!(
            result.value.as_ref().map(PackedLookupValue::as_slice),
            f.values.get(key).map(Vec::as_slice)
        );
        assert_eq!(
            result.report.encoded_bytes,
            result.report.pages * ENCODED_PAGE_BYTES as u64
        );
        assert_eq!(
            result.report.pages,
            result.report.path_branches as u64 + 1 + result.report.value_chunks as u64
        );
        if key.as_slice() == b"b" {
            assert_eq!(result.report.value_chunks, 3);
        }
        if key == &[0] {
            assert_eq!(result.value.unwrap().as_slice(), b"");
        }
    }
    let mut fs = FaultFileSystem::new(f.fs, FaultPlan::default());
    let empty = logical::empty_commitment(logical_context());
    let root = fs.root();
    let result = lookup(
        &mut fs,
        &root,
        &f.vault,
        context(),
        empty,
        None,
        b"missing",
        limits(),
    )
    .unwrap();
    assert!(result.value.is_none());
    assert_eq!(result.report, TreeLookupReport::default());
    assert!(
        lookup(
            &mut fs,
            &root,
            &f.vault,
            context(),
            f.root.claimed,
            None,
            b"missing",
            limits()
        )
        .is_err()
    );
    assert!(
        lookup(
            &mut fs,
            &root,
            &f.vault,
            context(),
            empty,
            Some(f.root.location),
            b"missing",
            limits()
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
}

#[test]
fn packed_tree_lookup_exact_minus_one_and_hard_limits_never_return_partial_values() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut fs = FaultFileSystem::new(f.fs, FaultPlan::default());
    let result = query(&mut fs, &f.vault, f.root, b"b", limits()).unwrap();
    let exact = TreeLookupLimits {
        maximum_path_branches: result.report.path_branches,
        maximum_pages: result.report.pages,
        maximum_encoded_bytes: result.report.encoded_bytes,
        maximum_value_bytes: f.values[b"b".as_slice()].len() as u64,
    };
    assert!(query(&mut fs, &f.vault, f.root, b"b", exact).is_ok());
    for short in [
        TreeLookupLimits {
            maximum_path_branches: exact.maximum_path_branches - 1,
            ..exact
        },
        TreeLookupLimits {
            maximum_pages: exact.maximum_pages - 1,
            ..exact
        },
        TreeLookupLimits {
            maximum_encoded_bytes: exact.maximum_encoded_bytes - 1,
            ..exact
        },
        TreeLookupLimits {
            maximum_value_bytes: exact.maximum_value_bytes - 1,
            ..exact
        },
    ] {
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            query(&mut fs, &f.vault, f.root, b"b", short).err(),
            Some(StorageError::ResourceLimit)
        );
        if short.maximum_value_bytes < exact.maximum_value_bytes {
            assert_eq!(
                fs.operation_count(Operation::ReadAt),
                exact.maximum_path_branches as u64 + 1
            );
        }
    }
    for over in [
        TreeLookupLimits {
            maximum_path_branches: logical::MAX_BRANCH_BITS + 1,
            ..limits()
        },
        TreeLookupLimits {
            maximum_pages: MAX_LOOKUP_PAGES + 1,
            ..limits()
        },
        TreeLookupLimits {
            maximum_encoded_bytes: MAX_LOOKUP_ENCODED_BYTES + 1,
            ..limits()
        },
        TreeLookupLimits {
            maximum_value_bytes: logical::MAX_VALUE_BYTES as u64 + 1,
            ..limits()
        },
        TreeLookupLimits {
            maximum_pages: 0,
            ..limits()
        },
        TreeLookupLimits {
            maximum_encoded_bytes: ENCODED_PAGE_BYTES as u64 - 1,
            ..limits()
        },
    ] {
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            query(&mut fs, &f.vault, f.root, b"b", over).err(),
            Some(StorageError::ResourceLimit)
        );
        assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
    }
    fs.arm(FaultPlan::default()).unwrap();
    for key in [vec![], vec![1; logical::MAX_KEY_BYTES + 1]] {
        assert!(query(&mut fs, &f.vault, f.root, &key, limits()).is_err());
    }
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
}

#[test]
fn packed_tree_lookup_every_observed_read_error_or_crash_has_no_partial_success() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut baseline = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let report = query(&mut baseline, &f.vault, f.root, b"b", limits())
        .unwrap()
        .report;
    assert_eq!(
        report,
        TreeLookupReport {
            pages: 7,
            encoded_bytes: 143_815,
            path_branches: 3,
            value_chunks: 3,
        }
    );
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        assert_eq!(baseline.operation_count(operation), report.pages);
        for occurrence in 1..=baseline.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut fs = FaultFileSystem::new(f.fs.clone(), plan);
                assert!(query(&mut fs, &f.vault, f.root, b"b", limits()).is_err());
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                assert_eq!(
                    query(&mut fs, &f.vault, f.root, b"b", limits())
                        .unwrap()
                        .value
                        .unwrap()
                        .as_slice(),
                    &f.values[b"b".as_slice()]
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 63);
}

#[test]
fn packed_tree_lookup_maximum_value_is_exact_and_redacted() {
    let mut f = fixture(logical::MAX_VALUE_BYTES);
    let result = query(&mut f.fs, &f.vault, f.root, b"b", limits()).unwrap();
    assert_eq!(
        result.report.value_chunks as usize,
        logical::MAX_VALUE_BYTES.div_ceil(MAX_CHUNK_DATA)
    );
    let value = result.value.unwrap();
    assert_eq!(value.as_slice(), &f.values[b"b".as_slice()]);
    assert_eq!(format!("{value:?}"), "PackedLookupValue([REDACTED])");
}
