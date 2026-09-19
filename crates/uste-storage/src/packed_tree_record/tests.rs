use super::*;
use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

mod transport;

fn owner() -> PackedPageContext {
    PackedPageContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([3; 16]),
        creation_revision: CommitRevision::new(9).unwrap(),
        profile: [4; 32],
        family: 5,
        object: [6; 16],
        page: 0,
    }
}
fn location() -> PackedLocator {
    PackedLocator {
        object: [7; 16],
        revision: CommitRevision::new(2).unwrap(),
        epoch: KeyEpoch::new(3).unwrap(),
        writer: WriterIncarnationId::from_bytes([8; 16]),
        page: 4,
        slot: 5,
    }
}
fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn node(payload: &[u8]) -> Result<TreeNode<'_>, StorageError> {
    TreeNode::decode(
        owner(),
        PackedRecord {
            kind: PackedRecordKind::TreeNode,
            payload,
        },
    )
}
fn chunk(payload: &[u8]) -> Result<ValueChunk<'_>, StorageError> {
    ValueChunk::decode(
        owner(),
        PackedRecord {
            kind: PackedRecordKind::ValueChunk,
            payload,
        },
    )
}
fn leaf() -> TreeNode<'static> {
    TreeNode::Leaf {
        key: b"k",
        value: logical::value_commitment(b"A").unwrap(),
        first_chunk: Some(location()),
    }
}
fn branch() -> TreeNode<'static> {
    TreeNode::Branch {
        bit: 12,
        left: ChildReference {
            location: location(),
            claimed: OrderedCommitment::claimed_nonempty(2, 8, [17; 32]).unwrap(),
        },
        right: ChildReference {
            location: PackedLocator {
                slot: 6,
                ..location()
            },
            claimed: OrderedCommitment::claimed_nonempty(3, 9, [34; 32]).unwrap(),
        },
    }
}

#[test]
fn packed_tree_record_literal_leaf_branch_chunk_and_locator_bytes() {
    let locator = hex(
        "070707070707070707070707070707070000000000000002000000000000000308080808080808080808080808080808000000040005",
    );
    assert_eq!(locator.len(), 54);
    let mut expected =
        hex("010100016b00000001df25c5122e9028e790f47ff010b032be55738b6fbc9c4689735272368c0fb83701");
    expected.extend(&locator);
    let encoded = leaf().encode(owner()).unwrap();
    assert_eq!(encoded.as_slice(), expected);
    assert_eq!(
        node(&encoded).unwrap().encode(owner()).unwrap().as_slice(),
        expected
    );
    assert_eq!(
        node(&encoded).unwrap().commitment(owner()).unwrap(),
        leaf().commitment(owner()).unwrap()
    );
    let encoded = branch().encode(owner()).unwrap();
    let mut expected = vec![1, 2, 0, 0, 0, 12];
    expected.extend(&locator);
    expected.extend(hex("00000000000000020000000000000008"));
    expected.extend([17; 32]);
    let mut right = locator;
    right[53] = 6;
    expected.extend(right);
    expected.extend(hex("00000000000000030000000000000009"));
    expected.extend([34; 32]);
    assert_eq!(encoded.as_slice(), expected);
    assert_eq!(encoded.len(), 210);
    assert_eq!(
        node(&encoded).unwrap().encode(owner()).unwrap().as_slice(),
        expected
    );
    let terminal = ValueChunk {
        remaining: 3,
        next: None,
        data: b"abc",
    }
    .encode(owner())
    .unwrap();
    assert_eq!(terminal.as_slice(), &[1, 0, 0, 0, 3, 0, 3, 0, 97, 98, 99]);
    assert_eq!(chunk(&terminal).unwrap().data, b"abc");
}

#[test]
fn packed_tree_record_all_truncations_trailing_bytes_tags_and_flags_fail_closed() {
    let leaf = leaf().encode(owner()).unwrap();
    let branch = branch().encode(owner()).unwrap();
    let data = vec![91; MAX_CHUNK_DATA];
    let chunk_bytes = ValueChunk {
        remaining: MAX_CHUNK_DATA as u32 + 1,
        next: Some(location()),
        data: &data,
    }
    .encode(owner())
    .unwrap();
    assert_eq!(chunk_bytes.len(), MAX_RECORD_PAYLOAD);
    for bytes in [&leaf, &branch] {
        for length in 0..bytes.len() {
            assert!(node(&bytes[..length]).is_err());
        }
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(node(&trailing).is_err());
        for offset in [0, 1] {
            let mut bad = bytes.to_vec();
            bad[offset] = 255;
            assert!(node(&bad).is_err());
        }
        assert!(
            TreeNode::decode(
                owner(),
                PackedRecord {
                    kind: PackedRecordKind::ValueChunk,
                    payload: bytes
                }
            )
            .is_err()
        );
    }
    for length in 0..chunk_bytes.len() {
        assert!(chunk(&chunk_bytes[..length]).is_err());
    }
    let mut trailing = chunk_bytes.to_vec();
    trailing.push(0);
    assert!(chunk(&trailing).is_err());
    for offset in [0, 7] {
        let mut bad = chunk_bytes.to_vec();
        bad[offset] = 255;
        assert!(chunk(&bad).is_err());
    }
    assert!(
        ValueChunk::decode(
            owner(),
            PackedRecord {
                kind: PackedRecordKind::TreeNode,
                payload: &chunk_bytes
            }
        )
        .is_err()
    );
    let mut bad = leaf.to_vec();
    bad[41] = 2;
    assert!(node(&bad).is_err());
}

#[test]
fn packed_tree_record_locator_boundaries_and_future_links_are_rejected() {
    for invalid in [
        PackedLocator {
            object: [0; 16],
            ..location()
        },
        PackedLocator {
            page: MAX_PAGES as u32,
            ..location()
        },
        PackedLocator {
            slot: MAX_SLOTS,
            ..location()
        },
        PackedLocator {
            revision: CommitRevision::new(10).unwrap(),
            ..location()
        },
    ] {
        let TreeNode::Leaf { key, value, .. } = leaf() else {
            unreachable!()
        };
        assert!(
            TreeNode::Leaf {
                key,
                value,
                first_chunk: Some(invalid)
            }
            .encode(owner())
            .is_err()
        );
    }
    let encoded = leaf().encode(owner()).unwrap();
    // The locator begins after 42 leaf bytes. Reserved zero revisions/epochs are wire errors.
    for (start, count) in [(42, 16), (58, 8), (66, 8)] {
        let mut bad = encoded.to_vec();
        bad[start..start + count].fill(0);
        assert!(node(&bad).is_err());
    }
    let mut bad = encoded.to_vec();
    bad[90..94].copy_from_slice(&(MAX_PAGES as u32).to_be_bytes());
    assert!(node(&bad).is_err());
    let mut bad = encoded.to_vec();
    bad[94..96].copy_from_slice(&MAX_SLOTS.to_be_bytes());
    assert!(node(&bad).is_err());
    let TreeNode::Branch { bit, left, right } = branch() else {
        unreachable!()
    };
    assert!(
        TreeNode::Branch {
            bit,
            left,
            right: ChildReference {
                location: left.location,
                ..right
            }
        }
        .encode(owner())
        .is_err()
    );
    let mut duplicate = branch().encode(owner()).unwrap();
    duplicate[108..162].copy_from_slice(&encoded[42..96]);
    assert!(node(&duplicate).is_err());
    assert!(
        TreeNode::Branch {
            bit: logical::MAX_BRANCH_BITS,
            left,
            right
        }
        .encode(owner())
        .is_err()
    );
    let TreeNode::Leaf { key, value, .. } = leaf() else {
        unreachable!()
    };
    let maximum = PackedLocator {
        page: MAX_PAGES as u32 - 1,
        slot: MAX_SLOTS - 1,
        revision: owner().creation_revision,
        ..location()
    };
    assert!(
        TreeNode::Leaf {
            key,
            value,
            first_chunk: Some(maximum)
        }
        .encode(owner())
        .is_ok()
    );
}

#[test]
fn packed_tree_record_key_value_and_imported_summary_bounds_are_exact() {
    let value = logical::value_commitment(b"").unwrap();
    for length in [0, logical::MAX_KEY_BYTES, logical::MAX_KEY_BYTES + 1] {
        let key = vec![91; length];
        let result = TreeNode::Leaf {
            key: &key,
            value,
            first_chunk: None,
        }
        .encode(owner());
        assert_eq!(result.is_ok(), length == logical::MAX_KEY_BYTES);
    }
    assert!(
        TreeNode::Leaf {
            key: b"k",
            value,
            first_chunk: Some(location())
        }
        .encode(owner())
        .is_err()
    );
    let mut wrong = value;
    wrong.digest[0] ^= 1;
    assert!(
        TreeNode::Leaf {
            key: b"k",
            value: wrong,
            first_chunk: None
        }
        .encode(owner())
        .is_err()
    );
    for length in [
        1,
        logical::MAX_VALUE_BYTES as u64,
        logical::MAX_VALUE_BYTES as u64 + 1,
    ] {
        let value = ValueCommitment {
            length,
            digest: [23; 32],
        };
        assert!(
            TreeNode::Leaf {
                key: b"k",
                value,
                first_chunk: None
            }
            .encode(owner())
            .is_err()
        );
        assert_eq!(
            TreeNode::Leaf {
                key: b"k",
                value,
                first_chunk: Some(location())
            }
            .encode(owner())
            .is_ok(),
            length <= logical::MAX_VALUE_BYTES as u64
        );
    }
    let maximum = logical::MAX_ENTRIES * (logical::MAX_KEY_BYTES + logical::MAX_VALUE_BYTES) as u64;
    for (entries, bytes, valid) in [
        (0, 0, false),
        (1, 0, false),
        (1, 1, true),
        (logical::MAX_ENTRIES, maximum, true),
        (logical::MAX_ENTRIES, maximum + 1, false),
        (logical::MAX_ENTRIES + 1, maximum, false),
        (u64::MAX, u64::MAX, false),
    ] {
        assert_eq!(
            OrderedCommitment::claimed_nonempty(entries, bytes, [0; 32]).is_ok(),
            valid
        );
    }
    let original = branch().encode(owner()).unwrap();
    for (offset, value) in [
        (60, 0),
        (60, logical::MAX_ENTRIES + 1),
        (68, 0),
        (68, u64::MAX),
    ] {
        let mut bad = original.to_vec();
        bad[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
        assert!(node(&bad).is_err());
    }
}

#[test]
fn packed_tree_record_chunk_partition_and_termination_are_closed() {
    assert_eq!(MAX_CHUNK_DATA, 15_729);
    for remaining in [
        1,
        MAX_CHUNK_DATA as u32,
        MAX_CHUNK_DATA as u32 + 1,
        logical::MAX_VALUE_BYTES as u32,
    ] {
        let data = vec![91; (remaining as usize).min(MAX_CHUNK_DATA)];
        let next = (remaining as usize > data.len()).then_some(location());
        let valid = ValueChunk {
            remaining,
            next,
            data: &data,
        };
        let encoded = valid.encode(owner()).unwrap();
        let decoded = chunk(&encoded).unwrap();
        assert_eq!(decoded.remaining, remaining);
        assert!(decoded.next == next);
        assert_eq!(decoded.data, data);
        assert!(
            ValueChunk {
                data: &data[..data.len() - 1],
                ..valid
            }
            .encode(owner())
            .is_err()
        );
        assert!(
            ValueChunk {
                next: if next.is_some() {
                    None
                } else {
                    Some(location())
                },
                ..valid
            }
            .encode(owner())
            .is_err()
        );
    }
    for remaining in [0, logical::MAX_VALUE_BYTES as u32 + 1, u32::MAX] {
        assert!(
            ValueChunk {
                remaining,
                next: None,
                data: b""
            }
            .encode(owner())
            .is_err()
        );
    }
    let mut valid = ValueChunk {
        remaining: 3,
        next: None,
        data: b"abc",
    }
    .encode(owner())
    .unwrap();
    for length in [0_u16, 2, 4, u16::MAX] {
        valid[5..7].copy_from_slice(&length.to_be_bytes());
        assert!(chunk(&valid).is_err());
    }
}

#[test]
fn packed_tree_record_logical_identity_excludes_physical_layout_but_not_contents() {
    let TreeNode::Leaf {
        key,
        value,
        first_chunk,
    } = leaf()
    else {
        unreachable!()
    };
    let moved = PackedLocator {
        object: [77; 16],
        page: 99,
        slot: 127,
        epoch: KeyEpoch::new(99).unwrap(),
        writer: WriterIncarnationId::from_bytes([99; 16]),
        revision: CommitRevision::new(8).unwrap(),
    };
    let original = TreeNode::Leaf {
        key,
        value,
        first_chunk,
    };
    let relocated = TreeNode::Leaf {
        key,
        value,
        first_chunk: Some(moved),
    };
    assert_eq!(
        original.commitment(owner()).unwrap(),
        relocated.commitment(owner()).unwrap()
    );
    assert_ne!(
        original.encode(owner()).unwrap().as_slice(),
        relocated.encode(owner()).unwrap().as_slice()
    );
    let new_owner = PackedPageContext {
        object: [88; 16],
        page: 100,
        creation_revision: CommitRevision::new(10).unwrap(),
        ..owner()
    };
    assert_eq!(
        original.commitment(owner()).unwrap(),
        original.commitment(new_owner).unwrap()
    );
    let TreeNode::Branch { bit, left, right } = branch() else {
        unreachable!()
    };
    let relocated = TreeNode::Branch {
        bit,
        left: ChildReference {
            location: moved,
            ..left
        },
        right,
    };
    assert_eq!(
        branch().commitment(owner()).unwrap(),
        relocated.commitment(owner()).unwrap()
    );
    let changed = TreeNode::Leaf {
        key,
        value: logical::value_commitment(b"B").unwrap(),
        first_chunk,
    };
    assert_ne!(
        original.commitment(owner()).unwrap(),
        changed.commitment(owner()).unwrap()
    );
    assert_ne!(
        original.commitment(owner()).unwrap(),
        original
            .commitment(PackedPageContext {
                family: 6,
                ..owner()
            })
            .unwrap()
    );
}
