//! Private batch-sized sorting/aggregation over an admitted primary owner cursor.
use super::*;
use uste_storage::{
    journal::CertificateAnchorReadReport,
    packed_tree_cursor::{TreeCursorLimits, TreeCursorReport},
};

#[derive(Clone, Copy)]
pub struct PackedQuotaRebuildLimits {
    pub certificates: CertificateAnchorReadLimits,
    pub cursor: TreeCursorLimits,
    pub lookup: TreeLookupLimits,
    pub batch: TreeBatchLimits,
    pub maximum_owners: u64,
    pub maximum_batch_owners: usize,
    pub maximum_batches: u64,
    pub maximum_lookup_pages: u64,
    pub maximum_lookup_bytes: u64,
}
pub struct PackedQuotaRebuildReport {
    pub certificate: CertificateAnchorReadReport,
    pub cursor: TreeCursorReport,
    pub batches: u64,
    pub maximum_batch_owners: usize,
    pub lookup_pages: u64,
    pub lookup_bytes: u64,
    pub batch_read_pages: u64,
    pub batch_read_bytes: u64,
    pub written_pages: u64,
    pub written_nodes: u64,
    pub maximum_admitted_metadata_bytes: u64,
}
struct PartialQuota {
    trees: [CanonicalPackedTree; 3],
    owners: u64,
    bytes: u64,
    principals: u64,
}

pub fn rebuild_packed_quota_prefix<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    primary: &PackedCoordinatorPrefix,
    limits: PackedQuotaRebuildLimits,
) -> Result<(PackedQuotaPrefix, PackedQuotaRebuildReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if primary.scope != recovery.scope() {
        return Err(TransactionError::IntegrityFailure);
    }
    if limits.maximum_batch_owners == 0
        || limits.maximum_batch_owners > MAX_BATCH_DELTAS
        || limits.maximum_batch_owners > limits.batch.maximum_deltas
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || primary.owner_count() > limits.maximum_owners
    {
        return Err(TransactionError::ResourceLimit);
    }
    let expected_batches = primary
        .owner_count()
        .max(1)
        .div_ceil(limits.maximum_batch_owners as u64);
    if expected_batches > limits.maximum_batches {
        return Err(TransactionError::ResourceLimit);
    }
    for tree in &primary.trees {
        recovery
            .journal
            .validate_packed_tree_binding(tree)
            .map_err(TransactionError::Storage)?;
    }
    let proof = recovery
        .journal
        .authenticate_certificate_anchor(
            fs,
            primary.anchor.0,
            primary.anchor.1,
            limits.certificates,
        )
        .map_err(TransactionError::Storage)?;
    let mut report = PackedQuotaRebuildReport {
        certificate: proof.report(),
        cursor: TreeCursorReport::default(),
        batches: 0,
        maximum_batch_owners: 0,
        lookup_pages: 0,
        lookup_bytes: 0,
        batch_read_pages: 0,
        batch_read_bytes: 0,
        written_pages: 0,
        written_nodes: 0,
        maximum_admitted_metadata_bytes: 0,
    };
    let mut maintenance = PackedIndexMaintenance::new(primary.scope, &mut recovery.journal, proof)?;
    let mut cursor = maintenance.cursor(&primary.trees[2], b"", None, limits.cursor)?;
    let mut partial: Option<PartialQuota> = None;
    let mut exhausted = false;
    loop {
        let mut owners = Vec::new();
        owners
            .try_reserve_exact(limits.maximum_batch_owners)
            .map_err(|_| TransactionError::ResourceLimit)?;
        let mut charges = Vec::new();
        charges
            .try_reserve_exact(limits.maximum_batch_owners)
            .map_err(|_| TransactionError::ResourceLimit)?;
        let mut charged_bytes = 0;
        for _ in 0..limits.maximum_batch_owners {
            let Some(entry) = maintenance.next(fs, &mut cursor)? else {
                exhausted = true;
                break;
            };
            let (reference, principal) = decode_owner(primary.scope, entry.key(), entry.value())
                .map_err(TransactionError::Storage)?;
            charged_bytes = add(charged_bytes, reference.byte_len())?;
            charges.push((principal, reference.byte_len()));
            let mut key = principal.as_bytes().to_vec();
            key.extend_from_slice(entry.key());
            owners.push(
                IndexDelta::new(key, None, Some(entry.value().to_vec()))
                    .map_err(TransactionError::Storage)?,
            );
        }
        if owners.is_empty() && partial.is_some() {
            break;
        }
        if report.batches == limits.maximum_batches {
            return Err(TransactionError::ResourceLimit);
        }
        owners.sort_by(|a, b| a.key().cmp(b.key()));
        charges.sort_by_key(|(principal, _)| *principal);
        let mut principals = partial.as_ref().map_or(0, |p| p.principals);
        let mut principal_deltas = Vec::new();
        principal_deltas
            .try_reserve_exact(charges.len())
            .map_err(|_| TransactionError::ResourceLimit)?;
        let mut index = 0;
        while index < charges.len() {
            let principal = charges[index].0;
            let mut count = 0;
            let mut bytes = 0;
            while index < charges.len() && charges[index].0 == principal {
                count = add(count, 1)?;
                bytes = add(bytes, charges[index].1)?;
                index += 1;
            }
            let previous = if let Some(before) = &partial {
                let mut lookup = limits.lookup;
                lookup.maximum_pages = lookup.maximum_pages.min(
                    limits
                        .maximum_lookup_pages
                        .checked_sub(report.lookup_pages)
                        .ok_or(TransactionError::ResourceLimit)?,
                );
                lookup.maximum_encoded_bytes = lookup.maximum_encoded_bytes.min(
                    limits
                        .maximum_lookup_bytes
                        .checked_sub(report.lookup_bytes)
                        .ok_or(TransactionError::ResourceLimit)?,
                );
                let result =
                    maintenance.get(fs, &before.trees[1], &principal.as_bytes(), lookup)?;
                report.lookup_pages = add(report.lookup_pages, result.report.pages)?;
                report.lookup_bytes = add(report.lookup_bytes, result.report.encoded_bytes)?;
                if report.lookup_pages > limits.maximum_lookup_pages
                    || report.lookup_bytes > limits.maximum_lookup_bytes
                {
                    return Err(TransactionError::ResourceLimit);
                }
                result.value.map(|v| v.as_slice().to_vec())
            } else {
                None
            };
            let (old_count, old_bytes) = previous
                .as_deref()
                .map(decode_pair)
                .transpose()?
                .unwrap_or((0, 0));
            if previous.is_none() {
                principals = add(principals, 1)?;
            }
            principal_deltas.push(
                IndexDelta::new(
                    principal.as_bytes().to_vec(),
                    previous,
                    Some(pair(add(old_count, count)?, add(old_bytes, bytes)?)),
                )
                .map_err(TransactionError::Storage)?,
            );
        }
        let owner_count = add(
            partial.as_ref().map_or(0, |p| p.owners),
            owners.len() as u64,
        )?;
        let bytes = add(partial.as_ref().map_or(0, |p| p.bytes), charged_bytes)?;
        if owner_count > primary.owner_count() {
            return Err(TransactionError::IntegrityFailure);
        }
        let metadata = [IndexDelta::new(
            KEY.to_vec(),
            partial
                .as_ref()
                .map(|p| head(p.owners, p.bytes, p.principals)),
            Some(head(owner_count, bytes, principals)),
        )
        .map_err(TransactionError::Storage)?];
        let mut trees = Vec::new();
        trees
            .try_reserve_exact(3)
            .map_err(|_| TransactionError::ResourceLimit)?;
        for (index, deltas) in [
            metadata.as_slice(),
            principal_deltas.as_slice(),
            owners.as_slice(),
        ]
        .into_iter()
        .enumerate()
        {
            let staged = maintenance.stage(
                fs,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                index as u8 + 1,
                partial.as_ref().map(|p| &p.trees[index]),
                deltas,
                limits.batch,
            )?;
            let work = staged.report();
            report.batch_read_pages = add(report.batch_read_pages, work.read_pages)?;
            report.batch_read_bytes = add(report.batch_read_bytes, work.read_bytes)?;
            report.written_pages = add(report.written_pages, work.written_pages)?;
            report.written_nodes = add(report.written_nodes, work.written_nodes)?;
            report.maximum_admitted_metadata_bytes = report
                .maximum_admitted_metadata_bytes
                .max(work.metadata_admitted_bytes);
            trees.push(staged.tree().clone());
        }
        let trees: [CanonicalPackedTree; 3] = trees
            .try_into()
            .map_err(|_| TransactionError::IntegrityFailure)?;
        if trees[0].family_descriptor().commitment.entries() != 1
            || trees[1].family_descriptor().commitment.entries() != principals
            || trees[2].family_descriptor().commitment.entries() != owner_count
        {
            return Err(TransactionError::IntegrityFailure);
        }
        partial = Some(PartialQuota {
            trees,
            owners: owner_count,
            bytes,
            principals,
        });
        report.batches = add(report.batches, 1)?;
        report.maximum_batch_owners = report.maximum_batch_owners.max(owners.len());
        if exhausted {
            break;
        }
    }
    let partial = partial.ok_or(TransactionError::IntegrityFailure)?;
    report.cursor = cursor.report();
    if !exhausted
        || report.cursor.returned_entries != primary.owner_count()
        || partial.owners != primary.owner_count()
        || report.batches != expected_batches
    {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok((
        PackedQuotaPrefix {
            scope: primary.scope,
            anchor: primary.anchor,
            primary: primary.families().map(|f| f.commitment),
            trees: partial.trees,
            owners: partial.owners,
            bytes: partial.bytes,
            principals: partial.principals,
        },
        report,
    ))
}
