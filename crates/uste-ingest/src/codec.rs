use uste_storage::{BlobId, BlobReference};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, SourceEventId,
    SourceEventRef,
};

use crate::{
    EngineTransaction, ImportAction, ImportBatch, ImportBatchId, ImportCheckpoint, ImportCursor,
    ImportError, ImportJobStatus, ImportStart, MAX_ROWS_PER_BATCH, MappedRowReceipt,
    MappingBinding, MappingProfile, SourceBinding,
};

const REQUEST_MAGIC: &[u8; 8] = b"UIRQ\0\x01\0\0";
pub const MAX_IMPORT_REQUEST_BYTES: usize = 16 * 1024 * 1024;

pub fn encode_transaction(transaction: &EngineTransaction) -> Result<Vec<u8>, ImportError> {
    let mut output = Vec::new();
    reserve(&mut output, 48)?;
    output.extend_from_slice(REQUEST_MAGIC);
    encode_scope(&mut output, transaction.scope());
    match transaction {
        EngineTransaction::Graph { graph_request, .. } => {
            output.extend_from_slice(&[0; 8]);
            encode_frame(&mut output, graph_request)?;
        }
        EngineTransaction::Import { action, .. } => {
            output.push(1);
            output.extend_from_slice(&[0; 7]);
            match &**action {
                ImportAction::StartJob(start) => {
                    output.extend_from_slice(&[0; 8]);
                    encode_record_ref(&mut output, start.job());
                    encode_source(&mut output, start.source());
                    encode_mapping(&mut output, start.mapping());
                    encode_frame(&mut output, start.graph_request())?;
                }
                ImportAction::ApplyBatch(batch) => {
                    output.push(1);
                    output.extend_from_slice(&[0; 7]);
                    encode_batch(&mut output, batch)?;
                }
            }
        }
    }
    if output.len() > MAX_IMPORT_REQUEST_BYTES {
        return Err(ImportError::ResourceLimit);
    }
    Ok(output)
}

pub fn decode_transaction(input: &[u8]) -> Result<EngineTransaction, ImportError> {
    if input.len() > MAX_IMPORT_REQUEST_BYTES {
        return Err(ImportError::ResourceLimit);
    }
    let mut cursor = Cursor::new(input);
    if cursor.take(8)? != REQUEST_MAGIC {
        return Err(ImportError::UnsupportedProfile);
    }
    let scope = decode_scope(&mut cursor)?;
    let kind = cursor.u8()?;
    cursor.zeros(7)?;
    let transaction = match kind {
        0 => EngineTransaction::Graph {
            scope,
            graph_request: owned_frame(&mut cursor)?,
        },
        1 => match cursor.u8()? {
            0 => {
                cursor.zeros(7)?;
                EngineTransaction::Import {
                    scope,
                    action: Box::new(ImportAction::StartJob(Box::new(ImportStart::new(
                        decode_record_ref(&mut cursor)?,
                        decode_source(&mut cursor)?,
                        decode_mapping(&mut cursor)?,
                        owned_frame(&mut cursor)?,
                    )?))),
                }
            }
            1 => {
                cursor.zeros(7)?;
                EngineTransaction::Import {
                    scope,
                    action: Box::new(ImportAction::ApplyBatch(Box::new(decode_batch(
                        &mut cursor,
                    )?))),
                }
            }
            _ => return Err(ImportError::UnsupportedProfile),
        },
        _ => return Err(ImportError::UnsupportedProfile),
    };
    if cursor.remaining() != 0 || encode_transaction(&transaction)? != input {
        return Err(ImportError::InvalidEncoding);
    }
    Ok(transaction)
}

fn encode_batch(output: &mut Vec<u8>, batch: &ImportBatch) -> Result<(), ImportError> {
    encode_record_ref(output, batch.id().job());
    output.extend_from_slice(&batch.id().sequence().to_be_bytes());
    encode_checkpoint(output, batch.expected());
    output.extend_from_slice(&batch.start().next_row().to_be_bytes());
    let row_count = u32::try_from(batch.rows().len()).map_err(|_| ImportError::ResourceLimit)?;
    output.extend_from_slice(&row_count.to_be_bytes());
    output.extend_from_slice(&[0; 4]);
    reserve(
        output,
        batch
            .rows()
            .len()
            .checked_mul(80)
            .ok_or(ImportError::ResourceLimit)?,
    )?;
    for row in batch.rows() {
        encode_source_event(output, row.source_event());
        output.extend_from_slice(&row.declared_payload_digest());
    }
    encode_frame(output, batch.graph_request())?;
    match batch.spatial_request() {
        None => output.extend_from_slice(&[0; 8]),
        Some(bytes) => {
            output.push(1);
            output.extend_from_slice(&[0; 7]);
            encode_frame(output, bytes)?;
        }
    }
    output.push(u8::from(batch.final_batch()));
    output.extend_from_slice(&[0; 7]);
    Ok(())
}

fn decode_batch(cursor: &mut Cursor<'_>) -> Result<ImportBatch, ImportError> {
    let id = ImportBatchId::new(decode_record_ref(cursor)?, cursor.u64()?)?;
    let expected = decode_checkpoint(cursor)?;
    let start = ImportCursor::new(cursor.u64()?)?;
    let count = cursor.u32()? as usize;
    cursor.zeros(4)?;
    if count == 0 || count > MAX_ROWS_PER_BATCH || count > cursor.remaining() / 80 {
        return Err(ImportError::ResourceLimit);
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|_| ImportError::ResourceLimit)?;
    for _ in 0..count {
        rows.push(MappedRowReceipt::new(
            decode_source_event(cursor)?,
            cursor.array()?,
        ));
    }
    let graph_request = owned_frame(cursor)?;
    let spatial_request = match cursor.u8()? {
        0 => {
            cursor.zeros(7)?;
            None
        }
        1 => {
            cursor.zeros(7)?;
            Some(owned_frame(cursor)?)
        }
        _ => return Err(ImportError::InvalidEncoding),
    };
    let final_batch = match cursor.u8()? {
        0 => false,
        1 => true,
        _ => return Err(ImportError::InvalidEncoding),
    };
    cursor.zeros(7)?;
    ImportBatch::new(
        id,
        expected,
        start,
        rows,
        graph_request,
        spatial_request,
        final_batch,
    )
}

pub(crate) fn encode_checkpoint(output: &mut Vec<u8>, value: &ImportCheckpoint) {
    encode_record_ref(output, value.job());
    output.extend_from_slice(&value.version().to_be_bytes());
    encode_source(output, value.source());
    encode_mapping(output, value.mapping());
    output.extend_from_slice(&value.next_batch().to_be_bytes());
    output.extend_from_slice(&value.cursor().next_row().to_be_bytes());
    output.extend_from_slice(&value.accepted_rows().to_be_bytes());
    output.extend_from_slice(&value.first_revision().get().to_be_bytes());
    output.extend_from_slice(&value.last_revision().get().to_be_bytes());
    output.push(match value.status() {
        ImportJobStatus::Open => 0,
        ImportJobStatus::Completed => 1,
    });
    output.extend_from_slice(&[0; 7]);
    output.extend_from_slice(&value.chain_digest());
}

pub(crate) fn decode_checkpoint(cursor: &mut Cursor<'_>) -> Result<ImportCheckpoint, ImportError> {
    let job = decode_record_ref(cursor)?;
    let version = cursor.u64()?;
    let source = decode_source(cursor)?;
    let mapping = decode_mapping(cursor)?;
    let next_batch = cursor.u64()?;
    let position = ImportCursor::new(cursor.u64()?)?;
    let accepted_rows = cursor.u64()?;
    let first_revision = revision(cursor.u64()?)?;
    let last_revision = revision(cursor.u64()?)?;
    let status = match cursor.u8()? {
        0 => ImportJobStatus::Open,
        1 => ImportJobStatus::Completed,
        _ => return Err(ImportError::InvalidCheckpoint),
    };
    cursor.zeros(7)?;
    let chain_digest = cursor.array()?;
    ImportCheckpoint::from_parts(
        job,
        version,
        source,
        mapping,
        next_batch,
        position,
        accepted_rows,
        first_revision,
        last_revision,
        status,
        chain_digest,
    )
}

pub(crate) fn encode_source(output: &mut Vec<u8>, value: &SourceBinding) {
    encode_record_ref(output, value.evidence());
    output.extend_from_slice(&value.version().to_be_bytes());
    encode_blob(output, value.blob());
}

pub(crate) fn decode_source(cursor: &mut Cursor<'_>) -> Result<SourceBinding, ImportError> {
    SourceBinding::new(
        decode_record_ref(cursor)?,
        cursor.u64()?,
        decode_blob(cursor)?,
    )
}

pub(crate) fn encode_mapping(output: &mut Vec<u8>, value: &MappingBinding) {
    encode_record_ref(output, value.evidence());
    output.extend_from_slice(&value.version().to_be_bytes());
    encode_blob(output, value.blob());
    output.push(match value.profile() {
        MappingProfile::TypedRecordsV1 => 0,
    });
    output.extend_from_slice(&[0; 7]);
}

pub(crate) fn decode_mapping(cursor: &mut Cursor<'_>) -> Result<MappingBinding, ImportError> {
    let evidence = decode_record_ref(cursor)?;
    let version = cursor.u64()?;
    let blob = decode_blob(cursor)?;
    let profile = match cursor.u8()? {
        0 => MappingProfile::TypedRecordsV1,
        _ => return Err(ImportError::UnsupportedProfile),
    };
    cursor.zeros(7)?;
    MappingBinding::new(evidence, version, blob, profile)
}

pub(crate) fn encode_blob(output: &mut Vec<u8>, value: BlobReference) {
    encode_scope(output, value.scope());
    output.extend_from_slice(&value.id().as_bytes());
    output.extend_from_slice(&value.byte_len().to_be_bytes());
    output.extend_from_slice(&value.chunk_count().to_be_bytes());
    output.extend_from_slice(&[0; 4]);
    output.extend_from_slice(&value.content_digest());
}

pub(crate) fn decode_blob(cursor: &mut Cursor<'_>) -> Result<BlobReference, ImportError> {
    BlobReference::new(
        decode_scope(cursor)?,
        BlobId::from_bytes(cursor.array()?),
        cursor.u64()?,
        cursor.u32()?,
        {
            cursor.zeros(4)?;
            cursor.array()?
        },
    )
    .map_err(|_| ImportError::InvalidEncoding)
}

pub(crate) fn encode_scope(output: &mut Vec<u8>, scope: NamespaceRef) {
    output.extend_from_slice(scope.database().as_bytes());
    output.extend_from_slice(scope.namespace().as_bytes());
}

pub(crate) fn decode_scope(cursor: &mut Cursor<'_>) -> Result<NamespaceRef, ImportError> {
    Ok(NamespaceRef::new(
        DatabaseId::from_bytes(cursor.array()?),
        NamespaceId::from_bytes(cursor.array()?),
    ))
}

pub(crate) fn encode_record_ref(output: &mut Vec<u8>, value: RecordRef) {
    encode_scope(
        output,
        NamespaceRef::new(value.database(), value.namespace()),
    );
    output.extend_from_slice(value.record().as_bytes());
}

pub(crate) fn decode_record_ref(cursor: &mut Cursor<'_>) -> Result<RecordRef, ImportError> {
    let scope = decode_scope(cursor)?;
    Ok(RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(cursor.array()?),
    ))
}

pub(crate) fn encode_source_event(output: &mut Vec<u8>, value: SourceEventRef) {
    encode_scope(
        output,
        NamespaceRef::new(value.database(), value.namespace()),
    );
    output.extend_from_slice(value.source_event().as_bytes());
}

pub(crate) fn decode_source_event(cursor: &mut Cursor<'_>) -> Result<SourceEventRef, ImportError> {
    let scope = decode_scope(cursor)?;
    Ok(SourceEventRef::new(
        scope.database(),
        scope.namespace(),
        SourceEventId::from_bytes(cursor.array()?),
    ))
}

fn encode_frame(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ImportError> {
    let length = u64::try_from(bytes.len()).map_err(|_| ImportError::ResourceLimit)?;
    let next = output
        .len()
        .checked_add(8)
        .and_then(|value| value.checked_add(bytes.len()))
        .ok_or(ImportError::ResourceLimit)?;
    if next > MAX_IMPORT_REQUEST_BYTES {
        return Err(ImportError::ResourceLimit);
    }
    reserve(output, 8 + bytes.len())?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn reserve(output: &mut Vec<u8>, additional: usize) -> Result<(), ImportError> {
    output
        .try_reserve(additional)
        .map_err(|_| ImportError::ResourceLimit)
}

fn owned_frame(cursor: &mut Cursor<'_>) -> Result<Vec<u8>, ImportError> {
    let bytes = cursor.frame()?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| ImportError::ResourceLimit)?;
    output.extend_from_slice(bytes);
    Ok(output)
}

fn revision(value: u64) -> Result<CommitRevision, ImportError> {
    CommitRevision::new(value).map_err(|_| ImportError::InvalidCheckpoint)
}

pub(crate) struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.offset)
    }

    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], ImportError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ImportError::ResourceLimit)?;
        let value = self
            .input
            .get(self.offset..end)
            .ok_or(ImportError::InvalidEncoding)?;
        self.offset = end;
        Ok(value)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], ImportError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ImportError::InvalidEncoding)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ImportError> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ImportError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ImportError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub(crate) fn zeros(&mut self, length: usize) -> Result<(), ImportError> {
        if self.take(length)?.iter().all(|byte| *byte == 0) {
            Ok(())
        } else {
            Err(ImportError::InvalidEncoding)
        }
    }

    pub(crate) fn frame(&mut self) -> Result<&'a [u8], ImportError> {
        let length = usize::try_from(self.u64()?).map_err(|_| ImportError::ResourceLimit)?;
        self.take(length)
    }
}
