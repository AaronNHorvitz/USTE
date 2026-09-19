use super::*;

fn rooted() -> (Fixture, IndexRootInput, RecoveredIndexRoot) {
    let mut f = Fixture::new(false);
    let commit = f.commits[2];
    let input = IndexRootInput {
        scope: NamespaceRef::new(
            f.store.database,
            uste_types::NamespaceId::from_bytes([7; 16]),
        ),
        revision: commit.revision,
        certificate_digest: commit.certificate_digest,
        reducer_profile: [2; 32],
        logical_state_digest: [3; 32],
        index_profile: [4; 32],
    };
    let run = f
        .store
        .publish_index_run(
            &mut f.fs,
            input.scope,
            input.revision,
            input.index_profile,
            1,
            [
                IndexEntry {
                    key: b"ka".to_vec(),
                    value: b"one".to_vec(),
                },
                IndexEntry {
                    key: b"kb".to_vec(),
                    value: b"two".to_vec(),
                },
            ],
        )
        .unwrap();
    let root = f
        .store
        .publish_index_root_recovered(&mut f.fs, input, &[run])
        .unwrap();
    (f, input, root)
}

fn limits() -> CertificateAnchorReadLimits {
    CertificateAnchorReadLimits::new(4, 4 * SMALL_ENVELOPE_BYTES).unwrap()
}
fn read() -> IndexRunReadLimits {
    IndexRunReadLimits::new(8, 8, 4096).unwrap()
}
fn merge_limits() -> IndexRunMergeLimits {
    IndexRunMergeLimits::new(read(), 8, 4096, 8, 4096).unwrap()
}

#[test]
fn proof_bound_roots_support_every_existing_read_and_merge_without_history_map() {
    let (mut f, mut input, unbound) = rooted();
    let proof = f.proof(2).unwrap();
    let root = f
        .store
        .bind_index_root_certificate(unbound.clone(), proof.clone())
        .unwrap();
    assert_eq!(root, unbound);
    let next = f
        .store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"four",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    f.store.certificate_anchors.clear();
    let mut cache = PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap();
    assert!(
        f.store
            .index_get(&mut f.fs, &unbound, 1, b"ka", &mut cache)
            .is_err()
    );
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &root, 1, b"ka", &mut cache)
            .unwrap()
            .0,
        Some(b"one".to_vec())
    );
    assert_eq!(
        f.store
            .index_get_bounded(
                &mut f.fs,
                &root,
                1,
                b"kb",
                IndexGetLimits::new(8, 4096).unwrap(),
                &mut cache
            )
            .unwrap()
            .0,
        Some(b"two".to_vec())
    );
    assert_eq!(
        f.store
            .index_get_predecessor(
                &mut f.fs,
                &root,
                1,
                b"k",
                b"kz",
                IndexPredecessorLimits::new(8, 4096).unwrap(),
                &mut cache
            )
            .unwrap()
            .entry
            .unwrap()
            .key,
        b"kb"
    );
    assert_eq!(
        f.store
            .index_scan_prefix(&mut f.fs, &root, 1, b"k", 8, 4096, &mut cache)
            .unwrap()
            .entries
            .len(),
        2
    );
    assert_eq!(
        f.store
            .index_scan_prefix_bounded(
                &mut f.fs,
                &root,
                1,
                b"k",
                IndexScanLimits::new(8, 8, 4096).unwrap(),
                &mut cache
            )
            .unwrap()
            .entries
            .len(),
        2
    );
    let mut visited = Vec::new();
    f.store
        .index_scan_prefix_visit(
            &mut f.fs,
            &root,
            1,
            b"k",
            8,
            4096,
            &mut cache,
            &mut |entry| {
                visited.push(entry.key);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(visited, [b"ka", b"kb"]);
    assert_eq!(
        f.store
            .visit_index_run(&mut f.fs, &root, 1, read(), &mut |_, _| Ok(()))
            .unwrap()
            .entries,
        2
    );
    let mut cursor = f
        .store
        .open_index_run_cursor(&mut f.fs, &root, 1, read())
        .unwrap();
    let mut count = 0;
    while f
        .store
        .next_index_run_entry(&mut f.fs, &mut cursor)
        .unwrap()
        .is_some()
    {
        count += 1;
    }
    assert_eq!(count, 2);
    assert_eq!(f.store.finish_index_run_cursor(cursor).unwrap().entries, 2);
    f.store
        .scrub_index_root(&mut f.fs, &root, &mut cache)
        .unwrap();
    f.store
        .resync_index_root_bounded(&mut f.fs, &root, read())
        .unwrap();
    let (roots, report) = f
        .store
        .load_proven_index_root_manifests(&mut f.fs, input.scope, input.index_profile, limits())
        .unwrap();
    assert_eq!(roots.as_slice(), std::slice::from_ref(&root));
    assert_eq!(report.certificates, 2);
    assert_eq!(report.encoded_bytes, 2 * SMALL_ENVELOPE_BYTES);
    let delta = || {
        [IndexDelta::new(
            b"kc".to_vec(),
            None,
            Some(b"three".to_vec()),
        )]
    };
    let direct = f
        .store
        .merge_index_run(
            &mut f.fs,
            input.scope,
            next.revision,
            input.index_profile,
            1,
            Some(&root),
            merge_limits(),
            delta(),
        )
        .unwrap()
        .run
        .unwrap();
    let streamed = f
        .store
        .merge_index_run_visit(
            &mut f.fs,
            input.scope,
            next.revision,
            input.index_profile,
            1,
            Some(&root),
            merge_limits(),
            delta(),
            &mut |_, _| Ok(()),
        )
        .unwrap()
        .run
        .unwrap();
    assert_eq!(direct.logical_digest(), streamed.logical_digest());
    input.revision = next.revision;
    input.certificate_digest = next.certificate_digest;
    let proof = f
        .store
        .authenticate_certificate_anchor(
            &mut f.fs,
            next.revision,
            next.certificate_digest,
            limits(),
        )
        .unwrap();
    let stage = f
        .store
        .open_proven_index_recovery_stage(input.scope, &proof)
        .unwrap();
    let staged = f
        .store
        .stage_index_merge_visit(
            &mut f.fs,
            &stage,
            input.index_profile,
            1,
            Some(&root),
            merge_limits(),
            delta(),
            &mut |_, _| Ok(()),
        )
        .unwrap()
        .run
        .unwrap();
    assert_eq!(direct.logical_digest(), staged.logical_digest());
    f.store
        .finish_index_recovery_stage(stage, input, &[staged])
        .unwrap();
    f.store
        .publish_index_root_recovered(&mut f.fs, input, &[direct])
        .unwrap();
    let (roots, report) = f
        .store
        .load_proven_index_root_manifests(&mut f.fs, input.scope, input.index_profile, limits())
        .unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(report.certificates, 3);
    assert_eq!(roots[1], root);
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &roots[0], 1, b"kc", &mut cache)
            .unwrap()
            .0,
        Some(b"three".to_vec())
    );
    assert!(f.store.certificate_anchors.is_empty());
    assert!(matches!(
        f.store.load_proven_index_root_manifests(
            &mut f.fs,
            input.scope,
            input.index_profile,
            CertificateAnchorReadLimits::new(1, SMALL_ENVELOPE_BYTES).unwrap()
        ),
        Err(StorageError::ResourceLimit)
    ));
}

#[test]
fn proof_bound_roots_reject_other_owner_even_with_matching_resident_anchor() {
    let (mut f, input, root) = rooted();
    let proof = f.proof(2).unwrap();
    let bound = f
        .store
        .bind_index_root_certificate(root.clone(), proof)
        .unwrap();
    drop(f.store);
    let (store, _) = JournalStore::open(
        &mut f.fs,
        &entry("certificate-proof"),
        input.scope.database(),
        CounterEntropy::new(942_000),
        CounterEntropy::new(943_000),
        &mut TestKeyAdapter,
        |_| Ok(()),
    )
    .unwrap();
    f.store = store;
    f.fs.arm(FaultPlan::default()).unwrap();
    let mut cache = PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap();
    assert!(
        f.store
            .index_get(&mut f.fs, &bound, 1, b"ka", &mut cache)
            .is_err()
    );
    assert!(
        f.store
            .open_index_run_cursor(&mut f.fs, &bound, 1, read())
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    // Legacy unbound handles retain their map-based behavior, and fresh proof admission works.
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &root, 1, b"ka", &mut cache)
            .unwrap()
            .0,
        Some(b"one".to_vec())
    );
    let (fresh, _) = f
        .store
        .load_proven_index_root_manifests(&mut f.fs, input.scope, input.index_profile, limits())
        .unwrap();
    assert_eq!(fresh[0], bound);
    f.store.certificate_anchors.clear();
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &fresh[0], 1, b"ka", &mut cache)
            .unwrap()
            .0,
        Some(b"one".to_vec())
    );
}

#[test]
fn proof_bound_root_discovery_read_faults_and_certificate_corruption_fail_closed() {
    let (mut baseline, input, _) = rooted();
    baseline.store.certificate_anchors.clear();
    baseline.fs.arm(FaultPlan::default()).unwrap();
    baseline
        .store
        .load_proven_index_root_manifests(
            &mut baseline.fs,
            input.scope,
            input.index_profile,
            limits(),
        )
        .unwrap();
    let reads = baseline.fs.operation_count(Operation::ReadAt);
    assert!(reads >= 3);
    eprintln!(
        "proof_bound_root_discovery read_boundaries={reads} fault_cases={}",
        reads * 3
    );
    for occurrence in 1..=reads {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let (mut f, input, expected) = rooted();
            f.store.certificate_anchors.clear();
            f.fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::ReadAt,
                    occurrence,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            assert!(
                f.store
                    .load_proven_index_root_manifests(
                        &mut f.fs,
                        input.scope,
                        input.index_profile,
                        limits()
                    )
                    .is_err()
            );
            assert_eq!(f.fs.pending_faults(), 0);
            drop(f.store);
            f.fs.restart().unwrap();
            let (store, _) = JournalStore::open(
                &mut f.fs,
                &entry("certificate-proof"),
                input.scope.database(),
                CounterEntropy::new(942_000),
                CounterEntropy::new(943_000),
                &mut TestKeyAdapter,
                |_| Ok(()),
            )
            .unwrap();
            f.store = store;
            f.store.certificate_anchors.clear();
            let (roots, _) = f
                .store
                .load_proven_index_root_manifests(
                    &mut f.fs,
                    input.scope,
                    input.index_profile,
                    limits(),
                )
                .unwrap();
            assert_eq!(roots, [expected]);
        }
    }
    let (mut f, input, _) = rooted();
    let offset = 3 * SMALL_ENVELOPE_BYTES;
    let mut encoded = read_bounded(
        &mut f.fs,
        &f.store.certificate_file,
        offset,
        SMALL_ENVELOPE_BYTES,
    )
    .unwrap();
    encoded[100] ^= 1;
    write_all_at(&mut f.fs, &f.store.certificate_file, offset, &encoded).unwrap();
    assert!(
        f.store
            .load_proven_index_root_manifests(&mut f.fs, input.scope, input.index_profile, limits())
            .is_err()
    );
}

#[test]
fn disk_certificate_mode_never_retains_anchors_and_counts_range_proof_io() {
    let (mut f, mut input, _) = rooted();
    assert_eq!(f.store.certificate_anchors.len(), 3);
    drop(f.store);
    f.fs.restart().unwrap();
    let mut replayed = Vec::new();
    let (store, report) = JournalStore::open_with_disk_certificate_anchors(
        &mut f.fs,
        &entry("certificate-proof"),
        input.scope.database(),
        CounterEntropy::new(942_000),
        CounterEntropy::new(943_000),
        &mut TestKeyAdapter,
        limits(),
        |group| {
            replayed.push(group.revision.get());
            Ok(())
        },
    )
    .unwrap();
    f.store = store;
    assert_eq!(replayed, [1, 2, 3]);
    assert_eq!(report.frontier, Some(input.revision));
    assert!(f.store.certificate_anchors.is_empty());
    let roots = f
        .store
        .load_index_root_manifests(&mut f.fs, input.scope, input.index_profile)
        .unwrap();
    let mut cache = PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap();
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &roots[0], 1, b"ka", &mut cache)
            .unwrap()
            .0,
        Some(b"one".to_vec())
    );
    let first = CommitRevision::new(1).unwrap();
    let exact_bytes = 12 * SMALL_ENVELOPE_BYTES; // 3 groups + 3 direct certificates + 6 proof certificates.
    let mut count = 0;
    let work = f
        .store
        .visit_committed_range_report(&mut f.fs, first, input.revision, 3, exact_bytes, |_, _| {
            count += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(work.encoded_bytes, exact_bytes);
    assert!(matches!(
        f.store.visit_committed_range_report(
            &mut f.fs,
            first,
            input.revision,
            3,
            exact_bytes - 1,
            |_, _| Ok(())
        ),
        Err(StorageError::ResourceLimit)
    ));
    let next = f
        .store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"four",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    assert!(f.store.certificate_anchors.is_empty());
    let stage = f
        .store
        .open_index_recovery_stage_with_io(
            &mut f.fs,
            input.scope,
            next.revision,
            next.certificate_digest,
        )
        .unwrap();
    let run = f
        .store
        .stage_index_merge_visit(
            &mut f.fs,
            &stage,
            input.index_profile,
            1,
            Some(&roots[0]),
            merge_limits(),
            [IndexDelta::new(
                b"kc".to_vec(),
                None,
                Some(b"three".to_vec()),
            )],
            &mut |_, _| Ok(()),
        )
        .unwrap()
        .run
        .unwrap();
    input.revision = next.revision;
    input.certificate_digest = next.certificate_digest;
    let staged = f
        .store
        .finish_index_recovery_stage(stage, input, &[run])
        .unwrap();
    assert_eq!(
        f.store
            .index_get(&mut f.fs, staged.read_root(), 1, b"kc", &mut cache)
            .unwrap()
            .0,
        Some(b"three".to_vec())
    );
    let published = f
        .store
        .publish_index_root_recovered_bounded(&mut f.fs, input, &[run], read())
        .unwrap();
    assert_eq!(
        f.store
            .index_get(&mut f.fs, &published, 1, b"kc", &mut cache)
            .unwrap()
            .0,
        Some(b"three".to_vec())
    );
    assert!(f.store.certificate_anchors.is_empty());
}

#[test]
fn disk_certificate_mode_admits_before_callbacks_or_repairs_and_rejects_late_corruption() {
    for (corrupt, admission) in [
        (
            false,
            CertificateAnchorReadLimits::new(2, 2 * SMALL_ENVELOPE_BYTES).unwrap(),
        ),
        (
            false,
            CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES - 1).unwrap(),
        ),
        (true, limits()),
    ] {
        let (mut f, input, _) = rooted();
        if corrupt {
            let offset = 3 * SMALL_ENVELOPE_BYTES;
            let mut bytes = read_bounded(
                &mut f.fs,
                &f.store.certificate_file,
                offset,
                SMALL_ENVELOPE_BYTES,
            )
            .unwrap();
            bytes[100] ^= 1;
            write_all_at(&mut f.fs, &f.store.certificate_file, offset, &bytes).unwrap();
        }
        drop(f.store);
        f.fs.arm(FaultPlan::default()).unwrap();
        let result = JournalStore::open_with_disk_certificate_anchors(
            &mut f.fs,
            &entry("certificate-proof"),
            input.scope.database(),
            CounterEntropy::new(942_000),
            CounterEntropy::new(943_000),
            &mut TestKeyAdapter,
            admission,
            |_| panic!("refused prefix must not produce callbacks"),
        );
        assert!(result.is_err());
        assert_eq!(f.fs.operation_count(Operation::SetLen), 0);
        assert_eq!(f.fs.operation_count(Operation::SyncData), 0);
    }
}

#[test]
fn disk_certificate_checkpoints_reprove_before_streaming_and_remain_optional() {
    let (mut f, input, _) = rooted();
    f.store
        .publish_checkpoint(
            &mut f.fs,
            CheckpointInput {
                scope: input.scope,
                revision: input.revision,
                certificate_digest: input.certificate_digest,
                reducer_profile: input.reducer_profile,
                logical_state_digest: input.logical_state_digest,
                payload: b"optional cache",
            },
        )
        .unwrap();
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"four",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    drop(f.store);
    let (store, _) = JournalStore::open_with_disk_certificate_anchors(
        &mut f.fs,
        &entry("certificate-proof"),
        input.scope.database(),
        CounterEntropy::new(942_000),
        CounterEntropy::new(943_000),
        &mut TestKeyAdapter,
        limits(),
        |_| Ok(()),
    )
    .unwrap();
    f.store = store;
    let collected = f.store.load_checkpoints(&mut f.fs, input.scope);
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].payload(), b"optional cache");
    let candidates = f.store.checkpoint_stream_candidates(&mut f.fs, input.scope);
    assert_eq!(candidates.len(), 1);
    let mut payload = Vec::new();
    f.store
        .stream_checkpoint_candidate(&mut f.fs, candidates[0], &mut |bytes| {
            payload.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
    assert_eq!(payload, b"optional cache");
    assert_eq!(f.store.certificate_anchor_residency(), (false, 0));

    // Refusal to prove an optional cache omits it; an explicit stream returns the refusal
    // before emitting even one byte. Neither path can manufacture an older journal frontier.
    f.store.certificate_read_limits =
        Some(CertificateAnchorReadLimits::new(1, SMALL_ENVELOPE_BYTES).unwrap());
    assert!(f.store.load_checkpoints(&mut f.fs, input.scope).is_empty());
    assert!(
        f.store
            .checkpoint_stream_candidates(&mut f.fs, input.scope)
            .is_empty()
    );
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        f.store
            .stream_checkpoint_candidate(&mut f.fs, candidates[0], &mut |_| {
                panic!("proof admission must precede payload output")
            }),
        Err(StorageError::ResourceLimit)
    ));
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    f.store.certificate_read_limits = Some(limits());

    let offset = 4 * SMALL_ENVELOPE_BYTES;
    let mut encoded = read_bounded(
        &mut f.fs,
        &f.store.certificate_file,
        offset,
        SMALL_ENVELOPE_BYTES,
    )
    .unwrap();
    encoded[100] ^= 1;
    write_all_at(&mut f.fs, &f.store.certificate_file, offset, &encoded).unwrap();
    assert!(
        f.store
            .stream_checkpoint_candidate(&mut f.fs, candidates[0], &mut |_| {
                panic!("late certificate corruption must precede payload output")
            })
            .is_err()
    );
    assert!(f.store.load_checkpoints(&mut f.fs, input.scope).is_empty());
    assert!(
        f.store
            .checkpoint_stream_candidates(&mut f.fs, input.scope)
            .is_empty()
    );
    assert_eq!(f.store.frontier.unwrap().get(), 4);
}
