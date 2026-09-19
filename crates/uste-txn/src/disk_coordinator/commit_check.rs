//! Shared admission before expensive external preparation. This is not a reservation.

use super::*;
use crate::commit_admission::CommitAdmission;

/// Privileged preflight result, not consumer authority or a durable commit acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskCommitCheck {
    Retry(TransactionOutcome),
    Ready { revision: CommitRevision },
}

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Apply the exact commit admission rules without preparing or publishing domain state.
    /// Callers must authorize first. A subsequent commit repeats admission: this result reserves
    /// nothing and cannot bypass changed state, expiry, cancellation, collision or owner checks.
    #[allow(clippy::too_many_arguments)]
    pub fn check_commit(
        &self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<DiskCommitCheck, TransactionError> {
        let admission = self.inner.admit_commit(
            filesystem,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: &self.base,
                overlay: self.admitted_overlay_limits(),
                lookup,
                cache,
            }),
        )?;
        Ok(match admission {
            CommitAdmission::Retry(outcome) => DiskCommitCheck::Retry(outcome),
            CommitAdmission::Fresh(admission) => DiskCommitCheck::Ready {
                revision: admission.revision,
            },
        })
    }
}
