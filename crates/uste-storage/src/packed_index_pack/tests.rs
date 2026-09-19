use super::*;
use crate::{
    AdapterErrorKind,
    fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation},
    memory::MemoryFileSystem,
};
use uste_crypto::{
    CryptoError, EntropyFailure, KeyAdapter, KeyEpoch, SecretKeyMaterial, WriterIncarnationId,
};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};

struct Entropy(u64);
impl EntropySource for Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        for (i, chunk) in out.chunks_mut(8).enumerate() {
            chunk.copy_from_slice(&(self.0 + i as u64).to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
struct Adapter;
impl KeyAdapter for Adapter {
    type Envelope = [u8; 32];
    fn wrap(
        &mut self,
        _: DatabaseId,
        key: &SecretKeyMaterial,
        _: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(*key.expose_to_adapter())
    }
    fn unwrap(
        &mut self,
        _: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*envelope))
    }
}
fn context() -> PackedPageContext {
    PackedPageContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([3; 16]),
        creation_revision: CommitRevision::FIRST,
        profile: [4; 32],
        family: 5,
        object: [0; 16],
        page: 0,
    }
}
fn vault() -> KeyVault<[u8; 32], Entropy> {
    KeyVault::create(context().scope.database(), &mut Adapter, Entropy(100)).unwrap()
}
fn limits() -> PackWriteLimits {
    PackWriteLimits {
        maximum_pages: 3,
        maximum_records: 256,
        maximum_payload_bytes: 3 * MAX_RECORD_PAYLOAD as u64,
    }
}
fn write_pack<F: FileSystem>(
    fs: &mut F,
    vault: &mut KeyVault<[u8; 32], Entropy>,
    seed: u64,
) -> Result<(ImmutablePack, Vec<PackedRecordAddress>), StorageError> {
    let mut writer =
        ImmutablePackWriter::create(fs, &fs.root(), context(), limits(), &mut Entropy(seed))?;
    let mut addresses = Vec::new();
    for payload in [
        vec![11; MAX_RECORD_PAYLOAD],
        vec![22; MAX_RECORD_PAYLOAD],
        vec![33],
    ] {
        addresses.push(writer.append(fs, vault, PackedRecordKind::ValueChunk, &payload)?);
    }
    Ok((writer.finish(fs, vault)?, addresses))
}
fn read<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    pack: ImmutablePack,
    address: PackedRecordAddress,
) -> Result<(PackedPage, PackReadReport), StorageError> {
    read_record_page(
        fs,
        &fs.root(),
        vault,
        pack,
        address,
        ENCODED_PAGE_BYTES as u64,
    )
}

#[test]
fn immutable_pack_bounded_multi_page_records_survive_model_restart() {
    let mut fs = MemoryFileSystem::default();
    let mut vault = vault();
    let root = fs.root();
    let mut writer =
        ImmutablePackWriter::create(&mut fs, &root, context(), limits(), &mut Entropy(200))
            .unwrap();
    let mut addresses = Vec::new();
    for i in 0..130 {
        addresses.push(
            writer
                .append(
                    &mut fs,
                    &mut vault,
                    PackedRecordKind::TreeNode,
                    &[i as u8; 100],
                )
                .unwrap(),
        );
    }
    addresses.push(
        writer
            .append(
                &mut fs,
                &mut vault,
                PackedRecordKind::ValueChunk,
                &vec![231; MAX_RECORD_PAYLOAD],
            )
            .unwrap(),
    );
    let pack = writer.finish(&mut fs, &mut vault).unwrap();
    assert_eq!(
        (pack.pages(), pack.records(), pack.payload_bytes()),
        (3, 131, 13_000 + MAX_RECORD_PAYLOAD as u64)
    );
    fs.restart().unwrap();
    for (i, address) in addresses.into_iter().enumerate() {
        let expected_page = if i < 128 {
            0
        } else if i < 130 {
            1
        } else {
            2
        };
        assert_eq!(address.context().page, expected_page);
        let (page, report) = read(&mut fs, &vault, pack, address).unwrap();
        assert_eq!(
            report,
            PackReadReport {
                pages: 1,
                encoded_bytes: ENCODED_PAGE_BYTES as u64
            }
        );
        let expected = if i == 130 {
            vec![231; MAX_RECORD_PAYLOAD]
        } else {
            vec![i as u8; 100]
        };
        assert_eq!(page.record(address.slot()).unwrap().payload, expected);
    }
    // A fresh descriptor never grants access to a different object, even under equal profiles.
    let (other, other_addresses) = write_pack(&mut fs, &mut vault, 300).unwrap();
    assert!(read(&mut fs, &vault, pack, other_addresses[0]).is_err());
    assert_eq!(other.pages(), 3);
}

#[test]
fn immutable_pack_limits_precede_io_and_append_refusal_is_sticky() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut vault = vault();
    let root = fs.root();
    for limits in [
        PackWriteLimits {
            maximum_pages: 0,
            ..limits()
        },
        PackWriteLimits {
            maximum_pages: MAX_PAGES + 1,
            ..limits()
        },
        PackWriteLimits {
            maximum_records: 0,
            ..limits()
        },
        PackWriteLimits {
            maximum_payload_bytes: 0,
            ..limits()
        },
    ] {
        assert!(
            ImmutablePackWriter::create(&mut fs, &root, context(), limits, &mut Entropy(1))
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
    for cap in [
        PackWriteLimits {
            maximum_pages: 1,
            ..limits()
        },
        PackWriteLimits {
            maximum_records: 1,
            ..limits()
        },
        PackWriteLimits {
            maximum_payload_bytes: MAX_RECORD_PAYLOAD as u64,
            ..limits()
        },
    ] {
        let root = fs.root();
        let seed = 1000 + fs.operation_count(Operation::CreateNew);
        let mut writer =
            ImmutablePackWriter::create(&mut fs, &root, context(), cap, &mut Entropy(seed))
                .unwrap();
        writer
            .append(
                &mut fs,
                &mut vault,
                PackedRecordKind::ValueChunk,
                &vec![1; MAX_RECORD_PAYLOAD],
            )
            .unwrap();
        assert_eq!(
            writer
                .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, b"x")
                .err(),
            Some(StorageError::ResourceLimit)
        );
        assert_eq!(
            writer
                .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, b"x")
                .err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(
            writer.finish(&mut fs, &mut vault).err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        assert_eq!(fs.operation_count(Operation::SyncAll), 0);
    }
    let (pack, addresses) = write_pack(&mut fs, &mut vault, 2000).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let root = fs.root();
    assert_eq!(
        read_record_page(
            &mut fs,
            &root,
            &vault,
            pack,
            addresses[0],
            ENCODED_PAGE_BYTES as u64 - 1
        )
        .err(),
        Some(StorageError::ResourceLimit)
    );
    for address in [
        PackedRecordAddress {
            slot: MAX_SLOTS,
            ..addresses[0]
        },
        PackedRecordAddress {
            context: PackedPageContext {
                family: 6,
                ..addresses[0].context
            },
            ..addresses[0]
        },
        PackedRecordAddress {
            context: PackedPageContext {
                page: pack.pages,
                ..addresses[0].context
            },
            ..addresses[0]
        },
    ] {
        assert_eq!(
            read(&mut fs, &vault, pack, address).err(),
            Some(StorageError::InvalidState)
        );
    }
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
    assert!(read(&mut fs, &vault, pack, addresses[0]).is_ok());
}

#[test]
fn immutable_pack_every_write_fault_preserves_old_durable_pack_without_success_descriptor() {
    let mut old_fs = MemoryFileSystem::default();
    let mut vault = vault();
    let (old, old_addresses) = write_pack(&mut old_fs, &mut vault, 1000).unwrap();
    let mut observed = FaultFileSystem::new(old_fs.clone(), FaultPlan::default());
    write_pack(&mut observed, &mut vault, 2000).unwrap();
    let mut cases = 0;
    for operation in [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        for occurrence in 1..=observed.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut fs = FaultFileSystem::new(old_fs.clone(), plan);
                let result = write_pack(&mut fs, &mut vault, 2000);
                assert!(result.is_err(), "{operation:?} {occurrence} {action:?}");
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                let (page, _) = read(&mut fs, &vault, old, old_addresses[0]).unwrap();
                assert_eq!(
                    page.record(0).unwrap().payload,
                    vec![11; MAX_RECORD_PAYLOAD]
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 21);
}

#[test]
fn immutable_pack_every_read_fault_refuses_then_recovers_and_corruption_never_returns_records() {
    let mut base = MemoryFileSystem::default();
    let mut vault = vault();
    let (pack, addresses) = write_pack(&mut base, &mut vault, 1000).unwrap();
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let plan = FaultPlan::new([FaultPoint {
                operation,
                occurrence: 1,
                action,
            }])
            .unwrap();
            let mut fs = FaultFileSystem::new(base.clone(), plan);
            assert!(read(&mut fs, &vault, pack, addresses[1]).is_err());
            assert_eq!(fs.pending_faults(), 0);
            fs.restart().unwrap();
            assert_eq!(
                read(&mut fs, &vault, pack, addresses[1])
                    .unwrap()
                    .0
                    .record(0)
                    .unwrap()
                    .payload,
                vec![22; MAX_RECORD_PAYLOAD]
            );
        }
    }
    for variant in 0..4 {
        let mut fs = base.clone();
        let file = fs
            .open_existing(&fs.root(), &pack_name(pack.context.object).unwrap())
            .unwrap();
        match variant {
            0 => fs
                .set_len(&file, pack.pages * ENCODED_PAGE_BYTES as u64 - 1)
                .unwrap(),
            1 => fs
                .set_len(&file, pack.pages * ENCODED_PAGE_BYTES as u64 + 1)
                .unwrap(),
            2 => {
                let mut byte = [0];
                let offset = ENCODED_PAGE_BYTES as u64 + 100;
                read_exact_at(&mut fs, &file, offset, &mut byte).unwrap();
                byte[0] ^= 1;
                write_all_at(&mut fs, &file, offset, &byte).unwrap();
            }
            _ => {
                let mut page = vec![0; ENCODED_PAGE_BYTES];
                read_exact_at(&mut fs, &file, 0, &mut page).unwrap();
                write_all_at(&mut fs, &file, ENCODED_PAGE_BYTES as u64, &page).unwrap();
            }
        }
        fs.sync_all(&file).unwrap();
        fs.restart().unwrap();
        assert!(read(&mut fs, &vault, pack, addresses[1]).is_err());
    }
}

#[test]
fn immutable_pack_short_io_retries_and_zero_progress_or_overreport_refuses() {
    for maximum in [1, 7, 4096, ENCODED_PAGE_BYTES - 1] {
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action: FaultAction::ShortWrite { maximum },
        }])
        .unwrap();
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut vault = vault();
        let (pack, addresses) = write_pack(&mut fs, &mut vault, 1000).unwrap();
        assert_eq!(fs.operation_count(Operation::WriteAt), 4);
        fs.restart().unwrap();
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence: 1,
                action: FaultAction::ShortRead { maximum },
            }])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            read(&mut fs, &vault, pack, addresses[0])
                .unwrap()
                .0
                .record(0)
                .unwrap()
                .payload,
            vec![11; MAX_RECORD_PAYLOAD]
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 2);
    }
    for operation in [Operation::WriteAt, Operation::ReadAt] {
        for action in [FaultAction::ZeroProgress, FaultAction::OverReport] {
            let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
            let mut vault = vault();
            let (pack, addresses) = write_pack(&mut fs, &mut vault, 1000).unwrap();
            fs.arm(
                FaultPlan::new([FaultPoint {
                    operation,
                    occurrence: 1,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            if operation == Operation::WriteAt {
                assert!(write_pack(&mut fs, &mut vault, 2000).is_err());
            } else {
                assert!(read(&mut fs, &vault, pack, addresses[0]).is_err());
            }
            assert_eq!(fs.pending_faults(), 0);
        }
    }
}

#[test]
fn immutable_pack_identity_entropy_and_collision_fail_without_overwriting() {
    struct BadEntropy(bool);
    impl EntropySource for BadEntropy {
        fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
            if self.0 {
                Err(EntropyFailure)
            } else {
                out.fill(0);
                Ok(())
            }
        }
    }
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let root = fs.root();
    for bad in [false, true] {
        assert!(
            ImmutablePackWriter::create(&mut fs, &root, context(), limits(), &mut BadEntropy(bad))
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
    let mut vault = vault();
    let (pack, addresses) = write_pack(&mut fs, &mut vault, 1000).unwrap();
    assert!(write_pack(&mut fs, &mut vault, 1000).is_err());
    fs.restart().unwrap();
    assert_eq!(
        read(&mut fs, &vault, pack, addresses[2])
            .unwrap()
            .0
            .record(0)
            .unwrap()
            .payload,
        &[33]
    );
}

#[test]
fn immutable_pack_failed_rollover_never_resumes_or_finishes_a_prefix() {
    for action in [
        FaultAction::Error(AdapterErrorKind::Io),
        FaultAction::CrashBefore,
        FaultAction::CrashAfter,
        FaultAction::ZeroProgress,
        FaultAction::OverReport,
    ] {
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action,
        }])
        .unwrap();
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut vault = vault();
        let root = fs.root();
        let mut writer =
            ImmutablePackWriter::create(&mut fs, &root, context(), limits(), &mut Entropy(1000))
                .unwrap();
        writer
            .append(
                &mut fs,
                &mut vault,
                PackedRecordKind::ValueChunk,
                &vec![1; MAX_RECORD_PAYLOAD],
            )
            .unwrap();
        assert!(
            writer
                .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, b"next")
                .is_err()
        );
        assert_eq!(fs.pending_faults(), 0);
        assert_eq!(
            writer
                .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, b"retry")
                .err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(
            writer.finish(&mut fs, &mut vault).err(),
            Some(StorageError::NeedsRecovery)
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 1);
        assert_eq!(fs.operation_count(Operation::SyncAll), 0);
        fs.restart().unwrap();
    }
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut vault = vault();
    let root = fs.root();
    let mut writer =
        ImmutablePackWriter::create(&mut fs, &root, context(), limits(), &mut Entropy(1000))
            .unwrap();
    writer
        .append(
            &mut fs,
            &mut vault,
            PackedRecordKind::ValueChunk,
            &vec![1; MAX_RECORD_PAYLOAD],
        )
        .unwrap();
    vault.lock();
    assert_eq!(
        writer
            .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, b"next")
            .err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    assert_eq!(
        writer.finish(&mut fs, &mut vault).err(),
        Some(StorageError::NeedsRecovery)
    );
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn immutable_pack_linked_geometry_does_not_relax_exact_descriptor_reads() {
    let mut base = MemoryFileSystem::default();
    let mut vault = vault();
    let (pack, addresses) = write_pack(&mut base, &mut vault, 1000).unwrap();
    for pages in [0, 1, 2, 3, 4] {
        let mut fs = base.clone();
        let directory = fs.root();
        let file = fs
            .open_existing(&directory, &pack_name(pack.context.object).unwrap())
            .unwrap();
        fs.set_len(&file, pages * ENCODED_PAGE_BYTES as u64)
            .unwrap();
        for address in &addresses {
            let linked = read_linked_record_page(
                &mut fs,
                &directory,
                &vault,
                address.context,
                address.slot,
                ENCODED_PAGE_BYTES as u64,
            );
            assert_eq!(linked.is_ok(), address.context.page < pages);
            assert_eq!(read(&mut fs, &vault, pack, *address).is_ok(), pages == 3);
        }
        fs.set_len(&file, pages * ENCODED_PAGE_BYTES as u64 + 1)
            .unwrap();
        assert!(
            read_linked_record_page(
                &mut fs,
                &directory,
                &vault,
                addresses[0].context,
                0,
                ENCODED_PAGE_BYTES as u64
            )
            .is_err()
        );
    }
    let mut fs = FaultFileSystem::new(base, FaultPlan::default());
    let root = fs.root();
    assert_eq!(
        read_linked_record_page(
            &mut fs,
            &root,
            &vault,
            addresses[0].context,
            0,
            ENCODED_PAGE_BYTES as u64 - 1
        )
        .err(),
        Some(StorageError::ResourceLimit)
    );
    assert!(
        read_linked_record_page(
            &mut fs,
            &root,
            &vault,
            addresses[0].context,
            MAX_SLOTS,
            ENCODED_PAGE_BYTES as u64
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
}
