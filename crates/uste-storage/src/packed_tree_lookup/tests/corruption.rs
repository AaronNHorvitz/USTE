use super::*;
use crate::packed_index_page::PackedRecord;
use std::fmt::Write as _;

fn name(object: [u8; 16]) -> EntryName {
    let mut name = "pack-".to_owned();
    for byte in object {
        write!(&mut name, "{byte:02x}").unwrap();
    }
    EntryName::new(name).unwrap()
}
fn physical(location: PackedLocator) -> PackedPageContext {
    let c = context();
    location
        .resolve(c.scope, c.profile, c.family, c.revision)
        .unwrap()
}
fn rewrite(
    f: &mut Fixture,
    location: PackedLocator,
    alter: impl FnOnce(PackedRecord<'_>, PackedPageContext) -> Vec<u8>,
) {
    let c = physical(location);
    let directory = f.fs.root();
    let (page, _) = read_linked_record_page(
        &mut f.fs,
        &directory,
        &f.vault,
        c,
        location.slot(),
        ENCODED_PAGE_BYTES as u64,
    )
    .unwrap();
    let changed = alter(page.record(location.slot()).unwrap(), c);
    let mut builder = PackedPageBuilder::new(c).unwrap();
    for slot in 0..page.record_count() {
        let record = page.record(slot).unwrap();
        builder
            .push(
                record.kind,
                if slot == location.slot() {
                    &changed
                } else {
                    record.payload
                },
            )
            .unwrap();
    }
    let encoded = builder.seal(&mut f.vault).unwrap();
    let file = f.fs.open_existing(&directory, &name(c.object)).unwrap();
    write_all_at(
        &mut f.fs,
        &file,
        c.page * ENCODED_PAGE_BYTES as u64,
        &encoded,
    )
    .unwrap();
    f.fs.sync_all(&file).unwrap();
    f.fs.restart().unwrap();
}

#[test]
fn packed_tree_lookup_authentic_false_leaf_chunk_and_cycle_never_return_values() {
    for variant in 0..4 {
        let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
        let leaf = f.leaves[b"b".as_slice()];
        let root = f.root.location;
        match variant {
            0 => rewrite(&mut f, leaf, |record, owner| {
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
            }),
            1 => rewrite(&mut f, leaf, |record, owner| {
                let TreeNode::Leaf { key, value, .. } = TreeNode::decode(owner, record).unwrap()
                else {
                    panic!("leaf")
                };
                TreeNode::Leaf {
                    key,
                    value,
                    first_chunk: Some(leaf),
                }
                .encode(owner)
                .unwrap()
                .to_vec()
            }),
            2 => rewrite(&mut f, root, |record, owner| {
                let TreeNode::Branch { bit, left, right } =
                    TreeNode::decode(owner, record).unwrap()
                else {
                    panic!("branch")
                };
                TreeNode::Branch {
                    bit,
                    left: ChildReference {
                        location: root,
                        ..left
                    },
                    right,
                }
                .encode(owner)
                .unwrap()
                .to_vec()
            }),
            _ => {
                let owner = physical(leaf);
                let directory = f.fs.root();
                let (page, _) = read_linked_record_page(
                    &mut f.fs,
                    &directory,
                    &f.vault,
                    owner,
                    leaf.slot(),
                    ENCODED_PAGE_BYTES as u64,
                )
                .unwrap();
                let TreeNode::Leaf {
                    first_chunk: Some(first),
                    ..
                } = TreeNode::decode(owner, page.record(leaf.slot()).unwrap()).unwrap()
                else {
                    panic!("leaf")
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
            }
        }
        assert!(
            query(&mut f.fs, &f.vault, f.root, b"b", limits()).is_err(),
            "variant {variant}"
        );
    }
}

#[test]
fn packed_tree_lookup_rejects_self_consistent_repeated_branch_bits() {
    let mut f = fixture(7);
    let root = f.root.location;
    let directory = f.fs.root();
    let (page, _) = read_linked_record_page(
        &mut f.fs,
        &directory,
        &f.vault,
        physical(root),
        root.slot(),
        ENCODED_PAGE_BYTES as u64,
    )
    .unwrap();
    let TreeNode::Branch {
        bit: parent_bit,
        left,
        ..
    } = TreeNode::decode(physical(root), page.record(root.slot()).unwrap()).unwrap()
    else {
        panic!("branch")
    };
    drop(page);
    let mut replacement = None;
    rewrite(&mut f, left.location, |record, owner| {
        let TreeNode::Branch { left, right, .. } = TreeNode::decode(owner, record).unwrap() else {
            panic!("branch")
        };
        let malformed = TreeNode::Branch {
            bit: parent_bit,
            left,
            right,
        };
        replacement = Some(malformed.commitment(owner).unwrap());
        malformed.encode(owner).unwrap().to_vec()
    });
    let mut false_root = None;
    rewrite(&mut f, root, |record, owner| {
        let TreeNode::Branch { bit, left, right } = TreeNode::decode(owner, record).unwrap() else {
            panic!("branch")
        };
        let malformed = TreeNode::Branch {
            bit,
            left: ChildReference {
                claimed: replacement.unwrap(),
                ..left
            },
            right,
        };
        false_root = Some(malformed.commitment(owner).unwrap());
        malformed.encode(owner).unwrap().to_vec()
    });
    // Defense in depth: even this self-consistent, noncanonical caller root is refused.
    f.root.claimed = false_root.unwrap();
    assert_eq!(
        query(&mut f.fs, &f.vault, f.root, b"b", limits()).err(),
        Some(StorageError::IntegrityFailure)
    );
}

#[test]
fn packed_tree_lookup_context_and_late_ciphertext_substitution_fail_closed() {
    let f = fixture(2 * MAX_CHUNK_DATA + 7);
    let c = context();
    for altered in [
        TreeReadContext {
            scope: NamespaceRef::new(DatabaseId::from_bytes([9; 16]), c.scope.namespace()),
            ..c
        },
        TreeReadContext {
            scope: NamespaceRef::new(c.scope.database(), NamespaceId::from_bytes([9; 16])),
            ..c
        },
        TreeReadContext {
            profile: [9; 32],
            ..c
        },
        TreeReadContext { family: 9, ..c },
        TreeReadContext {
            revision: CommitRevision::new(8).unwrap(),
            ..c
        },
    ] {
        let mut fs = FaultFileSystem::new(f.fs.clone(), FaultPlan::default());
        let root = fs.root();
        assert!(
            lookup(
                &mut fs,
                &root,
                &f.vault,
                altered,
                f.root.claimed,
                Some(f.root.location),
                b"b",
                limits()
            )
            .is_err()
        );
        if altered.revision < c.revision {
            assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
        }
    }
    let mut f = f;
    let leaf = f.leaves[b"b".as_slice()];
    let directory = f.fs.root();
    let (page, _) = read_linked_record_page(
        &mut f.fs,
        &directory,
        &f.vault,
        physical(leaf),
        leaf.slot(),
        ENCODED_PAGE_BYTES as u64,
    )
    .unwrap();
    let TreeNode::Leaf {
        first_chunk: Some(first),
        ..
    } = TreeNode::decode(physical(leaf), page.record(leaf.slot()).unwrap()).unwrap()
    else {
        panic!("leaf")
    };
    drop(page);
    let c = physical(first);
    let file = f.fs.open_existing(&directory, &name(c.object)).unwrap();
    let offset = c.page * ENCODED_PAGE_BYTES as u64 + 100;
    let mut byte = [0];
    read_exact_at(&mut f.fs, &file, offset, &mut byte).unwrap();
    byte[0] ^= 1;
    write_all_at(&mut f.fs, &file, offset, &byte).unwrap();
    f.fs.sync_all(&file).unwrap();
    f.fs.restart().unwrap();
    assert!(query(&mut f.fs, &f.vault, f.root, b"b", limits()).is_err());
}

#[test]
fn packed_tree_lookup_self_consistent_wrong_key_partition_is_not_absence() {
    let mut f = fixture(1);
    let c = context();
    let directory = f.fs.root();
    let base = physical(f.root.location);
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
        &mut Entropy(5000),
    )
    .unwrap();
    let mut children = Vec::new();
    // Deliberately place z on the zero side and a on the one side of their first differing bit.
    for key in [b"z", b"a"] {
        let leaf = TreeNode::Leaf {
            key,
            value: logical::value_commitment(b"").unwrap(),
            first_chunk: None,
        };
        let claimed = leaf.commitment(writer.context()).unwrap();
        let bytes = leaf.encode(writer.context()).unwrap();
        let address = writer
            .append(&mut f.fs, &mut f.vault, PackedRecordKind::TreeNode, &bytes)
            .unwrap();
        children.push(ChildReference {
            location: PackedLocator::from_address(address),
            claimed,
        });
    }
    let branch = TreeNode::Branch {
        bit: 4,
        left: children[0],
        right: children[1],
    };
    let expected = branch.commitment(writer.context()).unwrap();
    let bytes = branch.encode(writer.context()).unwrap();
    let address = writer
        .append(&mut f.fs, &mut f.vault, PackedRecordKind::TreeNode, &bytes)
        .unwrap();
    writer.finish(&mut f.fs, &mut f.vault).unwrap();
    for key in [b"a", b"z"] {
        assert_eq!(
            lookup(
                &mut f.fs,
                &directory,
                &f.vault,
                c,
                expected,
                Some(PackedLocator::from_address(address)),
                key,
                limits()
            )
            .err(),
            Some(StorageError::IntegrityFailure)
        );
    }
}
