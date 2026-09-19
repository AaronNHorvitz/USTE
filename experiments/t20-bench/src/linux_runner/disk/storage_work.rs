//! Privileged cold-open diagnostics. These are separate from graph suffix/index query work.
use super::*;
use uste_storage::journal::{BlobMetadataAdmissionReport, BlobRecoveryScanReport};

pub(super) fn storage_resident(disk: &Disk) -> bool {
    disk.certificate_anchor_residency().0 || disk.blob_metadata_residency().0
}

pub(super) fn storage_recovery_json(disk: &Disk) -> Result<serde_json::Value, LinuxRunnerError> {
    let report = disk
        .blob_recovery_report()
        .ok_or_else(|| error("USTE_BM01_STORAGE_MODE"))?;
    let (_, references, inventories, namespaces) = disk.blob_metadata_residency();
    Ok(serde_json::json!({
        "mode": "disk-blob-metadata-v1", "complete_filesystem_io_accounting": false,
        "measurement_scope": "last-storage-owner-cold-open-only",
        "resident_certificate_entries": disk.certificate_anchor_residency().1,
        "resident_blob_references": references, "resident_inventory_ids": inventories,
        "resident_namespace_totals": namespaces,
        "validation": scan(&report.validation), "replay": scan(&report.replay),
        "used_existing_catalog": report.used_existing_catalog,
        "discovery_certificate_bytes": report.discovery_certificate_bytes,
        "catalog_admission": report.admission.as_ref().map(admission),
        "catalog_rebuild": report.rebuild.as_ref().map(|work| serde_json::json!({
            "journal_groups": work.journal.groups, "journal_encoded_bytes": work.journal.encoded_bytes,
            "certificate_bytes": work.certificate_bytes, "merge_output_logical_bytes": work.merge_output_bytes,
            "merge_base_pages": work.merge_pages_read, "lookups": admission(&work.lookups),
            "admission": admission(&work.admission),
        })),
    }))
}

fn scan(report: &BlobRecoveryScanReport) -> serde_json::Value {
    serde_json::json!({
        "groups": report.groups, "encoded_group_certificate_bytes": report.encoded_group_certificate_bytes,
        "reference_bindings": report.reference_bindings, "verified_logical_blob_bytes": report.verified_blob_bytes,
        "maximum_live_inventory_references": report.maximum_live_inventory_references,
        "peak_pending_segment_tails": report.peak_pending_segment_tails,
    })
}

fn admission(report: &BlobMetadataAdmissionReport) -> serde_json::Value {
    serde_json::json!({
        "journal_groups": report.journal.groups, "journal_encoded_bytes": report.journal.encoded_bytes,
        "run_pages": report.run_pages, "run_entries": report.run_entries,
        "lookup_pages": report.lookup_pages, "lookup_fragments": report.lookup_fragments,
        "lookup_cache_hits": report.lookup_cache_hits,
        "lookup_result_bytes": report.lookup_result_bytes, "certificate_bytes": report.certificate_bytes,
    })
}
