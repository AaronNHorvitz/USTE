use super::*;

struct Fixture {
    fs: FaultFileSystem<MemoryFileSystem>,
    store: FaultStore,
    base: RecoveredIndexRoot,
    inputs: [IndexRootInput; 3],
}

impl Fixture {
    fn new() -> Self {
        let database = DatabaseId::from_bytes([0x91; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0x92; 16]));
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut fs,
            options(database, "index-stage"),
            create_vault(database, 920_000),
            CounterEntropy::new(921_000),
        )
        .unwrap();
        let first = store
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: b"synthetic staged base",
                    logical_event_digest: [1; 32],
                },
            )
            .unwrap();
        let input = IndexRootInput {
            scope,
            revision: first.revision,
            certificate_digest: first.certificate_digest,
            reducer_profile: [2; 32],
            logical_state_digest: [3; 32],
            index_profile: [4; 32],
        };
        let run = store
            .publish_index_run(
                &mut fs,
                scope,
                first.revision,
                input.index_profile,
                1,
                [IndexEntry {
                    key: b"k".to_vec(),
                    value: b"one".to_vec(),
                }],
            )
            .unwrap();
        let base = store
            .publish_index_root_recovered(&mut fs, input, &[run])
            .unwrap();
        let mut inputs = [input; 3];
        for (index, target) in inputs.iter_mut().enumerate().skip(1) {
            let committed = store
                .append_group(
                    &mut fs,
                    CommitInput {
                        encoded_group: b"synthetic staged suffix",
                        logical_event_digest: [index as u8; 32],
                    },
                )
                .unwrap();
            target.revision = committed.revision;
            target.certificate_digest = committed.certificate_digest;
            target.logical_state_digest = [index as u8 + 3; 32];
        }
        Self {
            fs,
            store,
            base,
            inputs,
        }
    }

    fn stage(&self, index: usize) -> IndexRecoveryStage {
        let input = self.inputs[index];
        self.store
            .open_index_recovery_stage(input.scope, input.revision, input.certificate_digest)
            .unwrap()
    }

    fn roots(&mut self) -> Vec<RecoveredIndexRoot> {
        self.store
            .load_index_roots(
                &mut self.fs,
                self.inputs[0].scope,
                self.inputs[0].index_profile,
            )
            .unwrap()
    }
}

fn merge(
    store: &mut FaultStore,
    fs: &mut FaultFileSystem<MemoryFileSystem>,
    stage: &IndexRecoveryStage,
    base: &RecoveredIndexRoot,
    before: &[u8],
    after: &[u8],
) -> Result<MergedIndexRun, StorageError> {
    store.stage_index_merge_visit(
        fs,
        stage,
        [4; 32],
        1,
        Some(base),
        IndexRunMergeLimits::new(
            IndexRunReadLimits::new(2, 4, 1024).unwrap(),
            4,
            1024,
            4,
            1024,
        )
        .unwrap(),
        [IndexDelta::new(
            b"k".to_vec(),
            Some(before.to_vec()),
            Some(after.to_vec()),
        )],
        &mut |key, value| {
            assert_eq!(key, b"k");
            assert_eq!(value, after);
            Ok(())
        },
    )
}

#[test]
fn historical_stages_remain_invisible_until_current_frontier_publication() {
    let mut f = Fixture::new();
    let stage = f.stage(1);
    let run = merge(&mut f.store, &mut f.fs, &stage, &f.base, b"one", b"two")
        .unwrap()
        .run
        .unwrap();
    let second = f
        .store
        .finish_index_recovery_stage(stage, f.inputs[1], &[run])
        .unwrap();
    assert_eq!(second.read_root().generation(), 0);
    assert_eq!(f.roots(), vec![f.base.clone()]);
    assert_eq!(
        f.store
            .publish_index_root_recovered(&mut f.fs, f.inputs[1], &[run]),
        Err(StorageError::InvalidState)
    );
    let mut cache = PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap();
    assert_eq!(
        f.store
            .index_get(&mut f.fs, second.read_root(), 1, b"k", &mut cache)
            .unwrap()
            .0
            .unwrap(),
        b"two"
    );
    let stage = f.stage(2);
    let run = merge(
        &mut f.store,
        &mut f.fs,
        &stage,
        second.read_root(),
        b"two",
        b"three",
    )
    .unwrap()
    .run
    .unwrap();
    let third = f
        .store
        .finish_index_recovery_stage(stage, f.inputs[2], &[run])
        .unwrap();
    assert_eq!(f.roots(), vec![f.base.clone()]);
    let published = f
        .store
        .publish_index_root_recovered(&mut f.fs, f.inputs[2], &[run])
        .unwrap();
    assert_ne!(published.generation(), 0);
    assert_eq!(f.roots()[0], published);
    assert_eq!(
        f.store
            .index_get(&mut f.fs, third.read_root(), 1, b"k", &mut cache)
            .unwrap()
            .0
            .unwrap(),
        b"three"
    );
    let database = f.inputs[0].scope.database();
    drop(f.store);
    f.fs.restart().unwrap();
    let store = reopen_fault_store(&mut f.fs, database, "index-stage", 922_000);
    assert_eq!(
        store
            .load_index_roots(&mut f.fs, f.inputs[0].scope, [4; 32])
            .unwrap()[0],
        published
    );
}

#[test]
fn stage_anchor_binding_and_forward_only_base_precede_io() {
    let mut f = Fixture::new();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        f.store
            .open_index_recovery_stage(f.inputs[1].scope, f.inputs[1].revision, [0; 32])
            .is_err()
    );
    let stage = f.stage(0);
    assert!(merge(&mut f.store, &mut f.fs, &stage, &f.base, b"one", b"two").is_err());
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
    let stage = f.stage(1);
    assert!(
        f.store
            .finish_index_recovery_stage(
                stage,
                f.inputs[1],
                &f.base.runs().copied().collect::<Vec<_>>()
            )
            .is_err()
    );
    let stale = f.stage(1);
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"later frontier",
                logical_event_digest: [9; 32],
            },
        )
        .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(merge(&mut f.store, &mut f.fs, &stale, &f.base, b"one", b"two").is_err());
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn every_staged_merge_io_failure_or_crash_preserves_the_published_base() {
    let mut baseline = Fixture::new();
    let stage = baseline.stage(1);
    baseline.fs.arm(FaultPlan::default()).unwrap();
    merge(
        &mut baseline.store,
        &mut baseline.fs,
        &stage,
        &baseline.base,
        b"one",
        b"two",
    )
    .unwrap();
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        for occurrence in 1..=baseline.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new();
                let stage = f.stage(1);
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    merge(&mut f.store, &mut f.fs, &stage, &f.base, b"one", b"two").is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(f.fs.pending_faults(), 0);
                let database = f.inputs[0].scope.database();
                drop(f.store);
                f.fs.restart().unwrap();
                let store = reopen_fault_store(&mut f.fs, database, "index-stage", 923_000);
                assert_eq!(store.frontier(), Some(f.inputs[2].revision));
                assert_eq!(
                    store
                        .load_index_roots(&mut f.fs, f.inputs[0].scope, [4; 32])
                        .unwrap(),
                    vec![f.base]
                );
            }
        }
    }
}

#[test]
fn staged_root_binding_corruption_and_visitor_failure_never_publish() {
    let mut f = Fixture::new();
    let before =
        f.fs.inner()
            .test_child_names(&f.store.database_directory)
            .unwrap();
    let stage = f.stage(1);
    let run = merge(&mut f.store, &mut f.fs, &stage, &f.base, b"one", b"two")
        .unwrap()
        .run
        .unwrap();
    for case in 0..4 {
        let mut input = f.inputs[1];
        match case {
            0 => {
                input.scope = NamespaceRef::new(
                    input.scope.database(),
                    uste_types::NamespaceId::from_bytes([0xff; 16]),
                )
            }
            1 => input.revision = f.inputs[2].revision,
            2 => input.certificate_digest[0] ^= 1,
            _ => input.index_profile[0] ^= 1,
        }
        assert!(
            f.store
                .finish_index_recovery_stage(f.stage(1), input, &[run])
                .is_err()
        );
    }
    let staged = f
        .store
        .finish_index_recovery_stage(stage, f.inputs[1], &[run])
        .unwrap();
    let run_name =
        f.fs.inner()
            .test_child_names(&f.store.database_directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("i-") && !before.contains(name))
            .unwrap();
    let file =
        f.fs.open_existing(&f.store.database_directory, &run_name)
            .unwrap();
    let mut byte = [0];
    assert_eq!(f.fs.read_at(&file, 100, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(f.fs.write_at(&file, 100, &byte).unwrap(), 1);
    let stage = f.stage(2);
    assert!(
        merge(
            &mut f.store,
            &mut f.fs,
            &stage,
            staged.read_root(),
            b"two",
            b"three"
        )
        .is_err()
    );
    assert_eq!(f.roots(), vec![f.base.clone()]);
    byte[0] ^= 1;
    assert_eq!(f.fs.write_at(&file, 100, &byte).unwrap(), 1);
    let result = f.store.stage_index_merge_visit(
        &mut f.fs,
        &stage,
        [4; 32],
        1,
        Some(staged.read_root()),
        IndexRunMergeLimits::new(
            IndexRunReadLimits::new(2, 4, 1024).unwrap(),
            4,
            1024,
            4,
            1024,
        )
        .unwrap(),
        [IndexDelta::new(
            b"k".to_vec(),
            Some(b"two".to_vec()),
            Some(b"three".to_vec()),
        )],
        &mut |_, _| Err(StorageError::ResourceLimit),
    );
    assert_eq!(result, Err(StorageError::ResourceLimit));
    assert_eq!(f.roots(), vec![f.base.clone()]);
    f.store.poisoned = true;
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        merge(
            &mut f.store,
            &mut f.fs,
            &stage,
            staged.read_root(),
            b"two",
            b"three"
        )
        .is_err()
    );
    assert!(
        f.store
            .open_index_recovery_stage(
                f.inputs[1].scope,
                f.inputs[1].revision,
                f.inputs[1].certificate_digest
            )
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
}
