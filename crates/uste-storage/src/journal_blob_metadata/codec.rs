use super::*;

pub(super) fn encode_counts(counts: BlobMetadataCounts) -> [u8; 48] {
    let mut bytes = [0; 48];
    bytes[..4].copy_from_slice(b"SBMD");
    bytes[4] = 1;
    for (offset, value) in [
        (8, counts.blobs),
        (16, counts.namespaces),
        (24, counts.inventories),
        (32, counts.reference_bindings),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

pub(super) fn decode_counts(bytes: &[u8]) -> Result<BlobMetadataCounts, StorageError> {
    if bytes.len() != 48
        || &bytes[..4] != b"SBMD"
        || bytes[4..8] != [1, 0, 0, 0]
        || bytes[40..].iter().any(|byte| *byte != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(BlobMetadataCounts {
        blobs: read_u64(bytes, 8)?,
        namespaces: read_u64(bytes, 16)?,
        inventories: read_u64(bytes, 24)?,
        reference_bindings: read_u64(bytes, 32)?,
    })
}

pub(super) fn reference_key(reference: BlobReference) -> [u8; 32] {
    let mut key = [0; 32];
    key[..16].copy_from_slice(reference.scope().namespace().as_bytes());
    key[16..].copy_from_slice(&reference.id().as_bytes());
    key
}

pub(super) fn encode_reference(reference: BlobReference, first: CommitRevision) -> [u8; 56] {
    let mut value = [0; 56];
    value[..8].copy_from_slice(&first.get().to_be_bytes());
    value[8..16].copy_from_slice(&reference.byte_len().to_be_bytes());
    value[16..20].copy_from_slice(&reference.chunk_count().to_be_bytes());
    value[24..].copy_from_slice(&reference.content_digest());
    value
}

pub(super) fn decode_reference(
    database: DatabaseId,
    frontier: CommitRevision,
    key: &[u8],
    value: &[u8],
) -> Result<(BlobReference, CommitRevision), StorageError> {
    if key.len() != 32 || value.len() != 56 || value[20..24] != [0; 4] {
        return Err(StorageError::IntegrityFailure);
    }
    let first =
        CommitRevision::new(read_u64(value, 0)?).map_err(|_| StorageError::IntegrityFailure)?;
    if first > frontier {
        return Err(StorageError::IntegrityFailure);
    }
    let reference = BlobReference::new(
        NamespaceRef::new(
            database,
            uste_types::NamespaceId::from_bytes(read_array(key, 0)?),
        ),
        crate::blob::BlobId::from_bytes(read_array(key, 16)?),
        read_u64(value, 8)?,
        u32::from_be_bytes(read_array(value, 16)?),
        read_array(value, 24)?,
    )
    .map_err(|_| StorageError::IntegrityFailure)?;
    Ok((reference, first))
}

pub(super) fn encode_inventory(
    count: usize,
    first: CommitRevision,
) -> Result<[u8; 16], StorageError> {
    let mut value = [0; 16];
    value[..4].copy_from_slice(
        &u32::try_from(count)
            .map_err(|_| StorageError::ResourceLimit)?
            .to_be_bytes(),
    );
    value[8..].copy_from_slice(&first.get().to_be_bytes());
    Ok(value)
}

pub(super) fn decode_inventory(
    frontier: CommitRevision,
    key: &[u8],
    value: &[u8],
) -> Result<(usize, CommitRevision), StorageError> {
    if key.len() != 32
        || key == EMPTY_BLOB_INVENTORY_DIGEST
        || value.len() != 16
        || value[4..8] != [0; 4]
    {
        return Err(StorageError::IntegrityFailure);
    }
    let count = u32::from_be_bytes(read_array(value, 0)?) as usize;
    let first =
        CommitRevision::new(read_u64(value, 8)?).map_err(|_| StorageError::IntegrityFailure)?;
    if count == 0 || count > crate::blob::MAX_BLOBS_PER_INVENTORY || first > frontier {
        return Err(StorageError::IntegrityFailure);
    }
    Ok((count, first))
}

pub(super) fn decode_namespace(
    database: DatabaseId,
    key: &[u8],
    value: &[u8],
) -> Result<(NamespaceRef, u64), StorageError> {
    if key.len() != 16 || value.len() != 8 {
        return Err(StorageError::IntegrityFailure);
    }
    let bytes = read_u64(value, 0)?;
    if bytes > MAX_NAMESPACE_BLOB_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    Ok((
        NamespaceRef::new(
            database,
            uste_types::NamespaceId::from_bytes(read_array(key, 0)?),
        ),
        bytes,
    ))
}
