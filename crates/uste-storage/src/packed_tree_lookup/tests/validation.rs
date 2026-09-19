use super::corruption::rewrite;
use super::*;
use crate::packed_tree_validation::*;

fn scan_limits() -> TreeValidationLimits {
    TreeValidationLimits {
        maximum_path_branches: 128,
        maximum_nodes: 10_000,
        maximum_logical_bytes: 64 * 1024 * 1024,
        maximum_pages: 20_000,
        maximum_encoded_bytes: 20_000 * ENCODED_PAGE_BYTES as u64,
    }
}
fn scan<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    root: ChildReference,
    limits: TreeValidationLimits,
) -> Result<ValidatedPackedTree, StorageError> {
    let directory = fs.root();
    validate_tree(
        fs,
        &directory,
        vault,
        context(),
        root.claimed,
        Some(root.location),
        limits,
    )
}

#[test]
fn packed_tree_validation_complete_fixture_and_exact_work_limits() {
    let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
    let result = scan(&mut f.fs, &f.vault, f.root, scan_limits()).unwrap();
    let report = result.report();
    assert_eq!(result.commitment(), f.root.claimed);
    assert!(result.root() == Some(f.root.location));
    assert_eq!(result.context().revision, context().revision);
    assert_eq!(report.nodes, 13);
    assert_eq!(report.entries, 7);
    assert_eq!(
        report.logical_bytes,
        f.values
            .iter()
            .map(|(k, v)| (k.len() + v.len()) as u64)
            .sum()
    );
    assert_eq!(report.value_chunks, 8);
    assert_eq!(report.pages, 21);
    assert_eq!(report.encoded_bytes, 21 * ENCODED_PAGE_BYTES as u64);
    let exact = TreeValidationLimits {
        maximum_path_branches: report.maximum_depth,
        maximum_nodes: report.nodes,
        maximum_logical_bytes: report.logical_bytes,
        maximum_pages: report.pages,
        maximum_encoded_bytes: report.encoded_bytes,
    };
    assert_eq!(
        scan(&mut f.fs, &f.vault, f.root, exact).unwrap().report(),
        report
    );
    for (index, limits) in [
        TreeValidationLimits {
            maximum_nodes: exact.maximum_nodes - 1,
            ..exact
        },
        TreeValidationLimits {
            maximum_logical_bytes: exact.maximum_logical_bytes - 1,
            ..exact
        },
        TreeValidationLimits {
            maximum_pages: exact.maximum_pages - 1,
            ..exact
        },
        TreeValidationLimits {
            maximum_encoded_bytes: exact.maximum_encoded_bytes - 1,
            ..exact
        },
        TreeValidationLimits {
            maximum_path_branches: exact.maximum_path_branches - 1,
            ..exact
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
        assert_eq!(
            scan(&mut fs, &f.vault, f.root, limits).err(),
            Some(StorageError::ResourceLimit)
        );
        if index < 2 {
            assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
        }
    }
}

#[test]
fn packed_tree_validation_all_read_faults_return_no_receipt_and_restart_exactly() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let expected = scan(&mut observed, &f.vault, f.root, scan_limits())
        .unwrap()
        .report();
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        for occurrence in 1..=observed.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut fs = FaultFileSystem::new(
                    f.fs.clone(),
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                );
                assert!(scan(&mut fs, &f.vault, f.root, scan_limits()).is_err());
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                assert_eq!(
                    scan(&mut fs, &f.vault, f.root, scan_limits())
                        .unwrap()
                        .report(),
                    expected
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 189);
}

#[test]
fn packed_tree_validation_streams_maximum_values_and_generated_reference_trees() {
    let mut f = fixture(logical::MAX_VALUE_BYTES);
    let report = scan(&mut f.fs, &f.vault, f.root, scan_limits())
        .unwrap()
        .report();
    assert_eq!(
        report.value_chunks,
        5 + logical::MAX_VALUE_BYTES.div_ceil(MAX_CHUNK_DATA) as u64
    );
    for count in [1_usize, 2, 3, 7, 16, 31, 64, 127] {
        let directory = f.fs.root();
        let values: BTreeMap<_, _> = (0..count)
            .map(|i| {
                let mut key = (i as u32 * 7919).to_be_bytes().to_vec();
                if i % 3 == 0 {
                    key.extend([0, 255, 0]);
                }
                (key, vec![i as u8; (i * 37) % 1031])
            })
            .collect();
        let base = f
            .root
            .location
            .resolve(
                context().scope,
                context().profile,
                context().family,
                context().revision,
            )
            .unwrap();
        let mut writer = ImmutablePackWriter::create(
            &mut f.fs,
            &directory,
            PackedPageContext {
                object: [0; 16],
                page: 0,
                ..base
            },
            PackWriteLimits {
                maximum_pages: 128,
                maximum_records: 1024,
                maximum_payload_bytes: 1024 * 1024,
            },
            &mut Entropy(10_000 + count as u64 * 100),
        )
        .unwrap();
        let root = build(
            &mut f.fs,
            &mut f.vault,
            &mut writer,
            &values.iter().collect::<Vec<_>>(),
            &mut BTreeMap::new(),
        );
        writer.finish(&mut f.fs, &mut f.vault).unwrap();
        f.fs.restart().unwrap();
        let result = scan(&mut f.fs, &f.vault, root, scan_limits()).unwrap();
        assert_eq!(result.report().entries, count as u64);
        assert_eq!(result.report().nodes, 2 * count as u64 - 1);
        assert_eq!(
            result.report().logical_bytes,
            values.iter().map(|(k, v)| (k.len() + v.len()) as u64).sum()
        );
        assert_eq!(result.commitment(), root.claimed);
    }
}

#[test]
fn packed_tree_validation_empty_context_and_hard_admission_precede_io() {
    let f = fixture(1);
    let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let directory = fs.root();
    let empty = logical::empty_commitment(logical_context());
    let zero = TreeValidationLimits {
        maximum_path_branches: 0,
        maximum_nodes: 0,
        maximum_logical_bytes: 0,
        maximum_pages: 0,
        maximum_encoded_bytes: 0,
    };
    assert_eq!(
        validate_tree(&mut fs, &directory, &f.vault, context(), empty, None, zero)
            .unwrap()
            .report(),
        TreeValidationReport::default()
    );
    for (expected, root) in [(f.root.claimed, None), (empty, Some(f.root.location))] {
        assert_eq!(
            validate_tree(
                &mut fs,
                &directory,
                &f.vault,
                context(),
                expected,
                root,
                scan_limits()
            )
            .err(),
            Some(StorageError::IntegrityFailure)
        );
    }
    for limits in [
        TreeValidationLimits {
            maximum_path_branches: logical::MAX_BRANCH_BITS + 1,
            ..scan_limits()
        },
        TreeValidationLimits {
            maximum_nodes: MAX_VALIDATION_NODES + 1,
            ..scan_limits()
        },
        TreeValidationLimits {
            maximum_logical_bytes: MAX_VALIDATION_LOGICAL_BYTES + 1,
            ..scan_limits()
        },
        TreeValidationLimits {
            maximum_pages: MAX_VALIDATION_PAGES + 1,
            ..scan_limits()
        },
        TreeValidationLimits {
            maximum_encoded_bytes: MAX_VALIDATION_ENCODED_BYTES + 1,
            ..scan_limits()
        },
    ] {
        assert_eq!(
            scan(&mut fs, &f.vault, f.root, limits).err(),
            Some(StorageError::ResourceLimit)
        );
    }
    for altered in [
        TreeReadContext {
            family: 0,
            ..context()
        },
        TreeReadContext {
            revision: CommitRevision::FIRST,
            ..context()
        },
    ] {
        assert!(
            validate_tree(
                &mut fs,
                &directory,
                &f.vault,
                altered,
                f.root.claimed,
                Some(f.root.location),
                scan_limits()
            )
            .is_err()
        );
    }
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
    for altered in [
        TreeReadContext {
            profile: [9; 32],
            ..context()
        },
        TreeReadContext {
            family: 9,
            ..context()
        },
        TreeReadContext {
            scope: NamespaceRef::new(context().scope.database(), NamespaceId::from_bytes([9; 16])),
            ..context()
        },
    ] {
        assert!(
            validate_tree(
                &mut fs,
                &directory,
                &f.vault,
                altered,
                f.root.claimed,
                Some(f.root.location),
                scan_limits()
            )
            .is_err()
        );
    }
}

#[test]
fn packed_tree_validation_rejects_self_consistent_late_split_and_reversed_keys() {
    for (bit, reverse, valid) in [(7, false, true), (8, false, false), (7, true, false)] {
        let mut f = fixture(1);
        let directory = f.fs.root();
        let base = f
            .root
            .location
            .resolve(
                context().scope,
                context().profile,
                context().family,
                context().revision,
            )
            .unwrap();
        let mut writer = ImmutablePackWriter::create(
            &mut f.fs,
            &directory,
            PackedPageContext {
                object: [0; 16],
                page: 0,
                ..base
            },
            PackWriteLimits {
                maximum_pages: 1,
                maximum_records: 3,
                maximum_payload_bytes: 1024,
            },
            &mut Entropy(50_000),
        )
        .unwrap();
        let mut children = Vec::new();
        for key in [0_u8, 3] {
            let key = [key];
            let node = TreeNode::Leaf {
                key: &key,
                value: logical::value_commitment(&[]).unwrap(),
                first_chunk: None,
            };
            let claimed = node.commitment(writer.context()).unwrap();
            let bytes = node.encode(writer.context()).unwrap();
            let location = PackedLocator::from_address(
                writer
                    .append(&mut f.fs, &mut f.vault, PackedRecordKind::TreeNode, &bytes)
                    .unwrap(),
            );
            children.push(ChildReference { location, claimed });
        }
        if reverse {
            children.reverse();
        }
        let node = TreeNode::Branch {
            bit,
            left: children[0],
            right: children[1],
        };
        let claimed = node.commitment(writer.context()).unwrap();
        let bytes = node.encode(writer.context()).unwrap();
        let location = PackedLocator::from_address(
            writer
                .append(&mut f.fs, &mut f.vault, PackedRecordKind::TreeNode, &bytes)
                .unwrap(),
        );
        writer.finish(&mut f.fs, &mut f.vault).unwrap();
        f.fs.restart().unwrap();
        let result = scan(
            &mut f.fs,
            &f.vault,
            ChildReference { location, claimed },
            scan_limits(),
        );
        if valid {
            assert_eq!(result.unwrap().report().entries, 2);
        } else {
            assert_eq!(result.err(), Some(StorageError::IntegrityFailure));
        }
    }
}

#[test]
fn packed_tree_validation_stream_hash_matches_whole_values_and_is_fail_closed() {
    for length in [0, 1, 255, MAX_CHUNK_DATA, 2 * MAX_CHUNK_DATA + 7] {
        let data: Vec<_> = (0..length).map(|n| (n % 251) as u8).collect();
        for partition in [1, 7, 127, MAX_CHUNK_DATA] {
            let mut hash = logical::ValueStream::new(length as u64).unwrap();
            for chunk in data.chunks(partition) {
                hash.update(chunk).unwrap();
            }
            assert!(hash.finish().unwrap() == logical::value_commitment(&data).unwrap());
        }
    }
    assert!(logical::ValueStream::new(logical::MAX_VALUE_BYTES as u64 + 1).is_err());
    assert!(logical::ValueStream::new(1).unwrap().finish().is_err());
    let mut hash = logical::ValueStream::new(1).unwrap();
    assert!(hash.update(b"xx").is_err());
    assert!(hash.update(b"x").is_err());
    assert!(hash.finish().is_err());
}

#[test]
fn packed_tree_validation_late_chunk_corruption_and_cycles_invalidate_prior_receipts() {
    for variant in 0..4 {
        let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
        assert!(scan(&mut f.fs, &f.vault, f.root, scan_limits()).is_ok());
        let leaf = f.leaves[b"b".as_slice()];
        let c = context();
        let physical = leaf
            .resolve(c.scope, c.profile, c.family, c.revision)
            .unwrap();
        let directory = f.fs.root();
        let (page, _) = read_linked_record_page(
            &mut f.fs,
            &directory,
            &f.vault,
            physical,
            leaf.slot(),
            ENCODED_PAGE_BYTES as u64,
        )
        .unwrap();
        let TreeNode::Leaf {
            first_chunk: Some(first),
            ..
        } = TreeNode::decode(physical, page.record(leaf.slot()).unwrap()).unwrap()
        else {
            panic!("chunked leaf")
        };
        drop(page);
        if variant == 3 {
            rewrite(&mut f, leaf, |record, owner| {
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
        } else {
            rewrite(&mut f, first, |record, owner| {
                let chunk = ValueChunk::decode(owner, record).unwrap();
                let mut data = chunk.data.to_vec();
                if variant == 0 {
                    data[0] ^= 1;
                }
                ValueChunk {
                    data: &data,
                    next: if variant == 1 {
                        Some(first)
                    } else {
                        chunk.next
                    },
                    remaining: if variant == 2 {
                        chunk.remaining + MAX_CHUNK_DATA as u32
                    } else {
                        chunk.remaining
                    },
                }
                .encode(owner)
                .unwrap()
                .to_vec()
            });
        }
        assert_eq!(
            scan(&mut f.fs, &f.vault, f.root, scan_limits()).err(),
            Some(StorageError::IntegrityFailure)
        );
        assert_eq!(
            query(&mut f.fs, &f.vault, f.root, b"a", limits())
                .unwrap()
                .value
                .unwrap()
                .as_slice(),
            b"a"
        );
    }
}
