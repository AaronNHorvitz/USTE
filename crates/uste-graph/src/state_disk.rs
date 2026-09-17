//! Certificate-anchored complete graph-state derived cache.
//!
//! This profile remains optional and read-only. The authenticated journal is the only commit
//! authority, and this module does not construct reducer state during recovery.

use sha2::{Digest, Sha256};
use uste_crypto::EntropySource;
use uste_storage::{
    DurableIndexRoot, IndexEntry, IndexRootInput, IndexRunDescriptor, IndexScrubReport,
    OwnershipFileSystem, PageCache, RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, StorageError},
};
use uste_txn::{CheckpointState, CommitCoordinator, TransactionError};
use uste_types::{CommitRevision, RecordRef};

use crate::codec::encode_result_policy;
use crate::{
    GraphCodecError, GraphDiskError, GraphSnapshot, GraphState, Record, encode_stored_record,
    state::ReverseReference,
};

pub const GRAPH_STATE_PROFILE_V1: [u8; 32] = [
    0x31, 0x3f, 0x9c, 0x9b, 0xe8, 0xa4, 0x20, 0x6d, 0xb1, 0x11, 0x8a, 0xf6, 0x2c, 0xe2, 0x08, 0x61,
    0x78, 0x44, 0x4c, 0x03, 0xdc, 0x22, 0xa0, 0xab, 0xba, 0xc7, 0xfb, 0x87, 0x14, 0xc6, 0x6d, 0x52,
];

const FAMILY_METADATA: u8 = 1;
const FAMILY_CURRENT_RECORD: u8 = 2;
const FAMILY_RECORD_HISTORY: u8 = 3;
const FAMILY_OUTGOING: u8 = 4;
const FAMILY_INCOMING: u8 = 5;
const FAMILY_PROVENANCE: u8 = 6;
const FAMILY_REVERSE: u8 = 7;
const FAMILY_POLICY: u8 = 8;
const FAMILY_COUNT: u8 = 8;

pub struct DerivedGraphStateRoot {
    root: RecoveredIndexRoot,
}

impl core::fmt::Debug for DerivedGraphStateRoot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("DerivedGraphStateRoot")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl DerivedGraphStateRoot {
    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }
}

pub fn publish_graph_state_root<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    snapshot: &GraphSnapshot,
) -> Result<DurableIndexRoot, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_snapshot_is_current(coordinator, snapshot)?;
    let revision = snapshot
        .revision()
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    let (anchor_revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    if anchor_revision != revision {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let logical_state_digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let expected = expected_runs(snapshot, revision)?;
    let mut runs = Vec::with_capacity(expected.len());
    for expected_run in &expected {
        let run = coordinator.publish_index_run_fallible(
            filesystem,
            revision,
            GRAPH_STATE_PROFILE_V1,
            expected_run.family,
            family_entries(snapshot, revision, expected_run.family)?,
        )?;
        if !expected_run.matches(&run) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        runs.push(run);
    }
    Ok(coordinator.publish_index_root(
        filesystem,
        IndexRootInput {
            scope: snapshot.scope(),
            revision,
            certificate_digest,
            reducer_profile: GraphState::REDUCER_PROFILE,
            logical_state_digest,
            index_profile: GRAPH_STATE_PROFILE_V1,
        },
        &runs,
    )?)
}

pub fn load_graph_state_roots<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    snapshot: &GraphSnapshot,
) -> Result<Vec<DerivedGraphStateRoot>, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_snapshot_is_current(coordinator, snapshot)?;
    let revision = snapshot
        .revision()
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    let digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let (anchor_revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    if anchor_revision != revision {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let expected = expected_runs(snapshot, revision)?;
    let mut cache = PageCache::default();
    let mut admitted = Vec::new();
    for root in coordinator.load_index_roots(filesystem, GRAPH_STATE_PROFILE_V1)? {
        if root.revision() == revision
            && root.certificate_digest() == &certificate_digest
            && root.reducer_profile() == &GraphState::REDUCER_PROFILE
            && root.logical_state_digest() == &digest
            && runs_match(root.runs(), &expected)
        {
            match coordinator.scrub_index_root(filesystem, &root, &mut cache) {
                Ok(_) => admitted.push(DerivedGraphStateRoot { root }),
                Err(TransactionError::Storage(
                    StorageError::IntegrityFailure | StorageError::UnsupportedProfile,
                )) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(admitted)
}

pub fn scrub_graph_state_root<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &DerivedGraphStateRoot,
    cache: &mut PageCache,
) -> Result<IndexScrubReport, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, root)?;
    Ok(coordinator.scrub_index_root(filesystem, &root.root, cache)?)
}

fn validate_current_root<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    root: &DerivedGraphStateRoot,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let Some((revision, certificate_digest)) = coordinator.checkpoint_anchor()? else {
        return Err(GraphDiskError::RootStateMismatch);
    };
    if root.root.revision() != revision
        || root.root.certificate_digest() != &certificate_digest
        || root.root.reducer_profile() != &GraphState::REDUCER_PROFILE
        || root.root.index_profile() != &GRAPH_STATE_PROFILE_V1
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    Ok(())
}

fn validate_snapshot_is_current<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    supplied: &GraphSnapshot,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let current = coordinator
        .reducer_state_for_checkpoint()?
        .current_snapshot();
    if supplied.scope() != current.scope()
        || supplied.revision() != current.revision()
        || GraphState::logical_state_digest(supplied)
            .map_err(|_| GraphDiskError::RootStateMismatch)?
            != GraphState::logical_state_digest(current)
                .map_err(|_| GraphDiskError::RootStateMismatch)?
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ExpectedRun {
    family: u8,
    entry_count: u64,
    logical_digest: [u8; 32],
}

impl ExpectedRun {
    fn matches(self, actual: &IndexRunDescriptor) -> bool {
        actual.family() == self.family
            && actual.entry_count() == self.entry_count
            && actual.logical_digest() == &self.logical_digest
    }
}

fn expected_runs(
    snapshot: &GraphSnapshot,
    revision: CommitRevision,
) -> Result<Vec<ExpectedRun>, GraphDiskError> {
    let mut expected = Vec::with_capacity(usize::from(FAMILY_COUNT));
    for family in 1..=FAMILY_COUNT {
        let mut builder = ExpectedRunBuilder::new(snapshot, revision, family);
        for entry in family_entries(snapshot, revision, family)? {
            let entry = entry.map_err(GraphDiskError::Storage)?;
            builder.add(&entry.key, &entry.value)?;
        }
        let run = builder.finish();
        if run.entry_count != 0 {
            expected.push(run);
        }
    }
    Ok(expected)
}

fn runs_match<'a>(
    actual: impl ExactSizeIterator<Item = &'a IndexRunDescriptor>,
    expected: &[ExpectedRun],
) -> bool {
    actual.len() == expected.len()
        && actual
            .zip(expected)
            .all(|(actual, expected)| expected.matches(actual))
}

type EntryIterator<'a> = Box<dyn Iterator<Item = Result<IndexEntry, StorageError>> + 'a>;

fn family_entries<'a>(
    snapshot: &'a GraphSnapshot,
    revision: CommitRevision,
    family: u8,
) -> Result<EntryIterator<'a>, GraphDiskError> {
    let records: &'a std::collections::BTreeMap<RecordRef, Record> = &snapshot.records;
    let entries: EntryIterator<'a> = match family {
        FAMILY_METADATA => Box::new(core::iter::once(Ok(IndexEntry {
            key: b"graph-state-v1".to_vec(),
            value: metadata_value(snapshot, revision)?,
        }))),
        FAMILY_CURRENT_RECORD => Box::new(snapshot.records.values().map(|record| {
            encode_stored_record(record)
                .map(|value| IndexEntry {
                    key: record.id().record().as_bytes().to_vec(),
                    value,
                })
                .map_err(codec_storage_error)
        })),
        FAMILY_RECORD_HISTORY => Box::new(snapshot.history.iter().flat_map(|(id, versions)| {
            versions.iter().map(move |record| {
                encode_stored_record(record)
                    .map(|value| IndexEntry {
                        key: history_key(*id, record.modified_revision()),
                        value,
                    })
                    .map_err(codec_storage_error)
            })
        })),
        FAMILY_OUTGOING => Box::new(snapshot.outgoing.iter().flat_map(
            move |(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(records, *entity, *relationship)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            },
        )),
        FAMILY_INCOMING => Box::new(snapshot.incoming.iter().flat_map(
            move |(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(records, *entity, *relationship)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            },
        )),
        FAMILY_PROVENANCE => Box::new(snapshot.provenance.iter().flat_map(|(evidence, claims)| {
            claims.iter().map(move |claim| {
                Ok(IndexEntry {
                    key: pair_key(*evidence, *claim),
                    value: Vec::new(),
                })
            })
        })),
        FAMILY_REVERSE => Box::new(snapshot.reverse.iter().flat_map(|(target, owners)| {
            owners.iter().map(move |(owner, reference)| {
                Ok(IndexEntry {
                    key: pair_key(*target, *owner),
                    value: reverse_value(*reference),
                })
            })
        })),
        FAMILY_POLICY => {
            let current = snapshot.policy.iter().map(|policy| {
                encode_result_policy(Some(policy))
                    .map(|value| IndexEntry {
                        key: vec![0],
                        value,
                    })
                    .map_err(codec_storage_error)
            });
            let history = snapshot.policy_history.iter().map(|(revision, policy)| {
                encode_result_policy(Some(policy))
                    .map(|value| {
                        let mut key = Vec::with_capacity(9);
                        key.push(1);
                        key.extend_from_slice(&revision.get().to_be_bytes());
                        IndexEntry { key, value }
                    })
                    .map_err(codec_storage_error)
            });
            Box::new(current.chain(history))
        }
        _ => return Err(GraphDiskError::IndexCorrupt),
    };
    Ok(entries)
}

fn metadata_value(
    snapshot: &GraphSnapshot,
    revision: CommitRevision,
) -> Result<Vec<u8>, GraphDiskError> {
    let counts = [
        count(snapshot.records.len())?,
        total(snapshot.history.values().map(Vec::len))?,
        total(snapshot.outgoing.values().map(|values| values.len()))?,
        total(snapshot.incoming.values().map(|values| values.len()))?,
        total(snapshot.provenance.values().map(|values| values.len()))?,
        total(snapshot.reverse.values().map(|values| values.len()))?,
        count(snapshot.policy_history.len())?,
        u64::from(snapshot.policy.is_some()),
    ];
    let mut value = Vec::with_capacity(80);
    value.extend_from_slice(b"UGSM");
    value.extend_from_slice(&[1, 0, 0, 0]);
    value.extend_from_slice(&revision.get().to_be_bytes());
    for count in counts {
        value.extend_from_slice(&count.to_be_bytes());
    }
    debug_assert_eq!(value.len(), 80);
    Ok(value)
}

fn count(value: usize) -> Result<u64, GraphDiskError> {
    u64::try_from(value).map_err(|_| GraphDiskError::IndexCorrupt)
}

fn total(mut values: impl Iterator<Item = usize>) -> Result<u64, GraphDiskError> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(count(value)?)
            .ok_or(GraphDiskError::IndexCorrupt)
    })
}

fn history_key(id: RecordRef, revision: CommitRevision) -> Vec<u8> {
    let mut key = Vec::with_capacity(24);
    key.extend_from_slice(id.record().as_bytes());
    key.extend_from_slice(&revision.get().to_be_bytes());
    key
}

fn pair_key(left: RecordRef, right: RecordRef) -> Vec<u8> {
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(left.record().as_bytes());
    key.extend_from_slice(right.record().as_bytes());
    key
}

fn reverse_value(reference: ReverseReference) -> Vec<u8> {
    let mut value = Vec::with_capacity(24);
    value.push(reference.owner_kind);
    value.push(reference.owner_state);
    value.extend_from_slice(&reference.roles.to_be_bytes());
    value.extend_from_slice(&reference.owner_version.get().to_be_bytes());
    value.extend_from_slice(&reference.owner_revision.get().to_be_bytes());
    value.extend_from_slice(&[0; 4]);
    value
}

fn relationship_neighbor(
    records: &std::collections::BTreeMap<RecordRef, Record>,
    entity: RecordRef,
    relationship: RecordRef,
) -> Result<RecordRef, StorageError> {
    let Some(Record::Relationship(record)) = records.get(&relationship) else {
        return Err(StorageError::IntegrityFailure);
    };
    if record.from == entity {
        Ok(record.to)
    } else if record.to == entity {
        Ok(record.from)
    } else {
        Err(StorageError::IntegrityFailure)
    }
}

struct ExpectedRunBuilder {
    family: u8,
    entry_count: u64,
    digest: Sha256,
}

impl ExpectedRunBuilder {
    fn new(snapshot: &GraphSnapshot, revision: CommitRevision, family: u8) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(snapshot.scope().namespace().as_bytes());
        digest.update(revision.get().to_be_bytes());
        digest.update(GRAPH_STATE_PROFILE_V1);
        digest.update([family]);
        Self {
            family,
            entry_count: 0,
            digest,
        }
    }

    fn add(&mut self, key: &[u8], value: &[u8]) -> Result<(), GraphDiskError> {
        self.entry_count = self
            .entry_count
            .checked_add(1)
            .ok_or(GraphDiskError::IndexCorrupt)?;
        self.digest.update(
            u32::try_from(key.len())
                .map_err(|_| GraphDiskError::IndexCorrupt)?
                .to_be_bytes(),
        );
        self.digest.update(
            u64::try_from(value.len())
                .map_err(|_| GraphDiskError::IndexCorrupt)?
                .to_be_bytes(),
        );
        self.digest.update(key);
        self.digest.update(value);
        Ok(())
    }

    fn finish(self) -> ExpectedRun {
        ExpectedRun {
            family: self.family,
            entry_count: self.entry_count,
            logical_digest: self.digest.finalize().into(),
        }
    }
}

fn codec_storage_error(error: GraphCodecError) -> StorageError {
    if matches!(error, GraphCodecError::ResourceLimit) {
        StorageError::ResourceLimit
    } else {
        StorageError::IntegrityFailure
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssertionAction, DurablePolicyMutation, Expected, GraphTransaction, NewEntity, NewEvidence,
        NewRecord, NewRelationship, Operation, ValidTime,
    };
    use uste_policy::{NamespacePolicy, PolicyVersion, QuotaLimits};
    use uste_txn::TransactionState;
    use uste_types::{BoundedString, DatabaseId, NamespaceId, NamespaceRef, RecordId, Value};

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([0xa1; 16]),
            NamespaceId::from_bytes([0xa2; 16]),
        )
    }

    fn record(value: u8) -> RecordRef {
        RecordRef::new(
            scope().database(),
            scope().namespace(),
            RecordId::from_bytes([value; 16]),
        )
    }

    fn text(value: &str) -> BoundedString {
        BoundedString::new(value.to_owned()).unwrap()
    }

    fn canonical_fixture() -> (GraphSnapshot, [RecordRef; 4]) {
        let ids = [record(1), record(2), record(3), record(4)];
        let mut state = GraphState::new(scope());
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Entity(NewEntity {
                                id: ids[0],
                                entity_type: text("left"),
                                schema_version: 1,
                                properties: Value::Null,
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Entity(NewEntity {
                                id: ids[1],
                                entity_type: text("right"),
                                schema_version: 1,
                                properties: Value::Null,
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Evidence(NewEvidence {
                                id: ids[2],
                                digest: [0x33; 32],
                                locator: text("fixture://state-root"),
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Relationship(NewRelationship {
                                id: ids[3],
                                from: ids[0],
                                to: ids[1],
                                relationship_type: text("edge"),
                                properties: Value::Null,
                                evidence: vec![ids[2]],
                                valid_time: ValidTime::Unknown,
                            }),
                        },
                    ],
                ),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![Operation::ActOnRelationship {
                        target: ids[3],
                        expected: Expected::Version(crate::RecordVersion::FIRST),
                        action: AssertionAction::Accept,
                        correction: None,
                        correction_expected: None,
                    }],
                ),
                CommitRevision::new(2).unwrap(),
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        (state.snapshot(), ids)
    }

    fn entries(snapshot: &GraphSnapshot, family: u8) -> Vec<IndexEntry> {
        family_entries(snapshot, CommitRevision::new(2).unwrap(), family)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn canonical_fixture_pins_every_nonempty_family_and_run_digest() {
        let (snapshot, [left, right, evidence, relationship]) = canonical_fixture();
        let revision = CommitRevision::new(2).unwrap();

        let metadata = entries(&snapshot, FAMILY_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].key, b"graph-state-v1");
        assert_eq!(
            &metadata[0].value[..16],
            [b"UGSM\x01\0\0\0".as_slice(), &2_u64.to_be_bytes()].concat()
        );
        assert_eq!(
            &metadata[0].value[16..],
            [4_u64, 5, 1, 1, 1, 3, 0, 0]
                .into_iter()
                .flat_map(u64::to_be_bytes)
                .collect::<Vec<_>>()
        );

        let current = entries(&snapshot, FAMILY_CURRENT_RECORD);
        assert_eq!(
            current
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            [&left, &right, &evidence, &relationship].map(|id| id.record().as_bytes().to_vec())
        );
        for entry in &current {
            let id = RecordRef::new(
                scope().database(),
                scope().namespace(),
                RecordId::from_bytes(entry.key.clone().try_into().unwrap()),
            );
            assert_eq!(
                entry.value,
                encode_stored_record(snapshot.record(id).unwrap()).unwrap()
            );
        }

        let history = entries(&snapshot, FAMILY_RECORD_HISTORY);
        assert_eq!(history.len(), 5);
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            vec![
                history_key(left, CommitRevision::FIRST),
                history_key(right, CommitRevision::FIRST),
                history_key(evidence, CommitRevision::FIRST),
                history_key(relationship, CommitRevision::FIRST),
                history_key(relationship, revision),
            ]
        );

        assert_eq!(
            entries(&snapshot, FAMILY_OUTGOING),
            vec![IndexEntry {
                key: pair_key(left, relationship),
                value: right.record().as_bytes().to_vec(),
            }]
        );
        assert_eq!(
            entries(&snapshot, FAMILY_INCOMING),
            vec![IndexEntry {
                key: pair_key(right, relationship),
                value: left.record().as_bytes().to_vec(),
            }]
        );
        assert_eq!(
            entries(&snapshot, FAMILY_PROVENANCE),
            vec![IndexEntry {
                key: pair_key(evidence, relationship),
                value: Vec::new(),
            }]
        );
        let reverse = entries(&snapshot, FAMILY_REVERSE);
        assert_eq!(
            reverse
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            vec![
                pair_key(left, relationship),
                pair_key(right, relationship),
                pair_key(evidence, relationship),
            ]
        );
        assert_eq!(
            reverse
                .iter()
                .map(|entry| entry.value.clone())
                .collect::<Vec<_>>(),
            [0x0008_u16, 0x0010, 0x0040].map(|roles| {
                reverse_value(ReverseReference {
                    owner_kind: 3,
                    owner_state: 2,
                    roles,
                    owner_version: crate::RecordVersion::new(2).unwrap(),
                    owner_revision: revision,
                })
            })
        );
        assert!(entries(&snapshot, FAMILY_POLICY).is_empty());

        let runs = expected_runs(&snapshot, revision).unwrap();
        assert_eq!(
            runs.iter()
                .map(|run| (run.family, run.entry_count))
                .collect::<Vec<_>>(),
            vec![(1, 1), (2, 4), (3, 5), (4, 1), (5, 1), (6, 1), (7, 3)]
        );
        assert_eq!(
            runs.iter()
                .map(|run| hex(&run.logical_digest))
                .collect::<Vec<_>>(),
            vec![
                "4dbe1e3810ea2d731493955c02f8da4ffc9bf85e7daa28b9b7f21118169788d9",
                "69cd84ba1c43b708efd74276a0ff5b0352273c9c927519860146e49b59e9308d",
                "f6818475c142205696213b8c18f1a48d9688eee627e13baea86938be73b08a35",
                "64d853f62bbcdad7f8dcc69690aeb7ec4d7c3144f1b399ea438290b0ff61f06b",
                "7855bec47713289802d8eb2d8df9be03bce4e36a1a9cf8cc0778130801a94332",
                "d7057241aaa6cfb400566303603cae6fa553053fad96ab516ced8c93637ac8ff",
                "27273b10d66f68ad64d24bf5baa321a444c76dca20c14ae07d6b6e62eac85d59",
            ]
        );
    }

    #[test]
    fn policy_only_state_has_canonical_metadata_and_policy_family() {
        let mut state = GraphState::new(scope());
        let policy = NamespacePolicy::new(
            scope(),
            PolicyVersion::new(1).unwrap(),
            QuotaLimits::new(1024, 2048, 4096, 2, 1024).unwrap(),
        );
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::with_policy_mutation(
                    scope(),
                    Vec::new(),
                    DurablePolicyMutation::Install { policy },
                ),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        let snapshot = state.snapshot();

        let runs = expected_runs(&snapshot, CommitRevision::FIRST).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].family, FAMILY_METADATA);
        assert_eq!(runs[0].entry_count, 1);
        assert_eq!(runs[1].family, FAMILY_POLICY);
        assert_eq!(runs[1].entry_count, 2);

        let metadata = family_entries(&snapshot, CommitRevision::FIRST, FAMILY_METADATA)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(metadata.key, b"graph-state-v1");
        assert_eq!(metadata.value.len(), 80);
        assert_eq!(&metadata.value[..8], b"UGSM\x01\0\0\0");
        assert_eq!(&metadata.value[8..16], &1_u64.to_be_bytes());
        assert_eq!(&metadata.value[64..72], &1_u64.to_be_bytes());
        assert_eq!(&metadata.value[72..80], &1_u64.to_be_bytes());

        let policy_entries: Vec<_> =
            family_entries(&snapshot, CommitRevision::FIRST, FAMILY_POLICY)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
        assert_eq!(policy_entries[0].key, vec![0]);
        assert_eq!(
            policy_entries[1].key,
            [vec![1], 1_u64.to_be_bytes().to_vec()].concat()
        );
        assert_eq!(policy_entries[0].value, policy_entries[1].value);
    }

    #[test]
    fn reverse_descriptor_has_fixed_versioned_field_layout() {
        let encoded = reverse_value(ReverseReference {
            owner_kind: 3,
            owner_state: 6,
            roles: 0x00a5,
            owner_version: crate::RecordVersion::new(9).unwrap(),
            owner_revision: CommitRevision::new(11).unwrap(),
        });
        assert_eq!(encoded.len(), 24);
        assert_eq!(&encoded[..4], &[3, 6, 0, 0xa5]);
        assert_eq!(&encoded[4..12], &9_u64.to_be_bytes());
        assert_eq!(&encoded[12..20], &11_u64.to_be_bytes());
        assert_eq!(&encoded[20..], &[0; 4]);
    }
}
