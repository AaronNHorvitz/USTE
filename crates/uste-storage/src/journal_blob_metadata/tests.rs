use super::*;

#[test]
fn blob_metadata_profile_and_closed_codec_shapes() {
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(b"USTE storage-blob-meta-v1")),
        BLOB_METADATA_PROFILE_V1
    );
    let counts = BlobMetadataCounts {
        blobs: 1,
        namespaces: 1,
        inventories: 2,
        reference_bindings: 3,
    };
    let bytes = encode_counts(counts);
    assert_eq!(&bytes[..8], b"SBMD\x01\0\0\0");
    assert_eq!(&bytes[8..16], &1_u64.to_be_bytes());
    assert_eq!(&bytes[24..32], &2_u64.to_be_bytes());
    assert_eq!(decode_counts(&bytes).unwrap(), counts);
    for length in 0..bytes.len() {
        assert!(decode_counts(&bytes[..length]).is_err());
    }
    for offset in [0, 4, 5, 6, 7, 40, 47] {
        let mut bad = bytes;
        bad[offset] ^= 1;
        assert!(decode_counts(&bad).is_err());
    }
    let scope = metadata_scope(DatabaseId::from_bytes([1; 16]));
    let reference = BlobReference::new(
        scope,
        crate::blob::BlobId::from_bytes([2; 16]),
        0,
        0,
        Sha256::digest(b"").into(),
    )
    .unwrap();
    let key = reference_key(reference);
    let value = encode_reference(reference, CommitRevision::FIRST);
    assert_eq!(
        decode_reference(scope.database(), CommitRevision::FIRST, &key, &value).unwrap(),
        (reference, CommitRevision::FIRST)
    );
    for length in 0..value.len() {
        assert!(
            decode_reference(
                scope.database(),
                CommitRevision::FIRST,
                &key,
                &value[..length]
            )
            .is_err()
        );
    }
    for offset in [7, 20, 23] {
        let mut bad = value;
        bad[offset] ^= 1;
        assert!(decode_reference(scope.database(), CommitRevision::FIRST, &key, &bad).is_err());
    }
    let value = encode_inventory(2, CommitRevision::FIRST).unwrap();
    assert_eq!(
        decode_inventory(CommitRevision::FIRST, &[1; 32], &value).unwrap(),
        (2, CommitRevision::FIRST)
    );
    for length in 0..value.len() {
        assert!(decode_inventory(CommitRevision::FIRST, &[1; 32], &value[..length]).is_err());
    }
    assert!(decode_inventory(CommitRevision::FIRST, &EMPTY_BLOB_INVENTORY_DIGEST, &value).is_err());
    assert!(
        decode_inventory(
            CommitRevision::FIRST,
            &[1; 32],
            &encode_inventory(0, CommitRevision::FIRST).unwrap()
        )
        .is_err()
    );
    assert!(
        decode_inventory(
            CommitRevision::FIRST,
            &[1; 32],
            &encode_inventory(1, CommitRevision::new(2).unwrap()).unwrap()
        )
        .is_err()
    );
}
