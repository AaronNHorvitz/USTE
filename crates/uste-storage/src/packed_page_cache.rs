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
#[cfg(test)]
mod tests;

const FIXED_BYTES: usize = 8 * 1024;
const ENTRY_BYTES: usize = PAGE_BYTES + 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedCacheReport {
    pub budget_bytes: usize,
    pub accounted_bytes: usize,
    pub resident_pages: usize,
    pub hits: u64,
    /// Cache misses, including failed page-load attempts; not physical I/O.
    pub misses: u64,
    pub evictions: u64,
}
/// Bounded resident plaintext with logical accounting, not an RSS or erasure guarantee.
/// Private readers may retain a bounded page handle during proof validation after eviction.
/// Key locking invalidates access immediately; pages are dropped on the next checked access
/// or explicit clear/drop, not synchronously by an independently owned vault.
pub struct PackedPageCache {
    budget: usize,
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
            pages: CachePages::default(),
            owner: None,
            session: None,
            hits: 0,
            misses: 0,
            evictions: 0,
            overflowed: false,
        })
    }
    /// Clear only this USTE cache and its binding, not host caches or cumulative counters.
    pub fn clear(&mut self) {
        self.pages = CachePages::default();
        self.owner = None;
        self.session = None;
    }
    pub fn report(&self) -> Result<PackedCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        Ok(PackedCacheReport {
            budget_bytes: self.budget,
            accounted_bytes: if self.pages.is_empty() {
                0
            } else {
                FIXED_BYTES + self.pages.len() * ENTRY_BYTES
            },
            resident_pages: self.pages.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
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
        let capacity = (self.budget - FIXED_BYTES) / ENTRY_BYTES;
        let (_, evicted) = self.pages.insert(context, Arc::clone(&page), capacity)?;
        if evicted {
            increment(&mut self.evictions, &mut self.overflowed);
        }
        Ok(page)
    }
}
fn increment(counter: &mut u64, overflowed: &mut bool) {
    if let Some(next) = counter.checked_add(1) {
        *counter = next;
    } else {
        *overflowed = true;
    }
}
