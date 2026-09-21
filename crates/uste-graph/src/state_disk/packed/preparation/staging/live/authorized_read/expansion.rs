use super::*;
use crate::state_disk::authorized_read::{ExpansionRead, read_with};
use uste_storage::packed_tree_cursor::{MAX_CURSOR_ENCODED_BYTES, MAX_CURSOR_PAGES};
use uste_storage::packed_tree_lookup::{MAX_LOOKUP_ENCODED_BYTES, MAX_LOOKUP_PAGES};
use uste_storage::{IndexScanEntry, MAX_INDEX_RESULT_BYTES, MAX_INDEX_VALUE_BYTES};
use uste_txn::PackedIndexReader;

/// Aggregate per-query work; trusted configuration, never consumer-controlled admission.
#[derive(Clone, Copy, Debug)]
pub struct PackedGraphExpansionLimits {
    pages: u64,
    encoded_bytes: u64,
    candidates: u64,
    returned_bytes: u64,
    lookups: u64,
}
impl PackedGraphExpansionLimits {
    pub fn new(
        pages: u64,
        encoded_bytes: u64,
        candidates: u64,
        returned_bytes: u64,
        lookups: u64,
    ) -> Result<Self, StorageError> {
        if pages > MAX_CURSOR_PAGES
            || encoded_bytes > MAX_CURSOR_ENCODED_BYTES
            || candidates > crate::MAX_TRAVERSAL_VISITS as u64
            || returned_bytes > MAX_INDEX_RESULT_BYTES as u64
            || lookups > 2 * crate::MAX_TRAVERSAL_VISITS as u64
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            pages,
            encoded_bytes,
            candidates,
            returned_bytes,
            lookups,
        })
    }
    fn charge(
        &mut self,
        pages: u64,
        encoded: u64,
        candidates: u64,
        returned: u64,
    ) -> Result<(), StorageError> {
        self.pages = self
            .pages
            .checked_sub(pages)
            .ok_or(StorageError::ResourceLimit)?;
        self.encoded_bytes = self
            .encoded_bytes
            .checked_sub(encoded)
            .ok_or(StorageError::ResourceLimit)?;
        self.candidates = self
            .candidates
            .checked_sub(candidates)
            .ok_or(StorageError::ResourceLimit)?;
        self.returned_bytes = self
            .returned_bytes
            .checked_sub(returned)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }
}
struct Reader<
    'a,
    'j,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
> {
    index: &'a PackedIndexReader<'j, F, W, E, I>,
    fs: &'a mut F,
    base: &'a PackedGraphBase,
    budget: PackedGraphExpansionLimits,
    cache: Option<&'a mut PackedPageCache>,
}
impl<F: OwnershipFileSystem, W: DurableKeyEnvelope, E: EntropySource, I: EntropySource>
    ExpansionRead for Reader<'_, '_, F, W, E, I>
{
    fn scan(
        &mut self,
        family: u8,
        id: RecordRef,
    ) -> Result<Option<Vec<IndexScanEntry>>, GraphDiskError> {
        let record_id = id.record();
        let lower = record_id.as_bytes();
        let mut upper = lower.to_vec();
        while upper.last() == Some(&255) {
            upper.pop();
        }
        let upper = if let Some(last) = upper.last_mut() {
            *last += 1;
            Some(upper)
        } else {
            None
        };
        let mut cursor = self.index.cursor(
            &self.base.trees[usize::from(family - 1)],
            lower,
            upper.as_deref(),
            TreeCursorLimits {
                // Secondary keys have exactly 32 bytes (9 bits per byte plus terminator).
                maximum_path_branches: 289,
                maximum_candidates: self.budget.candidates,
                maximum_returned_bytes: self.budget.returned_bytes,
                maximum_pages: self.budget.pages,
                maximum_encoded_bytes: self.budget.encoded_bytes,
            },
        )?;
        let mut entries = Vec::new();
        loop {
            let entry = match self.cache.as_deref_mut() {
                Some(cache) => self.index.next_cached(self.fs, &mut cursor, cache),
                None => self.index.next(self.fs, &mut cursor),
            }?;
            let Some(entry) = entry else {
                break;
            };
            if entry.key().len() != 32
                || !entry.key().starts_with(lower)
                || (family == FAMILY_PROVENANCE && !entry.value().is_empty())
                || (family != FAMILY_PROVENANCE && entry.value().len() != 16)
            {
                return Err(GraphDiskError::IndexCorrupt);
            }
            entries
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            entries.push(IndexScanEntry {
                key: entry.key().to_vec(),
                value: entry.value().to_vec(),
            });
        }
        let report = cursor.report();
        self.budget.charge(
            report.pages,
            report.encoded_bytes,
            report.candidates,
            report.returned_bytes,
        )?;
        Ok(Some(entries))
    }
    fn record(&mut self, id: RecordRef) -> Result<Record, GraphDiskError> {
        self.budget.lookups = self
            .budget
            .lookups
            .checked_sub(1)
            .ok_or(StorageError::ResourceLimit)?;
        let limits = TreeLookupLimits {
            maximum_path_branches: 145,
            maximum_pages: self.budget.pages.min(MAX_LOOKUP_PAGES),
            maximum_encoded_bytes: self.budget.encoded_bytes.min(MAX_LOOKUP_ENCODED_BYTES),
            maximum_value_bytes: self.budget.returned_bytes.min(MAX_INDEX_VALUE_BYTES as u64),
        };
        let tree = &self.base.trees[usize::from(FAMILY_CURRENT_RECORD - 1)];
        let key = id.record();
        let (record, returned, report) = match self.cache.as_deref_mut() {
            Some(cache) => {
                let result = self.index.get_cached_with(
                    self.fs,
                    tree,
                    key.as_bytes(),
                    limits,
                    cache,
                    |bytes| (decode_stored_record(bytes), bytes.len() as u64),
                )?;
                let (record, returned) = result.value.ok_or(GraphDiskError::IndexCorrupt)?;
                (record?, returned, result.report)
            }
            None => {
                let result = self.index.get(self.fs, tree, key.as_bytes(), limits)?;
                let value = result.value.ok_or(GraphDiskError::IndexCorrupt)?;
                (
                    decode_stored_record(value.as_slice())?,
                    value.as_slice().len() as u64,
                    result.report,
                )
            }
        };
        self.budget
            .charge(report.pages, report.encoded_bytes, 0, returned)?;
        if record.id() != id || record.modified_revision() > self.base.anchor.0 {
            return Err(GraphDiskError::IndexCorrupt);
        }
        Ok(record)
    }
    fn reference(&self, bytes: &[u8]) -> Result<RecordRef, GraphDiskError> {
        record_key(self.base.scope, bytes).map_err(GraphDiskError::Storage)
    }
}
pub(super) fn read<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    index: &PackedIndexReader<'_, F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    request: &GraphReadRequest,
    budget: PackedGraphExpansionLimits,
    cache: Option<&mut PackedPageCache>,
    authorize: &mut dyn FnMut(Action, Target) -> bool,
) -> Result<GraphReadOutput, GraphDiskError> {
    read_with(
        &mut Reader {
            index,
            fs,
            base,
            budget,
            cache,
        },
        request,
        authorize,
    )
}
