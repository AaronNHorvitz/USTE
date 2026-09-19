//! Shared authenticated suffix reader borrowing, never moving, the exclusive journal owner.
use super::*;
pub(crate) struct JournalTransactionReader<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub(crate) scope: NamespaceRef,
    pub(crate) journal: &'a JournalStore<F, W, E, I>,
}
impl<F, W, E, I> JournalTransactionReader<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Admit an inclusive range before I/O. This is a trusted recovery capability, not a
    /// consumer-authorized read, and does not establish domain or coordinator validity.
    pub fn open_transaction_cursor(
        &self,
        first: CommitRevision,
        last: CommitRevision,
        maximum_groups: u64,
        maximum_encoded_bytes: u64,
    ) -> Result<TransactionRecoveryCursor, TransactionError> {
        let anchor = self
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::IntegrityFailure)?;
        if first > last || last > anchor.0 {
            return Err(TransactionError::IntegrityFailure);
        }
        let total_groups = last.get() - first.get() + 1;
        if total_groups > maximum_groups {
            return Err(TransactionError::Storage(StorageError::ResourceLimit));
        }
        Ok(TransactionRecoveryCursor {
            scope: self.scope,
            anchor,
            next: Some(first),
            last,
            total_groups,
            initial_encoded_bytes: maximum_encoded_bytes,
            remaining_encoded_bytes: maximum_encoded_bytes,
            completed_groups: 0,
            certificate_window_size: 0,
            certificate_proofs: Vec::new().into_iter(),
            failed: false,
        })
    }

    /// Opt-in fixed-size lookahead for disk-certificate mode. Each window is fully authenticated
    /// before yielding its first transaction; all acquisition reads debit the same range budget.
    /// Resident-anchor mode preserves its existing per-group behavior. No full history is kept.
    pub fn open_transaction_cursor_with_certificate_window(
        &self,
        first: CommitRevision,
        last: CommitRevision,
        maximum_groups: u64,
        maximum_encoded_bytes: u64,
        window_size: u64,
    ) -> Result<TransactionRecoveryCursor, TransactionError> {
        if window_size == 0 || window_size > uste_storage::journal::MAX_CERTIFICATE_PROOF_WINDOW {
            return Err(TransactionError::ResourceLimit);
        }
        let mut cursor =
            self.open_transaction_cursor(first, last, maximum_groups, maximum_encoded_bytes)?;
        cursor.certificate_window_size = window_size;
        Ok(cursor)
    }

    /// Yield one completely authenticated canonical transaction and release its encrypted/read
    /// buffers before returning. A failed cursor cannot resume; restart with a fresh admission.
    pub fn next_recovered_transaction(
        &self,
        filesystem: &mut F,
        cursor: &mut TransactionRecoveryCursor,
    ) -> Result<Option<RecoveredFrontierTransaction>, TransactionError> {
        let result = (|| {
            if cursor.failed
                || cursor.scope != self.scope
                || self.journal.checkpoint_anchor() != Some(cursor.anchor)
            {
                return Err(TransactionError::IntegrityFailure);
            }
            let Some(revision) = cursor.next else {
                return Ok(None);
            };
            let proof = if let Some(limits) = self
                .journal
                .certificate_anchor_read_limits()
                .filter(|_| cursor.certificate_window_size != 0)
            {
                if cursor.certificate_proofs.len() == 0 {
                    let window_last = revision
                        .get()
                        .checked_add(cursor.certificate_window_size - 1)
                        .ok_or(TransactionError::ResourceLimit)?
                        .min(cursor.last.get());
                    let window = self
                        .journal
                        .authenticate_certificate_window(
                            filesystem,
                            revision,
                            CommitRevision::new(window_last)
                                .map_err(|_| TransactionError::IntegrityFailure)?,
                            limits,
                            cursor.remaining_encoded_bytes,
                        )
                        .map_err(map_open_error)?;
                    cursor.remaining_encoded_bytes = cursor
                        .remaining_encoded_bytes
                        .checked_sub(window.report().encoded_bytes)
                        .ok_or(TransactionError::IntegrityFailure)?;
                    cursor.certificate_proofs = window.into_proofs();
                }
                let proof = cursor
                    .certificate_proofs
                    .next()
                    .ok_or(TransactionError::IntegrityFailure)?;
                if proof.anchor().0 != revision {
                    return Err(TransactionError::IntegrityFailure);
                }
                Some(proof)
            } else {
                None
            };
            let mut transaction = None;
            let report = if let Some(proof) = proof.as_ref() {
                self.journal.visit_proven_committed_group_report(
                    filesystem,
                    proof,
                    cursor.remaining_encoded_bytes,
                    |_, group| {
                        let mut captured = capture_group(self.scope, group)?;
                        captured.certificate_proof = Some(proof.clone());
                        transaction = Some(captured);
                        Ok(())
                    },
                )
            } else {
                self.journal.visit_committed_range_with_proofs_report(
                    filesystem,
                    revision,
                    revision,
                    1,
                    cursor.remaining_encoded_bytes,
                    |_, group, proof| {
                        let mut captured = capture_group(self.scope, group)?;
                        captured.certificate_proof = proof.cloned();
                        transaction = Some(captured);
                        Ok(())
                    },
                )
            }
            .map_err(map_open_error)?;
            let transaction = transaction.ok_or(TransactionError::IntegrityFailure)?;
            let completed = cursor
                .completed_groups
                .checked_add(report.groups)
                .filter(|completed| *completed <= cursor.total_groups)
                .ok_or(TransactionError::IntegrityFailure)?;
            let remaining = cursor
                .remaining_encoded_bytes
                .checked_sub(report.encoded_bytes)
                .ok_or(TransactionError::IntegrityFailure)?;
            let next = if revision == cursor.last {
                None
            } else {
                Some(
                    revision
                        .checked_next()
                        .map_err(|_| TransactionError::RevisionExhausted)?,
                )
            };
            cursor.completed_groups = completed;
            cursor.remaining_encoded_bytes = remaining;
            cursor.next = next;
            Ok(Some(transaction))
        })();
        if result.is_err() {
            cursor.failed = true;
        }
        result
    }

    /// Release exact terminal range consumption only after every selected group succeeded.
    pub fn finish_transaction_cursor(
        &self,
        cursor: TransactionRecoveryCursor,
    ) -> Result<JournalRangeReadReport, TransactionError> {
        if cursor.failed
            || cursor.next.is_some()
            || cursor.certificate_proofs.len() != 0
            || cursor.completed_groups != cursor.total_groups
            || cursor.scope != self.scope
            || self.journal.checkpoint_anchor() != Some(cursor.anchor)
        {
            return Err(TransactionError::IntegrityFailure);
        }
        Ok(JournalRangeReadReport {
            groups: cursor.completed_groups,
            encoded_bytes: cursor.initial_encoded_bytes - cursor.remaining_encoded_bytes,
        })
    }
}
