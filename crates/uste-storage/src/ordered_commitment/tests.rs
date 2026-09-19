use super::*;
use std::collections::BTreeMap;
use uste_types::{DatabaseId, NamespaceId};

mod reference;
use reference::Tree;

fn context() -> CommitmentContext {
    CommitmentContext::new(
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        [3; 32],
        7,
    )
    .unwrap()
}
fn limits() -> CommitmentLimits {
    CommitmentLimits {
        maximum_branches: MAX_BRANCH_BITS,
        maximum_input_bytes: MAX_PROOF_INPUT_BYTES,
    }
}
fn hex(bytes: &str) -> [u8; 32] {
    std::array::from_fn(|i| u8::from_str_radix(&bytes[2 * i..2 * i + 2], 16).unwrap())
}
fn apply(
    map: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    root: &mut OrderedCommitment,
    key: &[u8],
    value: Option<&[u8]>,
) {
    let tree = Tree::build(context(), map);
    let (leaf, branches) = tree.proof(key);
    let proof = LookupProof {
        leaf,
        branches: &branches,
    };
    let before = map.get(key).map(Vec::as_slice);
    assert!(
        verify_lookup(context(), *root, key, proof, limits()).unwrap()
            == before.map(value_commitment).transpose().unwrap()
    );
    let next = apply_delta(context(), *root, key, before, value, proof, limits()).unwrap();
    match value {
        Some(value) => {
            map.insert(key.to_vec(), value.to_vec());
        }
        None => {
            map.remove(key);
        }
    }
    assert_eq!(next, Tree::build(context(), map).root());
    *root = next;
}

#[test]
fn ordered_commitment_matches_independent_python_sha256_goldens() {
    assert_eq!(
        *empty_commitment(context()).digest(),
        hex("20e05dc475e75384475922992db42c0640e801f16de9b442b01a62ae5082373c")
    );
    let value = value_commitment(b"A").unwrap();
    assert_eq!(
        value.digest,
        hex("df25c5122e9028e790f47ff010b032be55738b6fbc9c4689735272368c0fb837")
    );
    let leaf = leaf_commitment(context(), LeafProof { key: b"a", value }).unwrap();
    assert_eq!(
        *leaf.digest(),
        hex("cf793bc0171193181684944a12a513ee923789274d402fcc319d1301f4168f8e")
    );
    let mut map = BTreeMap::new();
    let mut root = empty_commitment(context());
    for (key, value) in [
        (b"a".as_slice(), b"A".as_slice()),
        (b"ab", b"BC"),
        (&[0], b""),
        (&[255], b"z"),
    ] {
        apply(&mut map, &mut root, key, Some(value));
    }
    assert_eq!((root.entries(), root.logical_bytes()), (4, 9));
    assert_eq!(
        *root.digest(),
        hex("0cd0830cfc9b147c5473d7e116e5e3dec6661fededa3763bdfedd4a0c9ffdc87")
    );
}

fn permutations() -> Vec<[usize; 4]> {
    let mut result = Vec::new();
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    if a != b && a != c && a != d && b != c && b != d && c != d {
                        result.push([a, b, c, d]);
                    }
                }
            }
        }
    }
    result
}

#[test]
fn ordered_commitment_key_bits_match_every_byte_pair_and_prefix_terminators() {
    for a in 0..=255_u8 {
        for b in 0..=255_u8 {
            let expected = (0..8)
                .find(|shift| (a >> (7 - shift)) & 1 != (b >> (7 - shift)) & 1)
                .map(|n| n + 1);
            assert_eq!(first_difference(&[a], &[b]), expected);
            if let Some(position) = expected {
                assert_eq!(bit(&[a], position), a > b);
                assert_eq!(bit(&[b], position), b > a);
            }
        }
        assert_eq!(first_difference(&[a], &[a, 0]), Some(9));
        assert!(!bit(&[a], 9));
        assert!(bit(&[a, 0], 9));
    }
}

#[test]
fn ordered_commitment_all_small_insert_delete_orders_are_canonical() {
    let keys: [&[u8]; 4] = [&[0], b"a", b"ab", &[255]];
    for insert in permutations() {
        for delete in permutations() {
            let mut map = BTreeMap::new();
            let mut root = empty_commitment(context());
            for i in insert {
                apply(&mut map, &mut root, keys[i], Some(&[i as u8]));
            }
            for i in delete {
                apply(&mut map, &mut root, keys[i], None);
            }
            assert_eq!(root, empty_commitment(context()));
        }
    }
}

#[test]
fn ordered_commitment_generated_updates_prefixes_and_absence_match_sorted_rebuilds() {
    let keys: Vec<Vec<u8>> = (0..96)
        .map(|n| match n % 4 {
            0 => vec![n as u8],
            1 => vec![0, n as u8],
            2 => vec![0, n as u8, 255],
            _ => vec![255, n as u8, 0],
        })
        .collect();
    let mut map = BTreeMap::new();
    let mut root = empty_commitment(context());
    let mut seed = 117_u64;
    for step in 0..1024_u64 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let key = &keys[(seed >> 32) as usize % keys.len()];
        let value = step.to_be_bytes();
        apply(
            &mut map,
            &mut root,
            key,
            (seed & 3 != 0).then_some(value.as_slice()),
        );
        if step % 31 == 0 {
            let tree = Tree::build(context(), &map);
            for key in &keys {
                let (leaf, branches) = tree.proof(key);
                assert!(
                    verify_lookup(
                        context(),
                        root,
                        key,
                        LookupProof {
                            leaf,
                            branches: &branches
                        },
                        limits()
                    )
                    .unwrap()
                        == map.get(key).map(|value| value_commitment(value).unwrap())
                );
            }
        }
    }
}

#[test]
fn ordered_commitment_proof_mutation_context_and_conflicts_are_atomic() {
    let map: BTreeMap<_, _> = [
        (b"a".to_vec(), b"A".to_vec()),
        (b"ab".to_vec(), b"B".to_vec()),
        (b"b".to_vec(), b"C".to_vec()),
    ]
    .into_iter()
    .collect();
    let tree = Tree::build(context(), &map);
    let root = tree.root();
    let (leaf, branches) = tree.proof(b"a");
    assert!(branches.len() >= 2);
    let proof = LookupProof {
        leaf,
        branches: &branches,
    };
    for before in [None, Some(b"wrong".as_slice())] {
        assert_eq!(
            apply_delta(context(), root, b"a", before, Some(b"Z"), proof, limits()),
            Err(CommitmentError::Conflict)
        );
    }
    for variant in 0..4 {
        let mut altered = context();
        match variant {
            0 => altered.family += 1,
            1 => altered.profile[0] ^= 1,
            2 => {
                altered.scope =
                    NamespaceRef::new(DatabaseId::from_bytes([9; 16]), altered.scope.namespace())
            }
            _ => {
                altered.scope =
                    NamespaceRef::new(altered.scope.database(), NamespaceId::from_bytes([9; 16]))
            }
        }
        assert!(matches!(
            verify_lookup(altered, root, b"a", proof, limits()),
            Err(CommitmentError::InvalidProof)
        ));
    }
    for index in 0..branches.len() {
        for variant in 0..4 {
            let mut bad = branches.clone();
            match variant {
                0 => bad[index].bit ^= 1,
                1 => bad[index].sibling.digest[0] ^= 1,
                2 => bad[index].sibling.entries += 1,
                _ => bad[index].sibling.logical_bytes += 1,
            }
            assert!(
                verify_lookup(
                    context(),
                    root,
                    b"a",
                    LookupProof {
                        leaf,
                        branches: &bad
                    },
                    limits()
                )
                .is_err()
            );
        }
    }
    let mut bad_leaf = leaf.unwrap();
    bad_leaf.value.digest[0] ^= 1;
    assert!(
        verify_lookup(
            context(),
            root,
            b"a",
            LookupProof {
                leaf: Some(bad_leaf),
                branches: &branches
            },
            limits()
        )
        .is_err()
    );
    let mut duplicate = branches.clone();
    duplicate[1].bit = duplicate[0].bit;
    assert!(
        verify_lookup(
            context(),
            root,
            b"a",
            LookupProof {
                leaf,
                branches: &duplicate
            },
            limits()
        )
        .is_err()
    );
    let mut reversed = branches.clone();
    reversed.reverse();
    assert!(
        verify_lookup(
            context(),
            root,
            b"a",
            LookupProof {
                leaf,
                branches: &reversed
            },
            limits()
        )
        .is_err()
    );
    assert!(
        verify_lookup(
            context(),
            root,
            b"a",
            LookupProof {
                leaf,
                branches: &branches[1..]
            },
            limits()
        )
        .is_err()
    );
    assert!(
        verify_lookup(
            context(),
            root,
            b"a",
            LookupProof {
                leaf: None,
                branches: &[]
            },
            limits()
        )
        .is_err()
    );
    assert_eq!(root, Tree::build(context(), &map).root());
    assert!(!format!("{root:?}").contains("0cd083"));
    assert!(format!("{root:?}").contains("REDACTED"));
}

#[test]
fn ordered_commitment_exact_work_and_size_limits_preserve_the_before_root() {
    let map: BTreeMap<_, _> = [
        (b"a".to_vec(), b"A".to_vec()),
        (b"b".to_vec(), b"B".to_vec()),
    ]
    .into_iter()
    .collect();
    let tree = Tree::build(context(), &map);
    let (leaf, branches) = tree.proof(b"a");
    let proof = LookupProof {
        leaf,
        branches: &branches,
    };
    assert_eq!(branches.len(), 1);
    let exact = CommitmentLimits {
        maximum_branches: 1,
        maximum_input_bytes: 96,
    };
    apply_delta(
        context(),
        tree.root(),
        b"a",
        Some(b"A"),
        Some(b"Z"),
        proof,
        exact,
    )
    .unwrap();
    for short in [
        CommitmentLimits {
            maximum_input_bytes: 95,
            ..exact
        },
        CommitmentLimits {
            maximum_branches: 0,
            ..exact
        },
    ] {
        assert_eq!(
            apply_delta(
                context(),
                tree.root(),
                b"a",
                Some(b"A"),
                Some(b"Z"),
                proof,
                short
            ),
            Err(CommitmentError::ResourceLimit)
        );
    }
    let key = vec![255; MAX_KEY_BYTES];
    let mut shorter = key.clone();
    shorter.pop();
    let mut max_map = BTreeMap::new();
    let mut root = empty_commitment(context());
    apply(&mut max_map, &mut root, &key, Some(b""));
    apply(&mut max_map, &mut root, &shorter, Some(b""));
    let empty = LookupProof {
        leaf: None,
        branches: &[],
    };
    assert!(
        apply_delta(
            context(),
            root,
            &vec![0; MAX_KEY_BYTES + 1],
            None,
            Some(b""),
            empty,
            limits()
        )
        .is_err()
    );
    assert!(apply_delta(context(), root, b"", None, Some(b""), empty, limits()).is_err());
    let value = vec![0; MAX_VALUE_BYTES + 1];
    assert_eq!(
        value_commitment(&value).err(),
        Some(CommitmentError::ResourceLimit)
    );
    assert_eq!(
        value_commitment(&value[..MAX_VALUE_BYTES]).unwrap().length,
        MAX_VALUE_BYTES as u64
    );
    let leaf = leaf_commitment(
        context(),
        LeafProof {
            key: b"a",
            value: value_commitment(b"").unwrap(),
        },
    )
    .unwrap();
    assert!(branch_commitment(context(), MAX_BRANCH_BITS, leaf, leaf).is_err());
    assert!(branch_commitment(context(), 1, empty_commitment(context()), leaf).is_err());
    assert_eq!(
        branch_commitment(
            context(),
            1,
            OrderedCommitment {
                entries: MAX_ENTRIES,
                ..leaf
            },
            leaf
        ),
        Err(CommitmentError::ResourceLimit)
    );
    assert_eq!(
        branch_commitment(
            context(),
            1,
            OrderedCommitment {
                logical_bytes: u64::MAX,
                ..leaf
            },
            leaf
        ),
        Err(CommitmentError::ResourceLimit)
    );
}
