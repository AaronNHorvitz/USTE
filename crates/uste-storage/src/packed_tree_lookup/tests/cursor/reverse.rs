use super::*;

pub(super) fn reverse(
    root: ChildReference,
    lower: &[u8],
    upper: Option<&[u8]>,
    limits: TreeCursorLimits,
) -> PackedTreeCursor {
    PackedTreeCursor::new_reverse(
        context(),
        root.claimed,
        Some(root.location),
        lower,
        upper,
        limits,
    )
    .unwrap()
}
pub(super) fn expected(
    values: &BTreeMap<Vec<u8>, Vec<u8>>,
    lower: &[u8],
    upper: Option<&[u8]>,
) -> Entries {
    values
        .iter()
        .rev()
        .filter(|(k, _)| k.as_slice() > lower && upper.is_none_or(|end| k.as_slice() <= end))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[test]
fn packed_tree_reverse_all_byte_prefix_and_exact_bounds_match_reference() {
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
    bounds.extend((0..=255).map(|b| vec![b]));
    for upper in bounds.iter().map(|b| Some(b.as_slice())).chain([None]) {
        for lower in [b"".as_slice(), &[0], b"a", b"ab", b"b", &[255]] {
            if upper.is_some_and(|end| lower > end) {
                continue;
            }
            let mut c = reverse(f.root, lower, upper, cursor_limits());
            assert_eq!(
                collect(&mut f.fs, &f.vault, &mut c).unwrap(),
                expected(&f.values, lower, upper),
                "{lower:?} {upper:?}"
            );
            let report = c.report();
            assert!(collect(&mut f.fs, &f.vault, &mut c).unwrap().is_empty());
            assert_eq!(c.report(), report);
        }
    }
}

#[test]
fn packed_tree_reverse_exact_budgets_and_all_observed_faults_are_sticky() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let mut c = reverse(f.root, b"a", Some(&[255]), cursor_limits());
    let result = collect(&mut observed, &f.vault, &mut c).unwrap();
    let report = c.report();
    let exact = TreeCursorLimits {
        maximum_candidates: report.candidates,
        maximum_returned_bytes: report.returned_bytes,
        maximum_pages: report.pages,
        maximum_encoded_bytes: report.encoded_bytes,
        ..cursor_limits()
    };
    let mut c = reverse(f.root, b"a", Some(&[255]), exact);
    assert_eq!(collect(&mut observed, &f.vault, &mut c).unwrap(), result);
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
        let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
        let mut c = reverse(f.root, b"a", Some(&[255]), limits);
        assert_eq!(
            collect(&mut fs, &f.vault, &mut c).err(),
            Some(StorageError::ResourceLimit)
        );
        let before = fs.operation_count(Operation::OpenExisting);
        assert_eq!(
            collect(&mut fs, &f.vault, &mut c).err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(fs.operation_count(Operation::OpenExisting), before);
    }
    // Observe once, independently of the exact-budget second traversal above.
    let mut observed = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
    let mut c = reverse(f.root, b"a", Some(&[255]), exact);
    collect(&mut observed, &f.vault, &mut c).unwrap();
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
                let mut c = reverse(f.root, b"a", Some(&[255]), exact);
                assert!(collect(&mut fs, &f.vault, &mut c).is_err());
                assert_eq!(fs.pending_faults(), 0);
                let before = fs.operation_count(Operation::OpenExisting);
                assert_eq!(
                    collect(&mut fs, &f.vault, &mut c).err(),
                    Some(StorageError::NeedsRecovery)
                );
                assert_eq!(fs.operation_count(Operation::OpenExisting), before);
                fs.restart().unwrap();
                let mut c = reverse(f.root, b"a", Some(&[255]), exact);
                assert_eq!(collect(&mut fs, &f.vault, &mut c).unwrap(), result);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, report.pages * 9);
}

#[test]
fn packed_tree_reverse_empty_singleton_and_maximum_value() {
    let mut f = fixture(logical::MAX_VALUE_BYTES);
    let mut c = reverse(f.root, b"ab", Some(b"b"), cursor_limits());
    assert_eq!(
        collect(&mut f.fs, &f.vault, &mut c).unwrap(),
        expected(&f.values, b"ab", Some(b"b"))
    );
    let empty = logical::empty_commitment(logical_context());
    let mut c =
        PackedTreeCursor::new_reverse(context(), empty, None, b"", None, cursor_limits()).unwrap();
    assert!(collect(&mut f.fs, &f.vault, &mut c).unwrap().is_empty());
    assert_eq!(c.report(), TreeCursorReport::default());
    let key = [0_u8];
    let root = ChildReference {
        location: f.leaves[key.as_slice()],
        claimed: logical::leaf_commitment(
            logical_context(),
            LeafProof {
                key: &key,
                value: logical::value_commitment(&[]).unwrap(),
            },
        )
        .unwrap(),
    };
    for upper in [None, Some(key.as_slice()), Some(b"".as_slice())] {
        let mut c = reverse(root, b"", upper, cursor_limits());
        let result = collect(&mut f.fs, &f.vault, &mut c).unwrap();
        assert_eq!(
            result,
            if upper == Some(b"".as_slice()) {
                vec![]
            } else {
                vec![(vec![0], vec![])]
            }
        );
    }
}

#[test]
fn packed_tree_reverse_sparse_variable_length_compressed_paths_match_reference() {
    let mut f = fixture(1);
    let mut seed = 0x1501_2026_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut values = BTreeMap::new();
    for _ in 0..96 {
        let len = 1 + next() as usize % 12;
        let mut key = b"shared".to_vec();
        key.extend((0..len).map(|_| next() as u8));
        values.insert(key, vec![next() as u8; 3]);
    }
    let physical = f
        .root
        .location
        .resolve(
            context().scope,
            context().profile,
            context().family,
            context().revision,
        )
        .unwrap();
    let directory = f.fs.root();
    let mut writer = ImmutablePackWriter::create(
        &mut f.fs,
        &directory,
        PackedPageContext {
            object: [0; 16],
            page: 0,
            ..physical
        },
        PackWriteLimits {
            maximum_pages: 128,
            maximum_records: 2048,
            maximum_payload_bytes: 1024 * 1024,
        },
        &mut Entropy(90_000),
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
    let mut bounds = vec![
        vec![],
        b"share".to_vec(),
        b"shared".to_vec(),
        b"shares".to_vec(),
    ];
    for key in values.keys() {
        bounds.push(key.clone());
        bounds.push(key[..key.len() - 1].to_vec());
        let mut after = key.clone();
        after.push(0);
        bounds.push(after);
    }
    for upper in &bounds {
        let mut c = reverse(root, b"", Some(upper), cursor_limits());
        assert_eq!(
            collect(&mut f.fs, &f.vault, &mut c).unwrap(),
            expected(&values, b"", Some(upper))
        );
        let lower = &bounds[next() as usize % bounds.len()];
        if lower <= upper {
            let mut c = reverse(root, lower, Some(upper), cursor_limits());
            assert_eq!(
                collect(&mut f.fs, &f.vault, &mut c).unwrap(),
                expected(&values, lower, Some(upper))
            );
        }
    }
}
