use crate::packed_index_page::PackedPage;
use std::sync::Arc;

/// Uncached reads retain their existing allocation; cached reads borrow via one bounded Arc.
pub(crate) enum ReadPage {
    Owned(PackedPage),
    Cached(Arc<PackedPage>),
}
impl core::ops::Deref for ReadPage {
    type Target = PackedPage;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(page) => page,
            Self::Cached(page) => page,
        }
    }
}
