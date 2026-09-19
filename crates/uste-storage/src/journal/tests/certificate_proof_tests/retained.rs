use super::*;

#[test]
fn retained_range_proofs_preserve_order_budgets_and_live_owner_binding() {
    for disk in [false, true] {
        let mut f = Fixture::new(false);
        if disk {
            f.store.certificate_anchors.clear();
            f.store.certificate_read_limits =
                Some(CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap());
        }
        for first in 1..=3 {
            for last in first..=3 {
                let groups = last - first + 1;
                let proof_reads = if disk {
                    (first..=last).map(|n| 4 - n).sum()
                } else {
                    0
                };
                let bytes = (2 * groups + proof_reads) * SMALL_ENVELOPE_BYTES;
                let mut retained = Vec::new();
                let mut seen = Vec::new();
                let report = f
                    .store
                    .visit_committed_range_with_proofs_report(
                        &mut f.fs,
                        CommitRevision::new(first).unwrap(),
                        CommitRevision::new(last).unwrap(),
                        groups,
                        bytes,
                        |_, group, proof| {
                            assert_eq!(proof.is_some(), disk);
                            if let Some(proof) = proof {
                                assert_eq!(
                                    proof.anchor(),
                                    (group.revision, group.certificate_digest)
                                );
                                assert_eq!(proof.report().certificates, 4 - group.revision.get());
                                assert_eq!(
                                    proof.report().encoded_bytes,
                                    (4 - group.revision.get()) * SMALL_ENVELOPE_BYTES
                                );
                                retained.push(proof.clone());
                            }
                            seen.push(group.revision.get());
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(seen, (first..=last).collect::<Vec<_>>());
                assert_eq!(
                    report,
                    JournalRangeReadReport {
                        groups,
                        encoded_bytes: bytes
                    }
                );
                f.fs.arm(FaultPlan::default()).unwrap();
                let foreign = Fixture::new(false);
                for proof in &retained {
                    f.store.validate_certificate_anchor_proof(proof).unwrap();
                    let scope = NamespaceRef::new(
                        f.store.database,
                        uste_types::NamespaceId::from_bytes([1; 16]),
                    );
                    f.store
                        .open_proven_index_recovery_stage(scope, proof)
                        .unwrap();
                    assert!(
                        foreign
                            .store
                            .open_proven_index_recovery_stage(scope, proof)
                            .is_err()
                    );
                }
                assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
                assert!(matches!(
                    f.store.visit_committed_range_with_proofs_report(
                        &mut f.fs,
                        CommitRevision::new(first).unwrap(),
                        CommitRevision::new(last).unwrap(),
                        groups,
                        bytes - 1,
                        |_, _, _| Ok(())
                    ),
                    Err(StorageError::ResourceLimit)
                ));
            }
        }
        if disk {
            assert!(f.store.certificate_anchors.is_empty());
        }
    }
}
