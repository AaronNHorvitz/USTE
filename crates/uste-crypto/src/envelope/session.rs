//! Opaque process-local cache binding. Not a key, authorization token or persisted identity.
use std::sync::Arc;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct UnlockedKeySession(Arc<()>);
impl UnlockedKeySession {
    pub(super) fn new() -> Self {
        Self(Arc::new(()))
    }
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl core::fmt::Debug for UnlockedKeySession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("UnlockedKeySession([REDACTED])")
    }
}
