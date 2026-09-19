use super::*;

#[test]
fn sparse_point_reads_match_reference_with_dense_pages_large_values_and_eviction() {
    let database = DatabaseId::from_bytes([0xa6; 16]);
    let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([1; 16]));
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut store = JournalStore::create(
        &mut fs,
        options(database, "sparse-point-reads"),
        create_vault(database, 106_000),
        CounterEntropy::new(107_000),
    )
    .unwrap();
    let commit = store
        .append_group(
            &mut fs,
            CommitInput {
                encoded_group: b"sparse point fixture",
                logical_event_digest: [0xa6; 32],
            },
        )
        .unwrap();
    let large = vec![0xA6; 2 * index::INDEX_PAGE_BYTES + 137];
    let expected = (0_u16..900)
        .map(|ordinal| {
            let value = if ordinal == 400 {
                large.clone()
            } else if ordinal.is_multiple_of(3) {
                Vec::new()
            } else {
                ordinal.to_be_bytes().to_vec()
            };
            ((ordinal * 2).to_be_bytes().to_vec(), value)
        })
        .collect::<BTreeMap<_, _>>();
    let profile = [0xa7; 32];
    let run = store
        .publish_index_run(
            &mut fs,
            scope,
            commit.revision,
            profile,
            1,
            expected.iter().map(|(key, value)| IndexEntry {
                key: key.clone(),
                value: value.clone(),
            }),
        )
        .unwrap();
    store
        .publish_index_root(
            &mut fs,
            IndexRootInput {
                scope,
                revision: commit.revision,
                certificate_digest: commit.certificate_digest,
                reducer_profile: [0xa8; 32],
                logical_state_digest: [0xa9; 32],
                index_profile: profile,
            },
            &[run],
        )
        .unwrap();
    let root = store
        .load_index_roots(&mut fs, scope, profile)
        .unwrap()
        .remove(0);
    for budget in [index::MIN_INDEX_CACHE_BYTES, 64 * 1024] {
        let mut cache = PageCache::new(budget).unwrap();
        for ordinal in 0_u16..=1800 {
            let key = ordinal.to_be_bytes();
            for _ in 0..2 {
                let (actual, stats) = store
                    .index_get_bounded(
                        &mut fs,
                        &root,
                        1,
                        &key,
                        IndexGetLimits::new(16, large.len()).unwrap(),
                        &mut cache,
                    )
                    .unwrap();
                assert_eq!(actual.as_ref(), expected.get(key.as_slice()));
                assert_eq!(
                    stats.result_bytes,
                    actual.as_ref().map_or(0, |value| value.len() as u64)
                );
                assert!(cache.accounted_bytes() <= budget);
            }
        }
        if budget == index::MIN_INDEX_CACHE_BYTES {
            assert!(cache.evictions() > 0);
        }
        assert!(matches!(
            store.index_get_bounded(
                &mut fs,
                &root,
                1,
                &800_u16.to_be_bytes(),
                IndexGetLimits::new(16, large.len() - 1).unwrap(),
                &mut cache
            ),
            Err(StorageError::ResourceLimit)
        ));
        cache.clear();
        assert!(matches!(
            store.index_get_bounded(
                &mut fs,
                &root,
                1,
                &800_u16.to_be_bytes(),
                IndexGetLimits::new(1, large.len()).unwrap(),
                &mut cache
            ),
            Err(StorageError::ResourceLimit)
        ));
    }
}
