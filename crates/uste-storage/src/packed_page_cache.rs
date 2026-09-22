//! Privileged immutable packed-page cache; no caller authorization or root admission.
use crate::{
    FileSystem, MAX_INDEX_CACHE_BYTES, MIN_INDEX_CACHE_BYTES,
    index::cache::CachePages,
    journal::{CertificateAnchorProof, StorageError},
    packed_index_pack::read_linked_record_page,
    packed_index_page::{ENCODED_PAGE_BYTES, MAX_SLOTS, PAGE_BYTES, PackedPage, PackedPageContext},
};
use std::sync::Arc;
use uste_crypto::{CryptoError, EntropySource, KeyVault, UnlockedKeySession};
mod lookup;
mod range;
use crate::packed_tree_lookup::{TreeLookupLimits, TreeLookupReport};
pub(crate) use lookup::Identity as LookupIdentity;
pub use lookup::PackedLookupCacheReport;
pub use range::PackedRangeCacheReport;
#[cfg(test)]
mod tests;

const FIXED_BYTES: usize = 8 * 1024;
const ENTRY_BYTES: usize = PAGE_BYTES + 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedCacheReport {
    pub budget_bytes: usize,
    /// Page partition; equals the total in the unchanged page-only mode.
    pub page_budget_bytes: usize,
    pub accounted_bytes: usize,
    pub resident_pages: usize,
    pub hits: u64,
    /// Cache misses, including failed page-load attempts; not physical I/O.
    pub misses: u64,
    pub evictions: u64,
    /// Included in total accounted bytes, never add it a second time.
    pub lookup: Option<PackedLookupCacheReport>,
    /// Included in total accounted bytes, never add it a second time.
    pub range: Option<PackedRangeCacheReport>,
}
/// Bounded resident plaintext with logical accounting, not an RSS or erasure guarantee.
/// Private readers may retain a bounded page handle during proof validation after eviction.
/// Key locking invalidates access immediately; pages are dropped on the next checked access
/// or explicit clear/drop, not synchronously by an independently owned vault.
pub struct PackedPageCache {
    budget: usize,
    page_budget: usize,
    lookup: Option<lookup::LookupCache>,
    range: Option<range::RangeCache>,
    pages: CachePages<PackedPageContext, Arc<PackedPage>>,
    owner: Option<CertificateAnchorProof>,
    session: Option<UnlockedKeySession>,
    hits: u64,
    misses: u64,
    evictions: u64,
    overflowed: bool,
}
impl core::fmt::Debug for PackedPageCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PackedPageCache")
            .field("budget", &self.budget)
            .field("resident_pages", &self.pages.len())
            .finish_non_exhaustive()
    }
}
impl PackedPageCache {
    #[cfg(test)]
    pub(crate) fn assert_metadata_allowance(&self) {
        assert!(
            self.pages.allocated_metadata_bytes() + size_of::<Self>()
                <= FIXED_BYTES + self.pages.len() * 1024
        );
    }
    pub fn new(budget: usize) -> Result<Self, StorageError> {
        if !(MIN_INDEX_CACHE_BYTES..=MAX_INDEX_CACHE_BYTES).contains(&budget) {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            budget,
            page_budget: budget,
            lookup: None,
            range: None,
            pages: CachePages::default(),
            owner: None,
            session: None,
            hits: 0,
            misses: 0,
            evictions: 0,
            overflowed: false,
        })
    }
    /// Trusted opt-in partition of one total budget, not an additional unreported cache.
    /// Positive results still require exact owner/session/root binding and normal authorization.
    pub fn new_with_lookup_budget(total: usize, lookup_bytes: usize) -> Result<Self, StorageError> {
        let mut cache = Self::new(total)?;
        let page_budget = total
            .checked_sub(lookup_bytes)
            .ok_or(StorageError::ResourceLimit)?;
        if page_budget < MIN_INDEX_CACHE_BYTES {
            return Err(StorageError::ResourceLimit);
        }
        cache.lookup = Some(lookup::LookupCache::new(lookup_bytes)?);
        cache.page_budget = page_budget;
        Ok(cache)
    }
    /// Trusted opt-in split of one total budget across pages, positive lookups and complete ranges.
    /// Existing constructors retain their exact page-only or page/lookup behavior.
    pub fn new_with_lookup_and_range_budget(
        total: usize,
        lookup_bytes: usize,
        range_bytes: usize,
    ) -> Result<Self, StorageError> {
        let mut cache = Self::new(total)?;
        let page_budget = total
            .checked_sub(lookup_bytes)
            .and_then(|bytes| bytes.checked_sub(range_bytes))
            .ok_or(StorageError::ResourceLimit)?;
        if page_budget < MIN_INDEX_CACHE_BYTES {
            return Err(StorageError::ResourceLimit);
        }
        cache.lookup = Some(lookup::LookupCache::new(lookup_bytes)?);
        cache.range = Some(range::RangeCache::new(range_bytes)?);
        cache.page_budget = page_budget;
        Ok(cache)
    }
    pub(crate) fn has_lookup_cache(&self) -> bool {
        self.lookup.is_some()
    }
    pub fn has_range_cache(&self) -> bool {
        self.range.is_some()
    }
    /// Clear only this USTE cache and its binding, not host caches or cumulative counters.
    pub fn clear(&mut self) {
        self.pages = CachePages::default();
        self.owner = None;
        self.session = None;
        if let Some(lookup) = &mut self.lookup {
            lookup.clear();
        }
        if let Some(range) = &mut self.range {
            range.clear();
        }
    }
    pub fn report(&self) -> Result<PackedCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        let lookup = self
            .lookup
            .as_ref()
            .map(lookup::LookupCache::report)
            .transpose()?;
        let range = self
            .range
            .as_ref()
            .map(range::RangeCache::report)
            .transpose()?;
        let page_bytes = if self.pages.is_empty() {
            0
        } else {
            FIXED_BYTES + self.pages.len() * ENTRY_BYTES
        };
        Ok(PackedCacheReport {
            budget_bytes: self.budget,
            page_budget_bytes: self.page_budget,
            accounted_bytes: page_bytes
                .checked_add(lookup.map_or(0, |r| r.accounted_bytes))
                .and_then(|bytes| bytes.checked_add(range.map_or(0, |r| r.accounted_bytes)))
                .ok_or(StorageError::ResourceLimit)?,
            resident_pages: self.pages.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            lookup,
            range,
        })
    }
    pub(crate) fn bind(
        &mut self,
        proof: &CertificateAnchorProof,
        session: &UnlockedKeySession,
    ) -> Result<(), StorageError> {
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !old.same_owner_as(proof))
        {
            return Err(StorageError::InvalidState);
        }
        if self
            .session
            .as_ref()
            .is_some_and(|old| !old.same_session(session))
        {
            self.clear();
        }
        if self.owner.is_none() {
            self.owner = Some(proof.clone());
            self.session = Some(session.clone());
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn page<F: FileSystem, W, E: EntropySource>(
        &mut self,
        fs: &mut F,
        directory: &F::Directory,
        vault: &KeyVault<W, E>,
        context: PackedPageContext,
        slot: u16,
    ) -> Result<Arc<PackedPage>, StorageError> {
        context.validate()?;
        if slot >= MAX_SLOTS {
            return Err(StorageError::InvalidState);
        }
        let session = match vault.unlocked_session() {
            Ok(session) => session,
            Err(error) => {
                self.clear();
                return Err(StorageError::Crypto(error));
            }
        };
        if self.owner.is_none()
            || !self
                .session
                .as_ref()
                .is_some_and(|old| old.same_session(session))
        {
            return Err(StorageError::Crypto(CryptoError::InvalidContext));
        }
        if let Some(index) = self.pages.touch(&context) {
            increment(&mut self.hits, &mut self.overflowed);
            let page = self
                .pages
                .page_mut(index)
                .ok_or(StorageError::IntegrityFailure)?;
            if page.record(slot).is_none() {
                return Err(StorageError::IntegrityFailure);
            }
            return Ok(Arc::clone(page));
        }
        increment(&mut self.misses, &mut self.overflowed);
        let (page, _) = read_linked_record_page(
            fs,
            directory,
            vault,
            context,
            slot,
            ENCODED_PAGE_BYTES as u64,
        )?;
        let page = Arc::new(page);
        let capacity = (self.page_budget - FIXED_BYTES) / ENTRY_BYTES;
        let (_, evicted) = self.pages.insert(context, Arc::clone(&page), capacity)?;
        if evicted {
            increment(&mut self.evictions, &mut self.overflowed);
        }
        Ok(page)
    }
    pub(crate) fn lookup_get<W, E: EntropySource>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<Option<lookup::CachedLookup>, StorageError> {
        if self.lookup.is_none() {
            return Ok(None);
        }
        self.check_lookup_session(vault)?;
        self.lookup
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .get(identity, key, limits)
    }
    pub(crate) fn lookup_get_with<W, E: EntropySource, R, F: FnOnce(&[u8]) -> R>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        key: &[u8],
        limits: TreeLookupLimits,
        map: &mut Option<F>,
    ) -> Result<Option<(R, TreeLookupReport)>, StorageError> {
        if self.lookup.is_none() {
            return Ok(None);
        }
        self.check_lookup_session(vault)?;
        self.lookup
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .get_with(identity, key, limits, map)
    }
    pub(crate) fn lookup_insert<W, E: EntropySource>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        key: &[u8],
        bytes: &[u8],
        report: TreeLookupReport,
    ) -> Result<(), StorageError> {
        if self.lookup.is_none() {
            return Ok(());
        }
        self.check_lookup_session(vault)?;
        self.lookup
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .insert(identity, key, bytes, report)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn range_get<W, E: EntropySource>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        limits: crate::packed_tree_cursor::TreeCursorLimits,
    ) -> Result<Option<range::CachedRange>, StorageError> {
        if self.range.is_none() {
            return Ok(None);
        }
        self.check_lookup_session(vault)?;
        self.range
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .get(identity, lower, upper, reverse, limits)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn range_get_with<W, E: EntropySource, T>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        limits: crate::packed_tree_cursor::TreeCursorLimits,
        map: &mut impl FnMut(&[u8], &[u8]) -> Result<T, StorageError>,
    ) -> Result<Option<range::CachedRange<T>>, StorageError> {
        if self.range.is_none() {
            return Ok(None);
        }
        self.check_lookup_session(vault)?;
        self.range
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .get_with(identity, lower, upper, reverse, limits, map)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn range_insert<W, E: EntropySource>(
        &mut self,
        vault: &KeyVault<W, E>,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        entries: &[crate::packed_tree_cursor::PackedCursorEntry],
        report: crate::packed_tree_cursor::TreeCursorReport,
    ) -> Result<(), StorageError> {
        if self.range.is_none() {
            return Ok(());
        }
        self.check_lookup_session(vault)?;
        self.range
            .as_mut()
            .ok_or(StorageError::InvalidState)?
            .insert(identity, lower, upper, reverse, entries, report)
    }
    fn check_lookup_session<W, E: EntropySource>(
        &mut self,
        vault: &KeyVault<W, E>,
    ) -> Result<(), StorageError> {
        let session = match vault.unlocked_session() {
            Ok(session) => session,
            Err(error) => {
                self.clear();
                return Err(StorageError::Crypto(error));
            }
        };
        if self.owner.is_none()
            || !self
                .session
                .as_ref()
                .is_some_and(|old| old.same_session(session))
        {
            return Err(StorageError::Crypto(CryptoError::InvalidContext));
        }
        Ok(())
    }
}
fn increment(counter: &mut u64, overflowed: &mut bool) {
    if let Some(next) = counter.checked_add(1) {
        *counter = next;
    } else {
        *overflowed = true;
    }
}
