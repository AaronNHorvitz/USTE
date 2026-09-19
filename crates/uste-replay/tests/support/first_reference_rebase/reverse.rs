use super::*;

fn repeated_suffix(
    disk_certificates: bool,
) -> (Fs, EntryName, FaultDiskCounter, CounterState, BlobInventory) {
    let (mut fs, name, disk, base_state, inventory) = fixture(true);
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen_certificate_mode(&mut fs, &name, base_state.clone(), disk_certificates);
    disk.commit(
        &mut fs,
        TransactionRequest {
            blob_inventory: Some(&inventory),
            ..request(3, &3_u64.to_be_bytes())
        },
        &mut TestClock(20),
        &NeverCancel,
        IndexGetLimits::new(16, 136).unwrap(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(disk.overlay_counts(), (2, 1));
    (fs, name, disk, base_state, inventory)
}

fn exact_suffix(disk_certificates: bool) -> CoordinatorFirstReferenceLimits {
    CoordinatorFirstReferenceLimits {
        maximum_owners: 1,
        maximum_groups: 2,
        // Two single-envelope groups and certificates, plus one terminal proof in disk mode.
        maximum_encoded_bytes: (4 + u64::from(disk_certificates)) * 4161,
    }
}

fn assert_first_owners(fs: &mut Fs, disk: &FaultDiskCounter, inventory: &BlobInventory) {
    for reference in inventory.references() {
        let expected = if reference.byte_len() == b"first owner remains principal one".len() as u64
        {
            1
        } else {
            2
        };
        assert_eq!(
            disk.committed_blob_owner(
                fs,
                *reference,
                IndexGetLimits::new(16, 136).unwrap(),
                &mut PageCache::new(64 * 1024).unwrap(),
            )
            .unwrap(),
            Some(PrincipalDigest::from_bytes([expected; 32]))
        );
    }
}

#[test]
fn reverse_first_reference_suffix_has_exact_linear_budget_and_preserves_earliest_principal() {
    for disk_certificates in [false, true] {
        let (mut fs, name, mut disk, _, inventory) = repeated_suffix(disk_certificates);
        let exact = exact_suffix(disk_certificates);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            disk.rebase_metadata_with_first_references(
                &mut fs,
                metadata_rebase_limits(),
                CoordinatorFirstReferenceLimits {
                    maximum_encoded_bytes: exact.maximum_encoded_bytes - 1,
                    ..exact
                },
            )
            .is_err()
        );
        assert_eq!(disk.overlay_counts(), (2, 1));
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), exact)
            .unwrap();
        assert_eq!(disk.overlay_counts(), (0, 0));
        assert_first_owners(&mut fs, &disk, &inventory);
        let state = disk.state().unwrap().clone();
        assert_eq!(state.revision.unwrap().get(), 3);
        drop(disk);
        fs.restart().unwrap();
        // Independent cold admission checks each declared first revision against the journal.
        let disk = reopen_certificate_mode(&mut fs, &name, state, disk_certificates);
        assert_eq!(disk.overlay_counts(), (0, 0));
        assert_first_owners(&mut fs, &disk, &inventory);
    }
}

#[test]
fn reverse_first_reference_suffix_selected_read_faults_preserve_exact_restart() {
    let (mut fs, _, mut sample, _, _) = repeated_suffix(true);
    fs.arm(FaultPlan::default()).unwrap();
    sample
        .rebase_metadata_with_first_references(
            &mut fs,
            metadata_rebase_limits(),
            exact_suffix(true),
        )
        .unwrap();
    let counts = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ]
    .map(|operation| (operation, fs.operation_count(operation)));
    drop(sample);
    let mut attempts = 0;
    let mut failures = 0;
    for (operation, count) in counts {
        assert!(count > 0);
        for occurrence in std::collections::BTreeSet::from([1, count.div_ceil(2), count]) {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, mut disk, base_state, inventory) = repeated_suffix(true);
                let current_state = disk.state().unwrap().clone();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = disk.rebase_metadata_with_first_references(
                    &mut fs,
                    metadata_rebase_limits(),
                    exact_suffix(true),
                );
                assert_eq!(fs.pending_faults(), 0);
                // Optional cache-discovery failures may fall back; success must still be exact.
                assert_eq!(
                    disk.overlay_counts(),
                    if result.is_ok() { (0, 0) } else { (2, 1) }
                );
                failures += usize::from(result.is_err());
                let state = if result.is_ok() {
                    current_state
                } else {
                    base_state
                };
                drop(disk);
                fs.restart().unwrap();
                let mut disk = reopen_certificate_mode(&mut fs, &name, state, true);
                disk.rebase_metadata_with_first_references(
                    &mut fs,
                    metadata_rebase_limits(),
                    exact_suffix(true),
                )
                .unwrap();
                assert_eq!(disk.state().unwrap().revision.unwrap().get(), 3);
                assert_eq!(disk.overlay_counts(), (0, 0));
                assert_first_owners(&mut fs, &disk, &inventory);
                attempts += 1;
            }
        }
    }
    eprintln!("reverse first-reference reads: {attempts} attempts, {failures} failures");
    assert_eq!(attempts, 27);
    assert!(failures > 0);
}
