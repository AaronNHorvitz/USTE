use super::*;
use crate::packed_tree_cursor::*;
type Entries = Vec<(Vec<u8>, Vec<u8>)>;
use super::corruption::rewrite;
mod reverse;

fn cursor_limits() -> TreeCursorLimits {
    TreeCursorLimits {
        maximum_path_branches: 128,
        maximum_candidates: 1000,
        maximum_returned_bytes: 64 * 1024 * 1024,
        maximum_pages: 5000,
        maximum_encoded_bytes: 5000 * ENCODED_PAGE_BYTES as u64,
    }
}
fn cursor(
    root: ChildReference,
    lower: &[u8],
    upper: Option<&[u8]>,
    limits: TreeCursorLimits,
) -> PackedTreeCursor {
    PackedTreeCursor::new(
        context(),
        root.claimed,
        Some(root.location),
        lower,
        upper,
        limits,
    )
    .unwrap()
}
fn collect<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    cursor: &mut PackedTreeCursor,
) -> Result<Entries, StorageError> {
    let directory = fs.root();
    let mut out = Vec::new();
    while let Some(entry) = cursor.next(fs, &directory, vault)? {
        assert_eq!(format!("{entry:?}"), "PackedCursorEntry([REDACTED])");
        let key_pointer = entry.key().as_ptr();
        let value_pointer = entry.value().as_ptr();
        let entry = entry.into_scan_entry();
        assert_eq!(entry.key.as_ptr(), key_pointer);
        assert_eq!(entry.value.as_ptr(), value_pointer);
        out.push((entry.key, entry.value));
    }
    Ok(out)
}
fn reference(
    values: &BTreeMap<Vec<u8>, Vec<u8>>,
    lower: &[u8],
    upper: Option<&[u8]>,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    values
        .iter()
        .filter(|(key, _)| key.as_slice() >= lower && upper.is_none_or(|end| key.as_slice() < end))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[test]
fn packed_tree_cursor_all_byte_and_prefix_bounds_match_reference() {
    let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut bounds = vec![
        vec![],
        vec![0, 0],
        vec![0, 1],
        vec![0, 255],
        b"aa".to_vec(),
        b"ab".to_vec(),
        b"aba".to_vec(),
        vec![255, 0],
    ];
    bounds.extend((0..=255).map(|byte| vec![byte]));
    for lower in &bounds {
        for upper in [
            None,
            Some(b"a".as_slice()),
            Some(b"ab".as_slice()),
            Some(b"b".as_slice()),
            Some([255].as_slice()),
        ] {
            if upper.is_some_and(|end| lower.as_slice() > end) {
                continue;
            }
            let mut c = cursor(f.root, lower, upper, cursor_limits());
            let actual = collect(&mut f.fs, &f.vault, &mut c).unwrap();
            assert_eq!(
                actual,
                reference(&f.values, lower, upper),
                "lower={lower:?} upper={upper:?}"
            );
            assert_eq!(c.report().returned_entries, actual.len() as u64);
            assert_eq!(
                c.report().returned_bytes,
                actual.iter().map(|(k, v)| (k.len() + v.len()) as u64).sum()
            );
            let before = c.report();
            assert!(collect(&mut f.fs, &f.vault, &mut c).unwrap().is_empty());
            assert_eq!(c.report(), before);
        }
    }
}

#[test]
fn packed_tree_cursor_compressed_seek_does_not_scan_prior_keyspace() {
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
            maximum_pages: 128,
            maximum_records: 2048,
            maximum_payload_bytes: 1024 * 1024,
        },
        &mut Entropy(70_000),
    )
    .unwrap();
    let values: BTreeMap<_, _> = (0..256_u16)
        .map(|i| {
            let mut key = b"compressed-prefix-".to_vec();
            key.extend(i.to_be_bytes());
            (key, vec![i as u8; (i as usize) % 23])
        })
        .collect();
    let root = build(
        &mut f.fs,
        &mut f.vault,
        &mut writer,
        &values.iter().collect::<Vec<_>>(),
        &mut BTreeMap::new(),
    );
    writer.finish(&mut f.fs, &mut f.vault).unwrap();
    f.fs.restart().unwrap();
    let mut bounds = vec![
        vec![],
        b"a".to_vec(),
        b"b".to_vec(),
        b"d".to_vec(),
        b"compressed".to_vec(),
        b"compressed-prefix-".to_vec(),
        b"compressed-prefix-\0".to_vec(),
        b"compressed-prefix-\x01".to_vec(),
    ];
    for i in [0_u16, 1, 63, 127, 200, 254, 255] {
        let mut key = b"compressed-prefix-".to_vec();
        key.extend(i.to_be_bytes());
        bounds.push(key.clone());
        key.push(0);
        bounds.push(key);
    }
    for lower in bounds {
        let mut c = cursor(root, &lower, None, cursor_limits());
        assert_eq!(
            collect(&mut f.fs, &f.vault, &mut c).unwrap(),
            reference(&values, &lower, None)
        );
        let mut c = reverse::reverse(root, b"", Some(&lower), cursor_limits());
        assert_eq!(
            collect(&mut f.fs, &f.vault, &mut c).unwrap(),
            reverse::expected(&values, b"", Some(&lower))
        );
    }
    let lower = b"compressed-prefix-\0\xff";
    let mut c = cursor(
        root,
        lower,
        None,
        TreeCursorLimits {
            maximum_pages: 10,
            maximum_candidates: 1,
            ..cursor_limits()
        },
    );
    assert_eq!(
        collect(&mut f.fs, &f.vault, &mut c).unwrap(),
        reference(&values, lower, None)
    );
    assert_eq!(c.report().pages, 10);
    assert_eq!(c.report().candidates, 1);
    let upper = b"compressed-prefix-\0\0";
    let mut c = reverse::reverse(
        root,
        b"",
        Some(upper),
        TreeCursorLimits {
            maximum_pages: 9,
            maximum_candidates: 1,
            ..cursor_limits()
        },
    );
    assert_eq!(
        collect(&mut f.fs, &f.vault, &mut c).unwrap(),
        reverse::expected(&values, b"", Some(upper))
    );
    assert_eq!(c.report().pages, 9);
    assert_eq!(c.report().candidates, 1);
}

#[test]
fn packed_tree_cursor_exact_cumulative_limits_and_sticky_errors() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let mut c = cursor(f.root, b"", None, cursor_limits());
    let expected = collect(&mut observed, &f.vault, &mut c).unwrap();
    let report = c.report();
    assert_eq!(report.pages, 21);
    assert_eq!(report.candidates, 7);
    let exact = TreeCursorLimits {
        maximum_candidates: report.candidates,
        maximum_returned_bytes: report.returned_bytes,
        maximum_pages: report.pages,
        maximum_encoded_bytes: report.encoded_bytes,
        ..cursor_limits()
    };
    let mut c = cursor(f.root, b"", None, exact);
    assert_eq!(collect(&mut observed, &f.vault, &mut c).unwrap(), expected);
    assert_eq!(c.report(), report);
    for limits in [
        TreeCursorLimits {
            maximum_candidates: exact.maximum_candidates - 1,
            ..exact
        },
        TreeCursorLimits {
            maximum_returned_bytes: exact.maximum_returned_bytes - 1,
            ..exact
        },
        TreeCursorLimits {
            maximum_pages: exact.maximum_pages - 1,
            ..exact
        },
        TreeCursorLimits {
            maximum_encoded_bytes: exact.maximum_encoded_bytes - 1,
            ..exact
        },
        TreeCursorLimits {
            maximum_path_branches: 0,
            ..exact
        },
    ] {
        let mut c = cursor(f.root, b"", None, limits);
        let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
        assert_eq!(
            collect(&mut fs, &f.vault, &mut c).err(),
            Some(StorageError::ResourceLimit)
        );
        let reads = fs.operation_count(Operation::OpenExisting);
        assert_eq!(
            collect(&mut fs, &f.vault, &mut c).err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(fs.operation_count(Operation::OpenExisting), reads);
    }
}

#[test]
fn packed_tree_cursor_every_read_fault_poisoning_and_restart_preserve_reference() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let mut c = cursor(f.root, b"a", Some(&[255]), cursor_limits());
    let expected = collect(&mut observed, &f.vault, &mut c).unwrap();
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
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut fs = FaultFileSystem::new(f.fs.clone(), plan);
                let mut c = cursor(f.root, b"a", Some(&[255]), cursor_limits());
                assert!(collect(&mut fs, &f.vault, &mut c).is_err());
                assert_eq!(fs.pending_faults(), 0);
                let before = fs.operation_count(Operation::OpenExisting);
                assert_eq!(
                    collect(&mut fs, &f.vault, &mut c).err(),
                    Some(StorageError::NeedsRecovery)
                );
                assert_eq!(fs.operation_count(Operation::OpenExisting), before);
                fs.restart().unwrap();
                let mut recovered = cursor(f.root, b"a", Some(&[255]), cursor_limits());
                assert_eq!(
                    collect(&mut fs, &f.vault, &mut recovered).unwrap(),
                    expected
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, c.report().pages * 9);
    assert_eq!(c.report().pages, 13);
    assert_eq!(cases, 117);
}

#[test]
fn packed_tree_cursor_empty_singleton_maximum_value_and_admission() {
    let mut f = fixture(logical::MAX_VALUE_BYTES);
    let mut c = cursor(f.root, b"b", Some(b"c"), cursor_limits());
    let actual = collect(&mut f.fs, &f.vault, &mut c).unwrap();
    assert_eq!(actual, reference(&f.values, b"b", Some(b"c")));
    let empty = logical::empty_commitment(logical_context());
    let mut c = PackedTreeCursor::new(context(), empty, None, b"", None, cursor_limits()).unwrap();
    assert!(collect(&mut f.fs, &f.vault, &mut c).unwrap().is_empty());
    assert_eq!(c.report(), TreeCursorReport::default());
    let key = [0_u8];
    let location = f.leaves[key.as_slice()];
    let claimed = logical::leaf_commitment(
        logical_context(),
        LeafProof {
            key: &key,
            value: logical::value_commitment(&[]).unwrap(),
        },
    )
    .unwrap();
    let mut c = cursor(
        ChildReference { location, claimed },
        b"",
        None,
        cursor_limits(),
    );
    assert_eq!(
        collect(&mut f.fs, &f.vault, &mut c).unwrap(),
        vec![(vec![0], vec![])]
    );
    let mut c = cursor(f.root, b"a", Some(b"a"), cursor_limits());
    assert!(collect(&mut f.fs, &f.vault, &mut c).unwrap().is_empty());
    assert_eq!(c.report(), TreeCursorReport::default());
    for limits in [
        TreeCursorLimits {
            maximum_path_branches: logical::MAX_BRANCH_BITS + 1,
            ..cursor_limits()
        },
        TreeCursorLimits {
            maximum_candidates: MAX_CURSOR_CANDIDATES + 1,
            ..cursor_limits()
        },
        TreeCursorLimits {
            maximum_returned_bytes: crate::packed_tree_validation::MAX_VALIDATION_LOGICAL_BYTES + 1,
            ..cursor_limits()
        },
        TreeCursorLimits {
            maximum_pages: MAX_CURSOR_PAGES + 1,
            ..cursor_limits()
        },
        TreeCursorLimits {
            maximum_encoded_bytes: MAX_CURSOR_ENCODED_BYTES + 1,
            ..cursor_limits()
        },
    ] {
        assert!(
            PackedTreeCursor::new(
                context(),
                f.root.claimed,
                Some(f.root.location),
                b"",
                None,
                limits
            )
            .is_err()
        );
    }
    assert!(
        PackedTreeCursor::new(
            context(),
            f.root.claimed,
            Some(f.root.location),
            b"b",
            Some(b"a"),
            cursor_limits()
        )
        .is_err()
    );
    assert!(
        PackedTreeCursor::new(
            context(),
            f.root.claimed,
            Some(f.root.location),
            &vec![0; logical::MAX_KEY_BYTES + 1],
            None,
            cursor_limits()
        )
        .is_err()
    );
    assert!(
        PackedTreeCursor::new(context(), f.root.claimed, None, b"", None, cursor_limits()).is_err()
    );
    assert!(
        PackedTreeCursor::new(
            context(),
            empty,
            Some(f.root.location),
            b"",
            None,
            cursor_limits()
        )
        .is_err()
    );
    assert!(
        PackedTreeCursor::new(
            TreeReadContext {
                family: 0,
                ..context()
            },
            empty,
            None,
            b"",
            None,
            cursor_limits()
        )
        .is_err()
    );
    assert!(
        PackedTreeCursor::new(
            TreeReadContext {
                revision: CommitRevision::FIRST,
                ..context()
            },
            f.root.claimed,
            Some(f.root.location),
            b"",
            None,
            cursor_limits()
        )
        .is_err()
    );
}

#[test]
fn packed_tree_cursor_late_content_corruption_and_wrong_scope_poison_without_values() {
    let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut c = cursor(f.root, b"a", None, cursor_limits());
    let directory = f.fs.root();
    let mut reversed = reverse::reverse(f.root, b"", None, cursor_limits());
    assert_eq!(
        reversed
            .next(&mut f.fs, &directory, &f.vault)
            .unwrap()
            .unwrap()
            .key(),
        &[255]
    );
    assert_eq!(
        c.next(&mut f.fs, &directory, &f.vault)
            .unwrap()
            .unwrap()
            .key(),
        b"a"
    );
    assert_eq!(
        c.next(&mut f.fs, &directory, &f.vault)
            .unwrap()
            .unwrap()
            .key(),
        b"ab"
    );
    let leaf = f.leaves[b"b".as_slice()];
    let ctx = context();
    let physical = leaf
        .resolve(ctx.scope, ctx.profile, ctx.family, ctx.revision)
        .unwrap();
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
    rewrite(&mut f, first, |record, owner| {
        let chunk = ValueChunk::decode(owner, record).unwrap();
        let mut data = chunk.data.to_vec();
        data[0] ^= 1;
        ValueChunk {
            data: &data,
            ..chunk
        }
        .encode(owner)
        .unwrap()
        .to_vec()
    });
    let directory = f.fs.root();
    assert_eq!(
        reversed.next(&mut f.fs, &directory, &f.vault).err(),
        Some(StorageError::IntegrityFailure)
    );
    assert_eq!(reversed.report().returned_entries, 1);
    assert_eq!(
        reversed.next(&mut f.fs, &directory, &f.vault).err(),
        Some(StorageError::NeedsRecovery)
    );
    assert_eq!(
        c.next(&mut f.fs, &directory, &f.vault).err(),
        Some(StorageError::IntegrityFailure)
    );
    assert_eq!(c.report().returned_entries, 2);
    assert_eq!(
        c.next(&mut f.fs, &directory, &f.vault).err(),
        Some(StorageError::NeedsRecovery)
    );
    for changed in [
        TreeReadContext {
            profile: [9; 32],
            ..ctx
        },
        TreeReadContext {
            scope: NamespaceRef::new(ctx.scope.database(), NamespaceId::from_bytes([9; 16])),
            ..ctx
        },
    ] {
        let mut c = PackedTreeCursor::new(
            changed,
            f.root.claimed,
            Some(f.root.location),
            b"",
            None,
            cursor_limits(),
        )
        .unwrap();
        assert!(c.next(&mut f.fs, &directory, &f.vault).is_err());
        assert_eq!(c.report().returned_entries, 0);
        assert_eq!(
            c.next(&mut f.fs, &directory, &f.vault).err(),
            Some(StorageError::NeedsRecovery)
        );
    }
}
