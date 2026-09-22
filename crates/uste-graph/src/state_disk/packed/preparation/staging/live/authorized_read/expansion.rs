use super::*;
use crate::state_disk::authorized_read::{ExpansionRead, ExpansionScanEntry, read_with};
use uste_storage::packed_tree_cursor::{MAX_CURSOR_ENCODED_BYTES, MAX_CURSOR_PAGES};
use uste_storage::packed_tree_lookup::{MAX_LOOKUP_ENCODED_BYTES, MAX_LOOKUP_PAGES};
use uste_storage::{MAX_INDEX_RESULT_BYTES, MAX_INDEX_VALUE_BYTES};
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
    ) -> Result<Option<Vec<ExpansionScanEntry>>, GraphDiskError> {
        let record_id = id.record();
        let lower = record_id.as_bytes();
        let (upper, upper_len) = prefix_upper_bound(lower);
        let upper = (upper_len != 0).then_some(&upper[..upper_len]);
        let limits = TreeCursorLimits {
            // Secondary keys have exactly 32 bytes (9 bits per byte plus terminator).
            maximum_path_branches: 289,
            maximum_candidates: self.budget.candidates,
            maximum_returned_bytes: self.budget.returned_bytes,
            maximum_pages: self.budget.pages,
            maximum_encoded_bytes: self.budget.encoded_bytes,
        };
        if self
            .cache
            .as_deref()
            .is_some_and(PackedPageCache::has_range_cache)
        {
            let result = self.index.range_cached_with(
                self.fs,
                &self.base.trees[usize::from(family - 1)],
                lower,
                upper,
                limits,
                false,
                self.cache
                    .as_deref_mut()
                    .ok_or(StorageError::InvalidState)?,
                |key, value| compact_entry(family, lower, key, value),
            )?;
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(result.entries.len())
                .map_err(|_| StorageError::ResourceLimit)?;
            for entry in result.entries {
                entries.push(entry?);
            }
            self.budget.charge(
                result.report.pages,
                result.report.encoded_bytes,
                result.report.candidates,
                result.report.returned_bytes,
            )?;
            return Ok(Some(entries));
        }
        let mut cursor = self.index.cursor(
            &self.base.trees[usize::from(family - 1)],
            lower,
            upper,
            limits,
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
            entries
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            entries.push(compact_entry(family, lower, entry.key(), entry.value())?);
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

fn compact_entry(
    family: u8,
    lower: &[u8; 16],
    key: &[u8],
    value: &[u8],
) -> Result<ExpansionScanEntry, GraphDiskError> {
    if key.len() != 32
        || !key.starts_with(lower)
        || (family == FAMILY_PROVENANCE && !value.is_empty())
        || (family != FAMILY_PROVENANCE && value.len() != 16)
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(ExpansionScanEntry {
        id: key[16..]
            .try_into()
            .map_err(|_| GraphDiskError::IndexCorrupt)?,
        neighbor: if family == FAMILY_PROVENANCE {
            None
        } else {
            Some(value.try_into().map_err(|_| GraphDiskError::IndexCorrupt)?)
        },
    })
}

/// Return the shortest exclusive upper bound for a fixed-width prefix. A zero length denotes an
/// unbounded upper range when every byte is `0xff`. Keeping the scratch value inline avoids a
/// temporary heap allocation before the cursor copies its durable bound.
fn prefix_upper_bound<const N: usize>(prefix: &[u8; N]) -> ([u8; N], usize) {
    let mut upper = *prefix;
    let mut len = N;
    while len != 0 && upper[len - 1] == u8::MAX {
        len -= 1;
    }
    if len != 0 {
        upper[len - 1] += 1;
    }
    (upper, len)
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

#[cfg(test)]
mod tests {
    use super::prefix_upper_bound;

    #[test]
    fn prefix_upper_bound_is_inline_minimal_and_carry_safe() {
        let (upper, len) = prefix_upper_bound(&[0x12, 0x34, 0x56]);
        assert_eq!(&upper[..len], &[0x12, 0x34, 0x57]);

        let (upper, len) = prefix_upper_bound(&[0x12, 0x34, 0xff]);
        assert_eq!(&upper[..len], &[0x12, 0x35]);

        let (upper, len) = prefix_upper_bound(&[0x12, 0xff, 0xff]);
        assert_eq!(&upper[..len], &[0x13]);

        let (_, len) = prefix_upper_bound(&[0xff, 0xff, 0xff]);
        assert_eq!(len, 0);
    }
}
