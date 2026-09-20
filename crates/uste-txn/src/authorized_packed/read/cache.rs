use super::*;
use uste_storage::packed_page_cache::{PackedCacheReport, PackedPageCache};

impl<'a, S, F, W, E, I> AuthorizedPackedReader<'a, S, F, W, E, I>
where
    S: AuthorizedPackedReadState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted adapter configuration. Requesting consumers cannot select cache or work budgets.
    pub fn new_with_cache_budget(
        inner: &'a PackedCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        limits: S::ReadLimits,
        cache_bytes: usize,
    ) -> Result<Self, AuthorizedError> {
        let metadata = AuthorizedPackedMetadata::new(inner, policy)?;
        let cache =
            PackedPageCache::new(cache_bytes).map_err(|_| AuthorizedError::ResourceLimit)?;
        Ok(Self {
            metadata,
            limits,
            cache: std::sync::Mutex::new(Some(cache)),
        })
    }

    /// Cardinality-sensitive logical accounting, not device I/O or process RSS.
    /// None indicates the compatibility constructor's uncached mode.
    pub fn cache_report(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<Option<PackedCacheReport>, AuthorizedError> {
        self.metadata.authorize(principal, Action::ManageSchema)?;
        self.cache
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)?
            .as_ref()
            .map(PackedPageCache::report)
            .transpose()
            .map_err(|_| AuthorizedError::ResourceLimit)
    }

    /// Drop USTE's resident pages/binding, retaining counters; does not clear host caches.
    pub fn clear_cache(&self, principal: &AuthenticatedPrincipal) -> Result<(), AuthorizedError> {
        self.metadata.authorize(principal, Action::ManageSchema)?;
        if let Some(cache) = self
            .cache
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)?
            .as_mut()
        {
            cache.clear();
        }
        Ok(())
    }
}
