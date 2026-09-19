use super::*;
use crate::{
    IndexDelta,
    packed_root_manifest::*,
    packed_tree_batch::{TreeBatchLimits, stage_batch},
    packed_tree_validation::{TreeValidationLimits, validate_tree},
};

#[test]
fn packed_root_manifest_model_restart_preserves_old_and_copy_on_write_trees() {
    let mut f = fixture(2 * MAX_CHUNK_DATA + 7);
    let c = context();
    let directory = f.fs.root();
    let write = PackedPageContext {
        scope: c.scope,
        profile: c.profile,
        family: c.family,
        creation_revision: c.revision.checked_next().unwrap(),
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([6; 16]),
        object: [0; 16],
        page: 0,
    };
    let next = stage_batch(
        &mut f.fs,
        &directory,
        &mut f.vault,
        &mut Entropy(90_000),
        c,
        f.root.claimed,
        Some(f.root.location),
        &[IndexDelta::new(
            b"a".to_vec(),
            Some(b"a".to_vec()),
            Some(b"changed".to_vec()),
        )
        .unwrap()],
        write,
        TreeBatchLimits {
            maximum_deltas: 1,
            maximum_input_bytes: 64,
            maximum_dirty_nodes: 128,
            maximum_path_branches: 128,
            maximum_read_pages: 128,
            maximum_read_bytes: 128 * ENCODED_PAGE_BYTES as u64,
            pack: PackWriteLimits {
                maximum_pages: 128,
                maximum_records: 128,
                maximum_payload_bytes: 1024 * 1024,
            },
        },
    )
    .unwrap();
    assert!(next.report().written_nodes < 13);
    let mut expected = f.values.clone();
    expected.insert(b"a".to_vec(), b"changed".to_vec());
    let manifests = [
        (c, f.root, f.values.clone()),
        (next.context(), next.root().unwrap(), expected),
    ];
    for (index, (view, root, _)) in manifests.iter().enumerate() {
        let context = PackedRootContext {
            scope: c.scope,
            profile: c.profile,
            epoch: KeyEpoch::FIRST,
            writer: WriterIncarnationId::from_bytes([7; 16]),
            object: [80 + index as u8; 16],
        };
        let claims = PackedRootClaims {
            revision: view.revision,
            generation: index as u64 + 1,
            certificate_digest: [20 + index as u8; 32],
            reducer_profile: [30; 32],
            state_commitment_profile: [31; 32],
            state_digest: [32 + index as u8; 32],
        };
        let families = [
            PackedRootFamily {
                family: c.family,
                commitment: root.claimed,
                root: Some(root.location),
            },
            PackedRootFamily {
                family: 5,
                commitment: logical::empty_commitment(
                    CommitmentContext::new(c.scope, c.profile, 5).unwrap(),
                ),
                root: None,
            },
        ];
        let encoded = seal_manifest(&mut f.vault, context, claims, &families).unwrap();
        // Test-only fixed names/claims. This is not the future journal-admitted publication API.
        let file =
            f.fs.create_new(
                &directory,
                &EntryName::new(format!("synthetic-manifest-{index}")).unwrap(),
            )
            .unwrap();
        write_all_at(&mut f.fs, &file, 0, &encoded).unwrap();
        f.fs.set_len(&file, encoded.len() as u64).unwrap();
        f.fs.sync_all(&file).unwrap();
    }
    f.fs.sync_directory(&directory).unwrap();
    f.fs.restart().unwrap();
    let directory = f.fs.root();
    for (index, (view, root, values)) in manifests.iter().enumerate() {
        let file =
            f.fs.open_existing(
                &directory,
                &EntryName::new(format!("synthetic-manifest-{index}")).unwrap(),
            )
            .unwrap();
        assert_eq!(
            f.fs.metadata(&file).unwrap().len,
            ENCODED_MANIFEST_BYTES as u64
        );
        let mut encoded = vec![0; ENCODED_MANIFEST_BYTES];
        read_exact_at(&mut f.fs, &file, 0, &mut encoded).unwrap();
        let context = PackedRootContext {
            scope: c.scope,
            profile: c.profile,
            epoch: KeyEpoch::FIRST,
            writer: WriterIncarnationId::from_bytes([7; 16]),
            object: [80 + index as u8; 16],
        };
        let opened = open_manifest(&f.vault, context, &encoded).unwrap();
        assert_eq!(opened.claims().revision, view.revision);
        let family = opened.families()[0];
        assert_eq!(family.commitment, root.claimed);
        assert!(family.root == Some(root.location));
        let admitted = validate_tree(
            &mut f.fs,
            &directory,
            &f.vault,
            *view,
            family.commitment,
            family.root,
            TreeValidationLimits {
                maximum_path_branches: 128,
                maximum_nodes: 128,
                maximum_logical_bytes: 1024 * 1024,
                maximum_pages: 128,
                maximum_encoded_bytes: 128 * ENCODED_PAGE_BYTES as u64,
            },
        )
        .unwrap();
        assert_eq!(admitted.report().entries, values.len() as u64);
        for (key, value) in values {
            assert_eq!(
                lookup(
                    &mut f.fs,
                    &directory,
                    &f.vault,
                    *view,
                    family.commitment,
                    family.root,
                    key,
                    limits()
                )
                .unwrap()
                .value
                .unwrap()
                .as_slice(),
                value
            );
        }
    }
}
