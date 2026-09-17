use sha2::{Digest, Sha256};
use uste_storage::BlobInventory;
use uste_txn::{ApplyError, CheckpointState, CheckpointStateError, TransactionState};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};

use crate::{
    MAX_SPATIAL_BATCH_RECORDS, MAX_SPATIAL_CATALOG_ENTRIES, ObservationOutcome, SpatialCatalog,
    SpatialError, SpatialRecord, codec::encode_record_ref, decode_record, encode_record,
    model::SpatialRecordRef,
};

const TRANSACTION_MAGIC: &[u8; 8] = b"USTX\0\x01\0\0";
const CHECKPOINT_MAGIC: &[u8; 8] = b"USCP\0\x01\0\0";
const MAX_SPATIAL_CHECKPOINT_BYTES: usize = 256 * 1024 * 1024;
const MAX_SPATIAL_TRANSACTION_BYTES: usize = 16 * 1024 * 1024;
const SPATIAL_REDUCER_PROFILE: [u8; 32] = [
    0x75, 0x73, 0x74, 0x65, 0x2d, 0x73, 0x70, 0x61, 0x74, 0x69, 0x61, 0x6c, 0x2d, 0x72, 0x65, 0x64,
    0x75, 0x63, 0x65, 0x72, 0x2d, 0x76, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpatialTransaction {
    scope: NamespaceRef,
    records: Vec<SpatialRecord>,
}

impl SpatialTransaction {
    pub fn new(scope: NamespaceRef, records: Vec<SpatialRecord>) -> Result<Self, SpatialError> {
        if records.is_empty() || records.len() > MAX_SPATIAL_BATCH_RECORDS {
            return Err(SpatialError::ResourceLimit);
        }
        if records.iter().any(|record| {
            let id = record.id();
            id.database() != scope.database() || id.namespace() != scope.namespace()
        }) {
            return Err(SpatialError::ScopeMismatch);
        }
        Ok(Self { scope, records })
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub fn records(&self) -> &[SpatialRecord] {
        &self.records
    }
}

/// Canonical transaction bytes used by the spatial reducer and journal replay.
pub fn encode_transaction(transaction: &SpatialTransaction) -> Result<Vec<u8>, SpatialError> {
    let mut payloads = Vec::new();
    payloads
        .try_reserve(transaction.records.len())
        .map_err(|_| SpatialError::ResourceLimit)?;
    let mut total = 8_usize + 16 + 16 + 4;
    for record in &transaction.records {
        let encoded = encode_record(record)?;
        total = total
            .checked_add(4)
            .and_then(|value| value.checked_add(encoded.len()))
            .ok_or(SpatialError::ResourceLimit)?;
        if total > MAX_SPATIAL_TRANSACTION_BYTES {
            return Err(SpatialError::ResourceLimit);
        }
        payloads.push(encoded);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(total)
        .map_err(|_| SpatialError::ResourceLimit)?;
    output.extend_from_slice(TRANSACTION_MAGIC);
    output.extend_from_slice(transaction.scope.database().as_bytes());
    output.extend_from_slice(transaction.scope.namespace().as_bytes());
    push_u32(&mut output, transaction.records.len())?;
    for encoded in payloads {
        push_u32(&mut output, encoded.len())?;
        output.extend_from_slice(&encoded);
    }
    Ok(output)
}

pub fn decode_transaction(input: &[u8]) -> Result<SpatialTransaction, SpatialError> {
    if input.len() > MAX_SPATIAL_TRANSACTION_BYTES {
        return Err(SpatialError::ResourceLimit);
    }
    let mut cursor = Cursor::new(input);
    if cursor.take(8)? != TRANSACTION_MAGIC {
        return Err(SpatialError::InvalidEncoding);
    }
    let database = DatabaseId::from_bytes(cursor.array()?);
    let namespace = NamespaceId::from_bytes(cursor.array()?);
    let count = cursor.u32()? as usize;
    if count == 0 || count > MAX_SPATIAL_BATCH_RECORDS || count > cursor.remaining() / 4 {
        return Err(SpatialError::ResourceLimit);
    }
    let mut records = Vec::new();
    records
        .try_reserve_exact(count)
        .map_err(|_| SpatialError::ResourceLimit)?;
    for _ in 0..count {
        let length = cursor.u32()? as usize;
        records.push(decode_record(cursor.take(length)?)?);
    }
    if cursor.remaining() != 0 {
        return Err(SpatialError::InvalidEncoding);
    }
    SpatialTransaction::new(NamespaceRef::new(database, namespace), records)
}

#[derive(Clone, Debug)]
pub struct SpatialSnapshot {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    catalog: SpatialCatalog,
}

impl SpatialSnapshot {
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }
    #[must_use]
    pub const fn revision(&self) -> Option<CommitRevision> {
        self.revision
    }
    #[must_use]
    pub const fn catalog(&self) -> &SpatialCatalog {
        &self.catalog
    }
}

#[derive(Clone, Debug)]
pub struct SpatialState {
    snapshot: SpatialSnapshot,
}

impl SpatialState {
    #[must_use]
    pub fn new(scope: NamespaceRef) -> Self {
        Self {
            snapshot: SpatialSnapshot {
                scope,
                revision: None,
                catalog: SpatialCatalog::new(),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct PreparedSpatial {
    snapshot: SpatialSnapshot,
    result_digest: [u8; 32],
}

impl TransactionState for SpatialState {
    type Prepared = PreparedSpatial;
    type Snapshot = SpatialSnapshot;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if blob_inventory.is_some() {
            return Err(ApplyError::InvalidRequest);
        }
        let transaction = decode_transaction(canonical_request).map_err(map_apply_error)?;
        if transaction.scope != self.snapshot.scope
            || self
                .snapshot
                .revision
                .is_some_and(|previous| revision <= previous)
        {
            return Err(ApplyError::Conflict);
        }
        let mut snapshot = self.snapshot.clone();
        let outcomes = snapshot
            .catalog
            .apply_batch_prepared(revision, transaction.records())
            .map_err(map_apply_error)?;
        snapshot.revision = Some(revision);
        let result_digest = digest_effects(
            &snapshot,
            revision,
            canonical_request,
            transaction.records(),
            &outcomes,
        )
        .map_err(map_apply_error)?;
        Ok(PreparedSpatial {
            snapshot,
            result_digest,
        })
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.result_digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        self.snapshot = prepared.snapshot;
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.snapshot.clone()
    }
}

impl CheckpointState for SpatialState {
    const REDUCER_PROFILE: [u8; 32] = SPATIAL_REDUCER_PROFILE;

    fn checkpoint_scope(snapshot: &Self::Snapshot) -> NamespaceRef {
        snapshot.scope
    }

    fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
        snapshot.revision
    }

    fn logical_state_digest(snapshot: &Self::Snapshot) -> Result<[u8; 32], CheckpointStateError> {
        digest_snapshot(snapshot, b"USTE-SPATIAL-LOGICAL-STATE-V1\0").map_err(map_checkpoint_error)
    }

    fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
        encode_checkpoint(snapshot)
    }

    fn decode_checkpoint(
        scope: NamespaceRef,
        revision: CommitRevision,
        encoded: &[u8],
    ) -> Result<Self, CheckpointStateError> {
        decode_checkpoint(scope, revision, encoded)
    }

    fn current_checkpoint_scope(&self) -> NamespaceRef {
        self.snapshot.scope
    }

    fn current_checkpoint_revision(&self) -> Option<CommitRevision> {
        self.snapshot.revision
    }

    fn current_logical_state_digest(&self) -> Result<[u8; 32], CheckpointStateError> {
        digest_snapshot(&self.snapshot, b"USTE-SPATIAL-LOGICAL-STATE-V1\0")
            .map_err(map_checkpoint_error)
    }

    fn encode_current_checkpoint(&self) -> Result<Vec<u8>, CheckpointStateError> {
        encode_checkpoint(&self.snapshot)
    }
}

fn encode_checkpoint(snapshot: &SpatialSnapshot) -> Result<Vec<u8>, CheckpointStateError> {
    let revision = snapshot.revision.ok_or(CheckpointStateError::Invalid)?;
    let entry_count = snapshot.catalog.entry_count();
    if entry_count == 0 || entry_count > MAX_SPATIAL_CATALOG_ENTRIES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(8 + 16 + 16 + 8 + 8)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    output.extend_from_slice(CHECKPOINT_MAGIC);
    output.extend_from_slice(snapshot.scope.database().as_bytes());
    output.extend_from_slice(snapshot.scope.namespace().as_bytes());
    output.extend_from_slice(&revision.get().to_be_bytes());
    output.extend_from_slice(&(entry_count as u64).to_be_bytes());
    snapshot
        .catalog
        .visit_records(|recorded_revision, record| {
            let encoded = encode_record_ref(record).map_err(map_checkpoint_error)?;
            let additional = 8_usize
                .checked_add(4)
                .and_then(|value| value.checked_add(encoded.len()))
                .ok_or(CheckpointStateError::ResourceLimit)?;
            let next_length = output
                .len()
                .checked_add(additional)
                .ok_or(CheckpointStateError::ResourceLimit)?;
            if next_length > MAX_SPATIAL_CHECKPOINT_BYTES {
                return Err(CheckpointStateError::ResourceLimit);
            }
            output
                .try_reserve(additional)
                .map_err(|_| CheckpointStateError::ResourceLimit)?;
            output.extend_from_slice(&recorded_revision.get().to_be_bytes());
            push_u32(&mut output, encoded.len()).map_err(map_checkpoint_error)?;
            output.extend_from_slice(&encoded);
            Ok::<(), CheckpointStateError>(())
        })?;
    Ok(output)
}

fn decode_checkpoint(
    scope: NamespaceRef,
    revision: CommitRevision,
    encoded: &[u8],
) -> Result<SpatialState, CheckpointStateError> {
    if encoded.len() > MAX_SPATIAL_CHECKPOINT_BYTES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let mut cursor = Cursor::new(encoded);
    if cursor.take(8).map_err(map_checkpoint_error)? != CHECKPOINT_MAGIC {
        return Err(CheckpointStateError::UnsupportedProfile);
    }
    let database = DatabaseId::from_bytes(cursor.array().map_err(map_checkpoint_error)?);
    let namespace = NamespaceId::from_bytes(cursor.array().map_err(map_checkpoint_error)?);
    if NamespaceRef::new(database, namespace) != scope
        || cursor.u64().map_err(map_checkpoint_error)? != revision.get()
    {
        return Err(CheckpointStateError::Invalid);
    }
    let count = cursor.u64().map_err(map_checkpoint_error)?;
    let count = usize::try_from(count).map_err(|_| CheckpointStateError::ResourceLimit)?;
    if count == 0 || count > MAX_SPATIAL_CATALOG_ENTRIES || count > cursor.remaining() / 12 {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| CheckpointStateError::ResourceLimit)?;
    let mut previous_key = None;
    for _ in 0..count {
        let recorded = CommitRevision::new(cursor.u64().map_err(map_checkpoint_error)?)
            .map_err(|_| CheckpointStateError::Invalid)?;
        if recorded > revision {
            return Err(CheckpointStateError::Invalid);
        }
        let length = cursor.u32().map_err(map_checkpoint_error)? as usize;
        let record = decode_record(cursor.take(length).map_err(map_checkpoint_error)?)
            .map_err(map_checkpoint_error)?;
        if record.id().database() != scope.database()
            || record.id().namespace() != scope.namespace()
        {
            return Err(CheckpointStateError::Invalid);
        }
        let key = SpatialRecordRef::from(&record).order_key();
        if previous_key.is_some_and(|previous| previous >= key) {
            return Err(CheckpointStateError::Invalid);
        }
        previous_key = Some(key);
        entries.push((recorded, record));
    }
    if cursor.remaining() != 0 {
        return Err(CheckpointStateError::Invalid);
    }
    entries.sort_unstable_by_key(|(recorded, record)| {
        (recorded.get(), SpatialRecordRef::from(record).order_key())
    });
    let mut catalog = SpatialCatalog::new();
    let mut entries = entries.into_iter().peekable();
    while let Some((recorded, first)) = entries.next() {
        let mut records = Vec::new();
        records
            .try_reserve(1)
            .map_err(|_| CheckpointStateError::ResourceLimit)?;
        records.push(first);
        while entries
            .peek()
            .is_some_and(|(next_revision, _)| *next_revision == recorded)
        {
            if records.len() == MAX_SPATIAL_BATCH_RECORDS {
                return Err(CheckpointStateError::ResourceLimit);
            }
            records
                .try_reserve(1)
                .map_err(|_| CheckpointStateError::ResourceLimit)?;
            let (_, record) = entries.next().ok_or(CheckpointStateError::Invalid)?;
            records.push(record);
        }
        catalog
            .apply_batch_prepared(recorded, &records)
            .map_err(map_checkpoint_error)?;
    }
    if catalog.entry_count() != count {
        return Err(CheckpointStateError::Invalid);
    }
    Ok(SpatialState {
        snapshot: SpatialSnapshot {
            scope,
            revision: Some(revision),
            catalog,
        },
    })
}

fn digest_snapshot(snapshot: &SpatialSnapshot, domain: &[u8]) -> Result<[u8; 32], SpatialError> {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(snapshot.scope.database().as_bytes());
    digest.update(snapshot.scope.namespace().as_bytes());
    digest.update(
        snapshot
            .revision
            .map_or(0, CommitRevision::get)
            .to_be_bytes(),
    );
    digest.update(
        u64::try_from(snapshot.catalog.entry_count())
            .map_err(|_| SpatialError::ResourceLimit)?
            .to_be_bytes(),
    );
    snapshot
        .catalog
        .visit_records(|recorded_revision, record| {
            let encoded = encode_record_ref(record)?;
            digest.update(recorded_revision.get().to_be_bytes());
            digest.update(
                u64::try_from(encoded.len())
                    .map_err(|_| SpatialError::ResourceLimit)?
                    .to_be_bytes(),
            );
            digest.update(encoded);
            Ok::<(), SpatialError>(())
        })?;
    Ok(digest.finalize().into())
}

fn digest_effects(
    snapshot: &SpatialSnapshot,
    revision: CommitRevision,
    canonical_request: &[u8],
    records: &[SpatialRecord],
    outcomes: &[ObservationOutcome],
) -> Result<[u8; 32], SpatialError> {
    let mut digest = Sha256::new();
    digest.update(b"USTE-SPATIAL-RESULT-V1\0");
    digest.update(revision.get().to_be_bytes());
    digest.update(
        u64::try_from(canonical_request.len())
            .map_err(|_| SpatialError::ResourceLimit)?
            .to_be_bytes(),
    );
    digest.update(canonical_request);
    digest.update(
        u64::try_from(records.len())
            .map_err(|_| SpatialError::ResourceLimit)?
            .to_be_bytes(),
    );
    let mut outcomes = outcomes.iter();
    for requested in records {
        let (recorded_revision, stored) = snapshot
            .catalog
            .stored_effect(requested)
            .ok_or(SpatialError::InvalidEncoding)?;
        let kind = match stored {
            SpatialRecordRef::World(_) => 0,
            SpatialRecordRef::Frame(_) => 1,
            SpatialRecordRef::Geometry(_) => 2,
            SpatialRecordRef::Observation(_) => 3,
        };
        digest.update([kind]);
        digest.update(recorded_revision.get().to_be_bytes());
        if matches!(stored, SpatialRecordRef::Observation(_)) {
            match outcomes.next().ok_or(SpatialError::InvalidEncoding)? {
                ObservationOutcome::Inserted { id } => {
                    digest.update([0]);
                    digest_record_ref(&mut digest, *id);
                }
                ObservationOutcome::Duplicate { existing } => {
                    digest.update([1]);
                    digest_record_ref(&mut digest, *existing);
                }
            }
        }
        let encoded = encode_record_ref(stored)?;
        digest.update(
            u64::try_from(encoded.len())
                .map_err(|_| SpatialError::ResourceLimit)?
                .to_be_bytes(),
        );
        digest.update(encoded);
    }
    if outcomes.next().is_some() {
        return Err(SpatialError::InvalidEncoding);
    }
    Ok(digest.finalize().into())
}

fn digest_record_ref(digest: &mut Sha256, reference: uste_types::RecordRef) {
    digest.update(reference.database().as_bytes());
    digest.update(reference.namespace().as_bytes());
    digest.update(reference.record().as_bytes());
}

fn map_apply_error(error: SpatialError) -> ApplyError {
    match error {
        SpatialError::ResourceLimit => ApplyError::ResourceLimit,
        SpatialError::VersionConflict
        | SpatialError::DuplicateRecord
        | SpatialError::SourceEventConflict => ApplyError::Conflict,
        _ => ApplyError::InvalidRequest,
    }
}

fn map_checkpoint_error(error: SpatialError) -> CheckpointStateError {
    match error {
        SpatialError::ResourceLimit => CheckpointStateError::ResourceLimit,
        SpatialError::UnsupportedProfile => CheckpointStateError::UnsupportedProfile,
        _ => CheckpointStateError::Invalid,
    }
}

fn push_u32(output: &mut Vec<u8>, value: usize) -> Result<(), SpatialError> {
    output.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| SpatialError::ResourceLimit)?
            .to_be_bytes(),
    );
    Ok(())
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.offset)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SpatialError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(SpatialError::ResourceLimit)?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(SpatialError::InvalidEncoding)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], SpatialError> {
        self.take(N)?
            .try_into()
            .map_err(|_| SpatialError::InvalidEncoding)
    }

    fn u32(&mut self) -> Result<u32, SpatialError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, SpatialError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
}
