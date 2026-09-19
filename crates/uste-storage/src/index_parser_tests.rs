use super::*;

// Preserve the old allocation/adjacent-window validation as a test-only reference.
fn reference<'a>(
    bytes: &'a [u8],
    root: &RecoveredIndexRoot,
    run: &IndexRunDescriptor,
    page_index: u64,
) -> Result<&'a [u8], StorageError> {
    if bytes.len() != INDEX_PAGE_BYTES
        || &bytes[..4] != PAGE_MAGIC
        || bytes[4] != MAJOR
        || bytes[5] != MINOR
        || bytes[6] != run.family
        || bytes[7] != 0
        || read_u64(bytes, 8)? != root.revision.get()
        || read_u64(bytes, 16)? != page_index
        || read_array::<32>(bytes, 32)? != root.index_profile
        || read_array::<16>(bytes, 64)? != run.object_id
    {
        return Err(StorageError::IntegrityFailure);
    }
    let count = usize::try_from(read_u32(bytes, 24)?).unwrap();
    let used = usize::try_from(read_u32(bytes, 28)?).unwrap();
    if count == 0
        || !(PAGE_HEADER_BYTES..=INDEX_PAGE_BYTES).contains(&used)
        || bytes[used..].iter().any(|byte| *byte != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    let fragments = FragmentIter {
        remaining: &bytes[PAGE_HEADER_BYTES..used],
        remaining_count: count,
    }
    .collect::<Result<Vec<_>, _>>()?;
    if fragments.len() != count
        || fragments.windows(2).any(|pair| {
            pair[0].key > pair[1].key
                || (pair[0].key == pair[1].key && pair[0].offset >= pair[1].offset)
        })
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(fragments.last().unwrap().key)
}

fn fixture() -> (RecoveredIndexRoot, IndexRunDescriptor, Vec<u8>) {
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    );
    let run = IndexRunDescriptor {
        scope,
        revision: CommitRevision::FIRST,
        index_profile: [3; 32],
        epoch: KeyEpoch::new(1).unwrap(),
        writer: WriterIncarnationId::from_bytes([4; 16]),
        family: 2,
        object_id: [5; 16],
        page_count: 1,
        entry_count: 2,
        logical_digest: [6; 32],
    };
    let root = RecoveredIndexRoot {
        scope,
        revision: run.revision,
        generation: 1,
        certificate_digest: [7; 32],
        reducer_profile: [8; 32],
        logical_state_digest: [9; 32],
        index_profile: run.index_profile,
        runs: vec![run],
    };
    let mut bytes = new_page(
        run.revision,
        run.index_profile,
        run.family,
        run.object_id,
        0,
    );
    // Two fragments of one key, followed by an empty value at a later key.
    for (key, total, offset, value) in [
        (b'a', 2_u32, 0_u32, b"x".as_slice()),
        (b'a', 2, 1, b"y".as_slice()),
        (b'b', 0, 0, b"".as_slice()),
    ] {
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&total.to_be_bytes());
        bytes.extend_from_slice(&offset.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes.push(key);
        bytes.extend_from_slice(value);
    }
    let used = u32::try_from(bytes.len()).unwrap();
    bytes[24..28].copy_from_slice(&3_u32.to_be_bytes());
    bytes[28..32].copy_from_slice(&used.to_be_bytes());
    bytes.resize(INDEX_PAGE_BYTES, 0);
    (root, run, bytes)
}

#[test]
fn streamed_page_validation_matches_collecting_reference_and_all_byte_mutations() {
    let (root, run, mut bytes) = fixture();
    let parsed = ParsedPage::new(&bytes, &root, &run, 0).unwrap();
    assert_eq!(parsed.last_key(), b"b");
    assert_eq!(parsed.fragments().count(), 3);
    for length in 0..bytes.len() {
        assert!(ParsedPage::new(&bytes[..length], &root, &run, 0).is_err());
    }
    for offset in 0..bytes.len() {
        bytes[offset] ^= 0xff;
        assert_eq!(
            ParsedPage::new(&bytes, &root, &run, 0).map(|page| page.last_key()),
            reference(&bytes, &root, &run, 0),
            "byte {offset}"
        );
        bytes[offset] ^= 0xff;
    }
    for field in [24, 28, 80, 84, 88, 92, 98, 102, 106, 110] {
        let saved: [u8; 4] = bytes[field..field + 4].try_into().unwrap();
        for value in [
            0_u32,
            1,
            2,
            3,
            4,
            79,
            80,
            81,
            132,
            133,
            134,
            16_384,
            u32::MAX,
        ] {
            bytes[field..field + 4].copy_from_slice(&value.to_be_bytes());
            assert_eq!(
                ParsedPage::new(&bytes, &root, &run, 0).map(|page| page.last_key()),
                reference(&bytes, &root, &run, 0),
                "field {field} value {value}"
            );
        }
        bytes[field..field + 4].copy_from_slice(&saved);
    }
    // Same authenticated run identity cannot substitute another family or page ordinal.
    let mut foreign = run;
    foreign.family += 1;
    assert!(ParsedPage::new(&bytes, &root, &foreign, 0).is_err());
    assert!(ParsedPage::new(&bytes, &root, &run, 1).is_err());
    for (offset, value) in [(132, b'a'), (114, 0), (114, b'z')] {
        let saved = bytes[offset];
        bytes[offset] = value;
        assert!(ParsedPage::new(&bytes, &root, &run, 0).is_err());
        bytes[offset] = saved;
    }
}

#[test]
fn dense_fragment_page_is_fully_validated_including_its_tail() {
    let (root, run, _) = fixture();
    let mut bytes = new_page(
        run.revision,
        run.index_profile,
        run.family,
        run.object_id,
        0,
    );
    let count = u32::try_from((INDEX_PAGE_BYTES - PAGE_HEADER_BYTES) / 18).unwrap();
    for offset in 0..count {
        for field in [1_u32, count, offset, 1] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(b"kv");
    }
    let used = u32::try_from(bytes.len()).unwrap();
    bytes[24..28].copy_from_slice(&count.to_be_bytes());
    bytes[28..32].copy_from_slice(&used.to_be_bytes());
    bytes.resize(INDEX_PAGE_BYTES, 0);
    assert_eq!(
        ParsedPage::new(&bytes, &root, &run, 0).unwrap().last_key(),
        b"k"
    );
    assert_eq!(reference(&bytes, &root, &run, 0).unwrap(), b"k");
    for declared in [count - 1, count + 1, u32::MAX] {
        bytes[24..28].copy_from_slice(&declared.to_be_bytes());
        assert!(ParsedPage::new(&bytes, &root, &run, 0).is_err());
    }
    bytes[24..28].copy_from_slice(&count.to_be_bytes());
    bytes[28..32].copy_from_slice(&(used + 1).to_be_bytes());
    assert!(ParsedPage::new(&bytes, &root, &run, 0).is_err());
}
