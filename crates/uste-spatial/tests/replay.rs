mod common;

use common::{child_frame, geometry, observation, point, reference, revision, world_and_root};
use uste_replay::{
    ReplayError, ReplayEvent, capture_reducer_checkpoint, cold_replay, verify_reducer_checkpoint,
};
use uste_spatial::{
    SpatialError, SpatialRecord, SpatialState, SpatialTransaction, decode_transaction,
    encode_transaction,
};
use uste_txn::{ApplyError, CheckpointState, TransactionState};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn replay_event<'a>(
    bytes: &'a [u8],
    revision_number: u64,
    state: &SpatialState,
) -> ReplayEvent<'a> {
    let revision = revision(revision_number);
    let prepared = TransactionState::prepare(state, bytes, None, revision).unwrap();
    ReplayEvent {
        revision,
        canonical_request: bytes,
        blob_inventory: None,
        expected_result_digest: SpatialState::result_digest(&prepared),
    }
}

#[test]
fn durable_spatial_replay_and_checkpoint_rebuild_the_validated_catalog() {
    let first = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let first_bytes = encode_transaction(&first).unwrap();
    assert_eq!(decode_transaction(&first_bytes).unwrap(), first);
    let genesis = SpatialState::new(scope());
    let first_event = replay_event(&first_bytes, 1, &genesis);
    let (state, report) = cold_replay(genesis, [first_event]).unwrap();
    assert_eq!(report.frontier, Some(revision(1)));

    let child = SpatialTransaction::new(
        scope(),
        vec![SpatialRecord::Frame(child_frame(12, 1, reference(11, 1)))],
    )
    .unwrap();
    let child_bytes = encode_transaction(&child).unwrap();
    let child_event = replay_event(&child_bytes, 2, &state);
    let (state, _) = cold_replay(state, [child_event]).unwrap();
    assert!(
        state
            .snapshot()
            .catalog()
            .frame_at(reference(12, 1), revision(2))
            .is_ok()
    );

    let checkpoint = capture_reducer_checkpoint(&state).unwrap();
    let restored = verify_reducer_checkpoint::<SpatialState>(scope(), &checkpoint).unwrap();
    assert!(
        restored
            .snapshot()
            .catalog()
            .frame_at(reference(12, 1), revision(2))
            .is_ok()
    );
    assert_eq!(
        SpatialState::logical_state_digest(&state.snapshot()).unwrap(),
        SpatialState::logical_state_digest(&restored.snapshot()).unwrap()
    );
}

#[test]
fn durable_prepare_and_replay_reject_frame_cycles_before_publication() {
    let first = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let first_bytes = encode_transaction(&first).unwrap();
    let genesis = SpatialState::new(scope());
    let first_event = replay_event(&first_bytes, 1, &genesis);
    let (state, _) = cold_replay(genesis, [first_event]).unwrap();
    let before = state.snapshot();

    let cycle = SpatialTransaction::new(
        scope(),
        vec![
            SpatialRecord::Frame(child_frame(12, 1, reference(13, 1))),
            SpatialRecord::Frame(child_frame(13, 1, reference(12, 1))),
        ],
    )
    .unwrap();
    let cycle_bytes = encode_transaction(&cycle).unwrap();
    assert!(matches!(
        TransactionState::prepare(&state, &cycle_bytes, None, revision(2)),
        Err(ApplyError::InvalidRequest)
    ));
    assert_eq!(state.snapshot().revision(), before.revision());

    let event = ReplayEvent {
        revision: revision(2),
        canonical_request: &cycle_bytes,
        blob_inventory: None,
        expected_result_digest: [0; 32],
    };
    assert!(matches!(
        cold_replay(state, [event]),
        Err(ReplayError::ReducerRejected)
    ));
}

#[test]
fn transaction_codec_rejects_every_truncation_and_scope_mismatch() {
    let transaction = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let bytes = encode_transaction(&transaction).unwrap();
    for end in 0..bytes.len() {
        assert!(
            decode_transaction(&bytes[..end]).is_err(),
            "accepted cut {end}"
        );
    }
    let foreign = NamespaceRef::new(
        DatabaseId::from_bytes([9; 16]),
        NamespaceId::from_bytes([2; 16]),
    );
    assert_eq!(
        SpatialTransaction::new(foreign, world_and_root().to_vec()),
        Err(SpatialError::ScopeMismatch)
    );
}

#[test]
fn genesis_has_a_logical_digest_without_pretending_it_is_checkpointable() {
    let state = SpatialState::new(scope());
    assert!(SpatialState::logical_state_digest(&state.snapshot()).is_ok());
    assert!(matches!(
        SpatialState::encode_checkpoint(&state.snapshot()),
        Err(uste_txn::CheckpointStateError::Invalid)
    ));
}

#[test]
fn result_digest_binds_effects_without_scanning_unrelated_state() {
    let plain_first = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let mut extended_records = world_and_root().to_vec();
    extended_records.push(SpatialRecord::Geometry(geometry(20, reference(11, 1))));
    let extended_first = SpatialTransaction::new(scope(), extended_records).unwrap();

    let mut plain = SpatialState::new(scope());
    let prepared = plain
        .prepare(
            &encode_transaction(&plain_first).unwrap(),
            None,
            revision(1),
        )
        .unwrap();
    plain.publish(prepared);
    let mut extended = SpatialState::new(scope());
    let prepared = extended
        .prepare(
            &encode_transaction(&extended_first).unwrap(),
            None,
            revision(1),
        )
        .unwrap();
    extended.publish(prepared);

    let same_request = SpatialTransaction::new(
        scope(),
        vec![SpatialRecord::Frame(child_frame(12, 1, reference(11, 1)))],
    )
    .unwrap();
    let same_bytes = encode_transaction(&same_request).unwrap();
    let plain_prepared = plain.prepare(&same_bytes, None, revision(2)).unwrap();
    let extended_prepared = extended.prepare(&same_bytes, None, revision(2)).unwrap();
    assert_eq!(
        SpatialState::result_digest(&plain_prepared),
        SpatialState::result_digest(&extended_prepared)
    );
}

#[test]
fn result_digest_distinguishes_observation_insert_from_retry() {
    let first = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let first_bytes = encode_transaction(&first).unwrap();
    let mut inserting = SpatialState::new(scope());
    let prepared = inserting.prepare(&first_bytes, None, revision(1)).unwrap();
    inserting.publish(prepared);

    let observed = SpatialRecord::Observation(Box::new(observation(50, 1, point(3, 4))));
    let existing = SpatialTransaction::new(scope(), vec![observed.clone()]).unwrap();
    let existing_bytes = encode_transaction(&existing).unwrap();
    let mut retrying = inserting.clone();
    let prepared = retrying
        .prepare(&existing_bytes, None, revision(2))
        .unwrap();
    retrying.publish(prepared);

    let advance = SpatialTransaction::new(
        scope(),
        vec![SpatialRecord::Frame(child_frame(12, 1, reference(11, 1)))],
    )
    .unwrap();
    let prepared = inserting
        .prepare(&encode_transaction(&advance).unwrap(), None, revision(2))
        .unwrap();
    inserting.publish(prepared);

    let inserting_effect = inserting
        .prepare(&existing_bytes, None, revision(3))
        .unwrap();
    let retry_effect = retrying
        .prepare(&existing_bytes, None, revision(3))
        .unwrap();
    assert_ne!(
        SpatialState::result_digest(&inserting_effect),
        SpatialState::result_digest(&retry_effect)
    );
}

#[test]
fn checkpoint_decoder_rejects_noncanonical_record_order() {
    let transaction = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let mut state = SpatialState::new(scope());
    let prepared = state
        .prepare(
            &encode_transaction(&transaction).unwrap(),
            None,
            revision(1),
        )
        .unwrap();
    state.publish(prepared);
    let payload = SpatialState::encode_checkpoint(&state.snapshot()).unwrap();

    const HEADER: usize = 56;
    let first_length = u32::from_be_bytes(payload[HEADER + 8..HEADER + 12].try_into().unwrap());
    let second_offset = HEADER + 12 + first_length as usize;
    let first = &payload[HEADER..second_offset];
    let second = &payload[second_offset..];
    let mut reordered = payload[..HEADER].to_vec();
    reordered.extend_from_slice(second);
    reordered.extend_from_slice(first);

    assert!(matches!(
        SpatialState::decode_checkpoint(scope(), revision(1), &reordered),
        Err(uste_txn::CheckpointStateError::Invalid)
    ));
}

#[test]
fn checkpoint_decoder_rejects_truncation_counts_duplicates_and_future_revisions() {
    let transaction = SpatialTransaction::new(scope(), world_and_root().to_vec()).unwrap();
    let mut state = SpatialState::new(scope());
    let prepared = state
        .prepare(
            &encode_transaction(&transaction).unwrap(),
            None,
            revision(1),
        )
        .unwrap();
    state.publish(prepared);
    let payload = SpatialState::encode_checkpoint(&state.snapshot()).unwrap();

    for end in 0..payload.len() {
        assert!(
            SpatialState::decode_checkpoint(scope(), revision(1), &payload[..end]).is_err(),
            "checkpoint accepted cut {end}"
        );
    }
    let mut trailing = payload.clone();
    trailing.push(0);
    assert!(SpatialState::decode_checkpoint(scope(), revision(1), &trailing).is_err());

    let mut zero_count = payload.clone();
    zero_count[48..56].copy_from_slice(&0_u64.to_be_bytes());
    assert!(SpatialState::decode_checkpoint(scope(), revision(1), &zero_count).is_err());

    let mut oversized_count = payload.clone();
    oversized_count[48..56].copy_from_slice(&1_000_001_u64.to_be_bytes());
    assert!(SpatialState::decode_checkpoint(scope(), revision(1), &oversized_count).is_err());

    let foreign_scope = NamespaceRef::new(
        DatabaseId::from_bytes([9; 16]),
        NamespaceId::from_bytes([2; 16]),
    );
    assert!(SpatialState::decode_checkpoint(foreign_scope, revision(1), &payload).is_err());

    let mut future_revision = payload.clone();
    future_revision[56..64].copy_from_slice(&2_u64.to_be_bytes());
    assert!(SpatialState::decode_checkpoint(scope(), revision(1), &future_revision).is_err());

    const HEADER: usize = 56;
    let first_length = u32::from_be_bytes(payload[HEADER + 8..HEADER + 12].try_into().unwrap());
    let first_end = HEADER + 12 + first_length as usize;
    let mut duplicate = payload[..first_end].to_vec();
    duplicate.extend_from_slice(&payload[HEADER..first_end]);
    duplicate.extend_from_slice(&payload[first_end..]);
    duplicate[48..56].copy_from_slice(&3_u64.to_be_bytes());
    assert!(SpatialState::decode_checkpoint(scope(), revision(1), &duplicate).is_err());
}
