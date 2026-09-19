use super::*;
use crate::{
    IndexDelta,
    packed_tree_batch::{StagedTreeBatch, TreeBatchLimits, stage_batch},
};

fn batch_limits() -> TreeBatchLimits {
    TreeBatchLimits {
        maximum_deltas: 512,
        maximum_input_bytes: 64 * 1024 * 1024,
        maximum_dirty_nodes: 4096,
        maximum_path_branches: 128,
        maximum_read_pages: 10_000,
        maximum_read_bytes: 10_000 * ENCODED_PAGE_BYTES as u64,
        pack: PackWriteLimits {
            maximum_pages: 2048,
            maximum_records: 4096,
            maximum_payload_bytes: 64 * 1024 * 1024,
        },
    }
}
fn write_context(c: TreeReadContext) -> PackedPageContext {
    PackedPageContext {
        scope: c.scope,
        profile: c.profile,
        family: c.family,
        creation_revision: c.revision.checked_next().unwrap(),
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([5; 16]),
        object: [0; 16],
        page: 0,
    }
}
fn stage<F: FileSystem>(
    fs: &mut F,
    vault: &mut KeyVault<[u8; 32], Entropy>,
    c: TreeReadContext,
    root: Option<ChildReference>,
    deltas: &[IndexDelta],
    seed: u64,
    limits: TreeBatchLimits,
) -> Result<StagedTreeBatch, StorageError> {
    let expected = root.map_or_else(
        || logical::empty_commitment(CommitmentContext::new(c.scope, c.profile, c.family).unwrap()),
        |root| root.claimed,
    );
    stage_batch(
        fs,
        &fs.root(),
        vault,
        &mut Entropy(seed),
        c,
        expected,
        root.map(|root| root.location),
        deltas,
        write_context(c),
        limits,
    )
}
fn reference(c: TreeReadContext, map: &BTreeMap<Vec<u8>, Vec<u8>>) -> OrderedCommitment {
    fn sorted(c: CommitmentContext, items: &[(&Vec<u8>, &Vec<u8>)]) -> OrderedCommitment {
        if items.is_empty() {
            return logical::empty_commitment(c);
        }
        if items.len() == 1 {
            return logical::leaf_commitment(
                c,
                LeafProof {
                    key: items[0].0,
                    value: logical::value_commitment(items[0].1).unwrap(),
                },
            )
            .unwrap();
        }
        let first = key_bits(items[0].0);
        let last = key_bits(items[items.len() - 1].0);
        let bit = (0..first.len().max(last.len()))
            .find(|i| {
                first.get(*i).copied().unwrap_or(false) != last.get(*i).copied().unwrap_or(false)
            })
            .unwrap();
        let at = items
            .iter()
            .position(|(key, _)| key_bits(key).get(bit).copied().unwrap_or(false))
            .unwrap();
        logical::branch_commitment(
            c,
            bit as u32,
            sorted(c, &items[..at]),
            sorted(c, &items[at..]),
        )
        .unwrap()
    }
    sorted(
        CommitmentContext::new(c.scope, c.profile, c.family).unwrap(),
        &map.iter().collect::<Vec<_>>(),
    )
}
fn check<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    staged: &StagedTreeBatch,
    values: &BTreeMap<Vec<u8>, Vec<u8>>,
) {
    assert_eq!(staged.logical_root(), reference(staged.context(), values));
    for key in values.keys().chain([b"missing".to_vec()].iter()) {
        let result = lookup(
            fs,
            &fs.root(),
            vault,
            staged.context(),
            staged.logical_root(),
            staged.root().map(|root| root.location),
            key,
            limits(),
        )
        .unwrap();
        assert_eq!(
            result.value.as_ref().map(PackedLookupValue::as_slice),
            values.get(key).map(Vec::as_slice)
        );
    }
}

#[test]
fn packed_tree_batch_generated_histories_match_reference_and_preserve_prior_roots() {
    let mut f = fixture(7);
    let old = f.root;
    let old_values = f.values.clone();
    let mut current = Some(f.root);
    let mut c = context();
    let mut seed = 17_u64;
    for batch in 0..32 {
        let mut changes = BTreeMap::new();
        for _ in 0..8 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let key = vec![(seed >> 32) as u8 % 64, (seed >> 40) as u8 % 3];
            let before = f.values.get(&key).cloned();
            let after = if before.is_some() && seed & 3 == 0 {
                None
            } else {
                Some(vec![
                    batch as u8;
                    if batch % 11 == 0 {
                        MAX_CHUNK_DATA + 1
                    } else {
                        3
                    }
                ])
            };
            changes.insert(key.clone(), IndexDelta::new(key, before, after).unwrap());
        }
        let deltas: Vec<_> = changes.into_values().collect();
        let staged = stage(
            &mut f.fs,
            &mut f.vault,
            c,
            current,
            &deltas,
            2000 + batch,
            batch_limits(),
        )
        .unwrap();
        for delta in &deltas {
            match delta.after() {
                Some(value) => {
                    f.values.insert(delta.key().to_vec(), value.to_vec());
                }
                None => {
                    f.values.remove(delta.key());
                }
            }
        }
        f.fs.restart().unwrap();
        check(&mut f.fs, &f.vault, &staged, &f.values);
        assert!(staged.report().written_nodes <= staged.report().dirty_nodes);
        current = staged.root();
        c = staged.context();
    }
    for (key, value) in &old_values {
        assert_eq!(
            query(&mut f.fs, &f.vault, old, key, limits())
                .unwrap()
                .value
                .unwrap()
                .as_slice(),
            value
        );
    }
}

#[test]
fn packed_tree_batch_bootstrap_subtree_reuse_same_value_and_empty_result() {
    let mut f = fixture(7);
    let deltas: Vec<_> = (0..128_u8)
        .map(|key| IndexDelta::new(vec![key], None, Some(vec![key])).unwrap())
        .collect();
    let staged = stage(
        &mut f.fs,
        &mut f.vault,
        context(),
        None,
        &deltas,
        3000,
        batch_limits(),
    )
    .unwrap();
    let values: BTreeMap<_, _> = (0..128_u8).map(|key| (vec![key], vec![key])).collect();
    check(&mut f.fs, &f.vault, &staged, &values);
    assert_eq!(staged.report().written_nodes, 255);
    assert_eq!(staged.report().written_chunks, 128);
    assert_eq!(staged.report().read_pages, 0);
    let c = staged.context();
    let root = staged.root();
    let before = values[&vec![64]].clone();
    let same = [IndexDelta::new(vec![64], Some(before.clone()), Some(before.clone())).unwrap()];
    let identical = stage(
        &mut f.fs,
        &mut f.vault,
        c,
        root,
        &same,
        3001,
        batch_limits(),
    )
    .unwrap();
    assert!(identical.pack().is_none());
    assert_eq!(identical.report().written_nodes, 0);
    assert!(identical.root().unwrap().location == root.unwrap().location);
    let changed = [IndexDelta::new(vec![64], Some(before), Some(b"replacement".to_vec())).unwrap()];
    let changed = stage(
        &mut f.fs,
        &mut f.vault,
        c,
        root,
        &changed,
        3002,
        batch_limits(),
    )
    .unwrap();
    assert_eq!(changed.report().written_nodes, 8); // Seven ancestors and exactly one leaf.
    assert_eq!(changed.report().written_chunks, 1);
    let mut modified = values.clone();
    modified.insert(vec![64], b"replacement".to_vec());
    check(&mut f.fs, &f.vault, &changed, &modified);
    let deletes: Vec<_> = modified
        .iter()
        .map(|(key, value)| IndexDelta::new(key.clone(), Some(value.clone()), None).unwrap())
        .collect();
    let empty = stage(
        &mut f.fs,
        &mut f.vault,
        changed.context(),
        changed.root(),
        &deletes,
        3003,
        batch_limits(),
    )
    .unwrap();
    assert!(empty.root().is_none());
    assert!(empty.pack().is_none());
    assert_eq!(empty.report().written_nodes, 0);
    assert!(empty.report().dirty_nodes > 0); // Intermediate dirty ancestors are not serialized.
    check(&mut f.fs, &f.vault, &empty, &BTreeMap::new());
    check(&mut f.fs, &f.vault, &staged, &values);
}

#[test]
fn packed_tree_batch_late_conflicts_unsorted_duplicates_and_limits_never_start_output() {
    let f = fixture(7);
    let mut fs = FaultFileSystem::new(f.fs, FaultPlan::default());
    let mut vault = f.vault;
    let first = IndexDelta::new(b"a".to_vec(), Some(b"a".to_vec()), Some(b"new".to_vec())).unwrap();
    let late = IndexDelta::new(
        b"b".to_vec(),
        Some(b"wrong".to_vec()),
        Some(b"new".to_vec()),
    )
    .unwrap();
    assert_eq!(
        stage(
            &mut fs,
            &mut vault,
            context(),
            Some(f.root),
            &[first.clone(), late],
            4000,
            batch_limits()
        )
        .err(),
        Some(StorageError::InvalidState)
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    for deltas in [
        vec![first.clone(), first.clone()],
        vec![
            IndexDelta::new(b"z".to_vec(), None, Some(b"z".to_vec())).unwrap(),
            first.clone(),
        ],
    ] {
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            stage(
                &mut fs,
                &mut vault,
                context(),
                Some(f.root),
                &deltas,
                4000,
                batch_limits()
            )
            .err(),
            Some(StorageError::InvalidState)
        );
        assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
    let result = stage(
        &mut fs,
        &mut vault,
        context(),
        Some(f.root),
        std::slice::from_ref(&first),
        4000,
        batch_limits(),
    )
    .unwrap();
    let report = result.report();
    let exact = TreeBatchLimits {
        maximum_deltas: 1,
        maximum_input_bytes: report.input_bytes,
        maximum_dirty_nodes: report.dirty_nodes as usize,
        maximum_read_pages: report.read_pages,
        maximum_read_bytes: report.read_bytes,
        pack: PackWriteLimits {
            maximum_pages: report.written_pages,
            maximum_records: report.written_nodes + report.written_chunks,
            maximum_payload_bytes: report.written_payload_bytes,
        },
        ..batch_limits()
    };
    stage(
        &mut fs,
        &mut vault,
        context(),
        Some(f.root),
        std::slice::from_ref(&first),
        4001,
        exact,
    )
    .unwrap();
    for short in [
        TreeBatchLimits {
            maximum_deltas: 0,
            ..exact
        },
        TreeBatchLimits {
            maximum_input_bytes: exact.maximum_input_bytes - 1,
            ..exact
        },
        TreeBatchLimits {
            maximum_dirty_nodes: exact.maximum_dirty_nodes - 1,
            ..exact
        },
        TreeBatchLimits {
            maximum_read_pages: exact.maximum_read_pages - 1,
            ..exact
        },
        TreeBatchLimits {
            maximum_read_bytes: exact.maximum_read_bytes - 1,
            ..exact
        },
        TreeBatchLimits {
            maximum_path_branches: 0,
            ..exact
        },
    ] {
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            stage(
                &mut fs,
                &mut vault,
                context(),
                Some(f.root),
                std::slice::from_ref(&first),
                4002,
                short
            )
            .err(),
            Some(StorageError::ResourceLimit)
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
}

#[test]
fn packed_tree_batch_every_observed_io_error_and_crash_preserves_the_base() {
    let f = fixture(7);
    let mut vault = f.vault;
    let changes = [
        IndexDelta::new(
            b"a".to_vec(),
            Some(b"a".to_vec()),
            Some(vec![9; 2 * MAX_CHUNK_DATA + 7]),
        )
        .unwrap(),
        IndexDelta::new(b"ab".to_vec(), Some(b"ab".to_vec()), None).unwrap(),
        IndexDelta::new(b"z".to_vec(), None, Some(b"new".to_vec())).unwrap(),
    ];
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let observed_stage = stage(
        &mut observed,
        &mut vault,
        context(),
        Some(f.root),
        &changes,
        5000,
        batch_limits(),
    )
    .unwrap();
    assert_eq!(observed_stage.report().read_pages, 7);
    assert_eq!(observed_stage.report().written_pages, 4);
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        for occurrence in 1..=observed.operation_count(operation) {
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
                assert!(
                    stage(
                        &mut fs,
                        &mut vault,
                        context(),
                        Some(f.root),
                        &changes,
                        5000,
                        batch_limits()
                    )
                    .is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                for (key, value) in &f.values {
                    assert_eq!(
                        query(&mut fs, &vault, f.root, key, limits())
                            .unwrap()
                            .value
                            .unwrap()
                            .as_slice(),
                        value
                    );
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 87);
}

#[test]
fn packed_tree_batch_output_limits_and_late_authenticated_corruption_preserve_no_prefix() {
    let f = fixture(7);
    let mut vault = f.vault;
    let changes = [IndexDelta::new(
        b"a".to_vec(),
        Some(b"a".to_vec()),
        Some(vec![9; 2 * MAX_CHUNK_DATA + 7]),
    )
    .unwrap()];
    let mut observed = f.fs.clone();
    let result = stage(
        &mut observed,
        &mut vault,
        context(),
        Some(f.root),
        &changes,
        6000,
        batch_limits(),
    )
    .unwrap();
    let report = result.report();
    assert!(report.written_pages >= 3);
    for pack in [
        PackWriteLimits {
            maximum_pages: report.written_pages - 1,
            ..batch_limits().pack
        },
        PackWriteLimits {
            maximum_records: report.written_nodes + report.written_chunks - 1,
            ..batch_limits().pack
        },
        PackWriteLimits {
            maximum_payload_bytes: report.written_payload_bytes - 1,
            ..batch_limits().pack
        },
    ] {
        let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
        assert_eq!(
            stage(
                &mut fs,
                &mut vault,
                context(),
                Some(f.root),
                &changes,
                6000,
                TreeBatchLimits {
                    pack,
                    ..batch_limits()
                }
            )
            .err(),
            Some(StorageError::ResourceLimit)
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 1);
        assert_eq!(fs.operation_count(Operation::SyncAll), 0);
        fs.restart().unwrap();
        for (key, value) in &f.values {
            assert_eq!(
                query(&mut fs, &vault, f.root, key, limits())
                    .unwrap()
                    .value
                    .unwrap()
                    .as_slice(),
                value
            );
        }
    }
    let mut f = fixture(7);
    let leaf = f.leaves[b"b".as_slice()];
    corruption::rewrite(&mut f, leaf, |record, owner| {
        let TreeNode::Leaf {
            key,
            mut value,
            first_chunk,
        } = TreeNode::decode(owner, record).unwrap()
        else {
            panic!("leaf")
        };
        value.digest[0] ^= 1;
        TreeNode::Leaf {
            key,
            value,
            first_chunk,
        }
        .encode(owner)
        .unwrap()
        .to_vec()
    });
    let changes = [
        IndexDelta::new(b"a".to_vec(), Some(b"a".to_vec()), Some(b"new-a".to_vec())).unwrap(),
        IndexDelta::new(
            b"b".to_vec(),
            Some(f.values[b"b".as_slice()].clone()),
            Some(b"new-b".to_vec()),
        )
        .unwrap(),
    ];
    let mut fs = FaultFileSystem::new(f.fs, FaultPlan::default());
    assert_eq!(
        stage(
            &mut fs,
            &mut f.vault,
            context(),
            Some(f.root),
            &changes,
            6000,
            batch_limits()
        )
        .err(),
        Some(StorageError::IntegrityFailure)
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(
        query(&mut fs, &f.vault, f.root, b"a", limits())
            .unwrap()
            .value
            .unwrap()
            .as_slice(),
        b"a"
    );
}

#[test]
fn packed_tree_batch_hard_bounds_context_and_empty_request_are_explicit() {
    use crate::packed_tree_batch::{
        MAX_BATCH_DELTAS, MAX_BATCH_DIRTY_NODES, MAX_BATCH_INPUT_BYTES, MAX_BATCH_READ_BYTES,
        MAX_BATCH_READ_PAGES,
    };
    let f = fixture(7);
    let mut fs = FaultFileSystem::new(f.fs, FaultPlan::default());
    let mut vault = f.vault;
    let c = context();
    let root = fs.root();
    for over in [
        TreeBatchLimits {
            maximum_deltas: MAX_BATCH_DELTAS + 1,
            ..batch_limits()
        },
        TreeBatchLimits {
            maximum_dirty_nodes: MAX_BATCH_DIRTY_NODES + 1,
            ..batch_limits()
        },
        TreeBatchLimits {
            maximum_input_bytes: MAX_BATCH_INPUT_BYTES + 1,
            ..batch_limits()
        },
        TreeBatchLimits {
            maximum_path_branches: logical::MAX_BRANCH_BITS + 1,
            ..batch_limits()
        },
        TreeBatchLimits {
            maximum_read_pages: MAX_BATCH_READ_PAGES + 1,
            ..batch_limits()
        },
        TreeBatchLimits {
            maximum_read_bytes: MAX_BATCH_READ_BYTES + 1,
            ..batch_limits()
        },
    ] {
        assert_eq!(
            stage(&mut fs, &mut vault, c, Some(f.root), &[], 7000, over).err(),
            Some(StorageError::ResourceLimit)
        );
    }
    for write in [
        PackedPageContext {
            family: 0,
            ..write_context(c)
        },
        PackedPageContext {
            profile: [8; 32],
            ..write_context(c)
        },
        PackedPageContext {
            scope: NamespaceRef::new(c.scope.database(), NamespaceId::from_bytes([8; 16])),
            ..write_context(c)
        },
        PackedPageContext {
            creation_revision: CommitRevision::FIRST,
            ..write_context(c)
        },
        PackedPageContext {
            object: [8; 16],
            ..write_context(c)
        },
        PackedPageContext {
            page: 1,
            ..write_context(c)
        },
    ] {
        assert_eq!(
            stage_batch(
                &mut fs,
                &root,
                &mut vault,
                &mut Entropy(7000),
                c,
                f.root.claimed,
                Some(f.root.location),
                &[],
                write,
                batch_limits()
            )
            .err(),
            Some(StorageError::InvalidState)
        );
    }
    let too_old = TreeReadContext {
        revision: CommitRevision::FIRST,
        ..c
    };
    assert_eq!(
        stage(
            &mut fs,
            &mut vault,
            too_old,
            Some(f.root),
            &[],
            7000,
            batch_limits()
        )
        .err(),
        Some(StorageError::IntegrityFailure)
    );
    let empty = stage(
        &mut fs,
        &mut vault,
        c,
        Some(f.root),
        &[],
        7000,
        batch_limits(),
    )
    .unwrap();
    assert!(empty.root().unwrap().location == f.root.location);
    assert!(empty.pack().is_none());
    assert_eq!(empty.report().read_pages, 0);
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn packed_tree_batch_maximum_delta_count_and_value_round_trip() {
    let mut f = fixture(7);
    let deltas: Vec<_> = (0..512_u16)
        .map(|key| {
            IndexDelta::new(
                key.to_be_bytes().to_vec(),
                None,
                Some(vec![(key % 251) as u8]),
            )
            .unwrap()
        })
        .collect();
    let result = stage(
        &mut f.fs,
        &mut f.vault,
        context(),
        None,
        &deltas,
        8000,
        batch_limits(),
    )
    .unwrap();
    assert_eq!(result.report().written_nodes, 1023);
    assert_eq!(result.report().written_chunks, 512);
    assert_eq!(result.report().read_pages, 0);
    let values: BTreeMap<_, _> = deltas
        .iter()
        .map(|delta| (delta.key().to_vec(), delta.after().unwrap().to_vec()))
        .collect();
    check(&mut f.fs, &f.vault, &result, &values);
    let mut over = deltas;
    over.push(IndexDelta::new(512_u16.to_be_bytes().to_vec(), None, Some(vec![1])).unwrap());
    assert_eq!(
        stage(
            &mut f.fs,
            &mut f.vault,
            context(),
            None,
            &over,
            8001,
            batch_limits()
        )
        .err(),
        Some(StorageError::ResourceLimit)
    );
    let delta = IndexDelta::new(
        b"large".to_vec(),
        None,
        Some(vec![91; logical::MAX_VALUE_BYTES]),
    )
    .unwrap();
    let large = stage(
        &mut f.fs,
        &mut f.vault,
        context(),
        None,
        std::slice::from_ref(&delta),
        8001,
        batch_limits(),
    )
    .unwrap();
    assert_eq!(large.report().written_nodes, 1);
    assert_eq!(
        large.report().written_chunks as usize,
        logical::MAX_VALUE_BYTES.div_ceil(MAX_CHUNK_DATA)
    );
    f.fs.restart().unwrap();
    let root = f.fs.root();
    let value = lookup(
        &mut f.fs,
        &root,
        &f.vault,
        large.context(),
        large.logical_root(),
        large.root().map(|root| root.location),
        b"large",
        limits(),
    )
    .unwrap()
    .value
    .unwrap();
    assert_eq!(value.as_slice(), delta.after().unwrap());
}
