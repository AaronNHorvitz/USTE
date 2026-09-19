//! Fixed-size sparse fences derived only while fully validating an immutable plaintext page.
use super::*;

const STRIDE: usize = 32;
const MAX_FRAGMENTS: usize = (INDEX_PAGE_BYTES - PAGE_HEADER_BYTES) / (FRAGMENT_HEADER_BYTES + 1);
const FENCES: usize = MAX_FRAGMENTS.div_ceil(STRIDE);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SparseDirectory {
    offsets: [u16; FENCES],
}

pub(super) struct SearchStart {
    pub offset: usize,
    pub ordinal: usize,
    pub probes: u64,
}

impl SparseDirectory {
    pub fn record(&mut self, ordinal: usize, offset: usize) -> Result<(), StorageError> {
        if ordinal >= MAX_FRAGMENTS || !(PAGE_HEADER_BYTES..INDEX_PAGE_BYTES).contains(&offset) {
            return Err(StorageError::IntegrityFailure);
        }
        if ordinal.is_multiple_of(STRIDE) {
            self.offsets[ordinal / STRIDE] =
                u16::try_from(offset).map_err(|_| StorageError::IntegrityFailure)?;
        }
        Ok(())
    }

    /// Return a block at or before the first fragment whose key is >= `key`. Equality starts
    /// one block earlier: a repeated key may begin before the first equal sampled fence.
    /// The caller still validates/assembles exact fragment offsets and lengths during traversal.
    pub fn start(
        &self,
        bytes: &[u8],
        fragment_count: usize,
        key: &[u8],
    ) -> Result<SearchStart, StorageError> {
        if fragment_count == 0 || fragment_count > MAX_FRAGMENTS {
            return Err(StorageError::IntegrityFailure);
        }
        if fragment_count <= STRIDE {
            return Ok(SearchStart {
                offset: PAGE_HEADER_BYTES,
                ordinal: 0,
                probes: 0,
            });
        }
        let mut low = 0;
        let mut high = fragment_count.div_ceil(STRIDE);
        let mut probes = 0;
        while low < high {
            let middle = low + (high - low) / 2;
            let offset = usize::from(self.offsets[middle]);
            let key_len = usize::try_from(read_u32(bytes, offset)?)
                .map_err(|_| StorageError::IntegrityFailure)?;
            let start = offset
                .checked_add(FRAGMENT_HEADER_BYTES)
                .ok_or(StorageError::IntegrityFailure)?;
            let end = start
                .checked_add(key_len)
                .ok_or(StorageError::IntegrityFailure)?;
            let fence = bytes
                .get(start..end)
                .ok_or(StorageError::IntegrityFailure)?;
            probes += 1;
            if fence < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let block = low.saturating_sub(1);
        Ok(SearchStart {
            offset: usize::from(self.offsets[block]),
            ordinal: block * STRIDE,
            probes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_fences_never_skip_the_linear_first_match_even_across_duplicate_blocks() {
        assert_eq!(size_of::<SparseDirectory>(), 60);
        for count in [1_usize, 31, 32, 33, 64, 65, 257, 858] {
            for repeated in [1_usize, 2, 31, 32, 33, 127, 858] {
                let mut bytes = vec![0; PAGE_HEADER_BYTES];
                let mut directory = SparseDirectory::default();
                let mut keys = Vec::new();
                let mut offsets = Vec::new();
                for ordinal in 0..count {
                    directory.record(ordinal, bytes.len()).unwrap();
                    offsets.push(bytes.len());
                    let key = u16::try_from(ordinal / repeated * 3).unwrap().to_be_bytes();
                    keys.push(key);
                    bytes.extend_from_slice(&2_u32.to_be_bytes());
                    bytes.extend_from_slice(&(repeated as u32).to_be_bytes());
                    bytes.extend_from_slice(&((ordinal % repeated) as u32).to_be_bytes());
                    bytes.extend_from_slice(&1_u32.to_be_bytes());
                    bytes.extend_from_slice(&key);
                    bytes.push(1);
                }
                assert!(bytes.len() <= INDEX_PAGE_BYTES);
                for target in 0..=u16::try_from(count / repeated * 3 + 3).unwrap() {
                    let target = target.to_be_bytes();
                    let expected = keys.iter().position(|key| key >= &target).unwrap_or(count);
                    let start = directory.start(&bytes, count, &target).unwrap();
                    assert!(start.ordinal <= expected);
                    assert!(expected - start.ordinal <= STRIDE);
                    assert_eq!(start.offset, offsets[start.ordinal]);
                    assert!(start.probes <= 5);
                    let selected = FragmentIter {
                        remaining: &bytes[start.offset..],
                        remaining_count: count - start.ordinal,
                    }
                    .map(Result::unwrap)
                    .find(|fragment| fragment.key >= target.as_slice());
                    assert_eq!(
                        selected.map(|fragment| fragment.key),
                        keys.get(expected).map(<[u8; 2]>::as_slice),
                    );
                    if count <= STRIDE {
                        assert_eq!(start.probes, 0);
                    }
                }
            }
        }
    }

    #[test]
    fn sparse_fence_geometry_and_truncation_refuse_without_allocation() {
        let mut directory = SparseDirectory::default();
        assert!(directory.record(MAX_FRAGMENTS, PAGE_HEADER_BYTES).is_err());
        assert!(directory.record(0, PAGE_HEADER_BYTES - 1).is_err());
        assert!(directory.record(0, INDEX_PAGE_BYTES).is_err());
        assert!(directory.start(&[], 0, b"a").is_err());
        assert!(directory.start(&[], MAX_FRAGMENTS + 1, b"a").is_err());
        assert!(directory.start(&[], STRIDE + 1, b"a").is_err());
    }
}
