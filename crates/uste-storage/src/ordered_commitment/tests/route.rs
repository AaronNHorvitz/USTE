use super::*;

#[test]
fn authenticated_chain_route_matches_independent_tree_and_full_hash_proof() {
    let mut map = BTreeMap::new();
    for byte in (0..=255_u8).step_by(2) {
        map.insert(vec![byte], vec![byte; 3]);
        map.insert(vec![byte, 0], vec![]);
    }
    let tree = Tree::build(context(), &map);
    for byte in 0..=255_u8 {
        for key in [vec![byte], vec![byte, 0], vec![byte, 1]] {
            let (leaf, branches) = tree.proof(&key);
            let leaf = leaf.unwrap();
            let routed = validate_lookup_route(&key, leaf, &branches, limits())
                .map(|()| (leaf.key == key).then_some(leaf.value));
            let verified = verify_lookup(
                context(),
                tree.root(),
                &key,
                LookupProof {
                    leaf: Some(leaf),
                    branches: &branches,
                },
                limits(),
            );
            assert!(routed == verified);
            assert!(routed.unwrap() == map.get(&key).map(|v| value_commitment(v).unwrap()));
        }
    }
}

#[test]
fn authenticated_chain_route_preserves_exact_admission_and_noncanonical_path_refusal() {
    let map = [b"a", b"b", b"c", b"d"]
        .into_iter()
        .map(|key| (key.to_vec(), key.to_vec()))
        .collect();
    let tree = Tree::build(context(), &map);
    let (leaf, branches) = tree.proof(b"a");
    let leaf = leaf.unwrap();
    assert!(branches.len() >= 2);
    let exact = CommitmentLimits {
        maximum_branches: branches.len() as u32,
        maximum_input_bytes: 1 + leaf.key.len() as u64 + 40 + branches.len() as u64 * 52,
    };
    assert!(validate_lookup_route(b"a", leaf, &branches, exact).is_ok());
    for short in [
        CommitmentLimits {
            maximum_branches: exact.maximum_branches - 1,
            ..exact
        },
        CommitmentLimits {
            maximum_input_bytes: exact.maximum_input_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            validate_lookup_route(b"a", leaf, &branches, short)
                == Err(CommitmentError::ResourceLimit)
        );
    }
    for variant in 0..3 {
        let mut bad = branches.clone();
        match variant {
            0 => bad[1].bit = bad[0].bit,
            1 => bad[0].bit = leaf.key.len() as u32 * 9 + 1,
            _ => bad.reverse(),
        }
        assert!(
            validate_lookup_route(b"a", leaf, &bad, limits()) == Err(CommitmentError::InvalidProof)
        );
    }
    assert!(
        validate_lookup_route(b"z", leaf, &branches, limits())
            == Err(CommitmentError::InvalidProof)
    );
    assert!(
        validate_lookup_route(b"", leaf, &branches, limits()) == Err(CommitmentError::InvalidProof)
    );
    assert!(
        validate_lookup_route(b"a", LeafProof { key: b"", ..leaf }, &branches, limits())
            == Err(CommitmentError::InvalidProof)
    );
    let oversized = LeafProof {
        value: ValueCommitment {
            length: MAX_VALUE_BYTES as u64 + 1,
            ..leaf.value
        },
        ..leaf
    };
    assert!(
        validate_lookup_route(b"a", oversized, &branches, limits())
            == Err(CommitmentError::ResourceLimit)
    );
}

#[test]
fn structural_route_is_not_authority_for_an_unbound_root_or_sibling() {
    let map = [b"a", b"b"]
        .into_iter()
        .map(|k| (k.to_vec(), k.to_vec()))
        .collect();
    let tree = Tree::build(context(), &map);
    let (leaf, mut branches) = tree.proof(b"a");
    let leaf = leaf.unwrap();
    branches[0].sibling.digest[0] ^= 1;
    // Deliberately succeeds structurally. Production may only use this helper
    // after the page/node chain has authenticated the exact sibling and leaf.
    assert!(validate_lookup_route(b"a", leaf, &branches, limits()).is_ok());
    assert!(
        verify_lookup(
            context(),
            tree.root(),
            b"a",
            LookupProof {
                leaf: Some(leaf),
                branches: &branches,
            },
            limits()
        ) == Err(CommitmentError::InvalidProof)
    );
}
