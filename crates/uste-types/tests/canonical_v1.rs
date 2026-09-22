use core::str::FromStr;

use uste_types::{
    BorrowedMapValue, BoundedString, DatabaseId, DecodeError, IdempotencyKey,
    MAX_COLLECTION_ENTRIES, MAX_EPOCH_SECONDS, MAX_INLINE_BYTES, MAX_NESTING_DEPTH,
    MAX_VALUE_NODES, MAX_VALUE_PAYLOAD, MIN_EPOCH_SECONDS, NamespaceId, RecordId, RecordRef,
    SourceEventId, TransactionId, UtcInstant, Value, decode_borrowed_map_value,
    decode_borrowed_map_value_with_fields, decode_map_value, decode_value, encode_value,
    encoded_len,
};

const VECTORS: &str = include_str!("../../../acceptance/r1/canonical-v1.tsv");

#[test]
fn literal_golden_vectors_are_exact_and_canonical() {
    for (line_number, line) in VECTORS.lines().enumerate().skip(1) {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(columns.len(), 3, "line {}", line_number + 1);
        let value = logical_value(columns[1]);
        let expected = decode_hex(columns[2]);
        let encoded = encode_value(&value).expect("golden value encodes");
        assert_eq!(encoded, expected, "case {}", columns[0]);
        assert_eq!(encoded_len(&value), Ok(expected.len()));
        let decoded = decode_value(&expected).expect("golden bytes decode");
        assert_eq!(decoded, value, "case {}", columns[0]);
        assert_eq!(encode_value(&decoded).expect("re-encode"), expected);
    }
}

#[test]
fn borrowed_root_map_keys_preserve_canonical_validation_and_owned_values() {
    let value = Value::map(vec![
        (
            BoundedString::new("alpha".into()).unwrap(),
            Value::Unsigned(7),
        ),
        (
            BoundedString::new("omega".into()).unwrap(),
            Value::string("owned".into()).unwrap(),
        ),
    ])
    .unwrap();
    let encoded = encode_value(&value).unwrap();
    let entries = decode_map_value(&encoded).unwrap().unwrap();
    assert_eq!(entries[0], ("alpha", Value::Unsigned(7)));
    assert_eq!(entries[1].0, "omega");
    assert!(matches!(entries[1].1, Value::String(_)));
    let bounds = encoded.as_ptr_range();
    assert!(entries.iter().all(|(key, _)| {
        let pointer = key.as_ptr();
        pointer >= bounds.start && pointer < bounds.end
    }));
    assert_eq!(
        decode_map_value(&encode_value(&Value::Null).unwrap()),
        Ok(None)
    );
    for cut in 0..encoded.len() {
        assert!(decode_map_value(&encoded[..cut]).is_err());
    }
}

#[test]
fn borrowed_root_map_direct_strings_borrow_without_changing_nested_values() {
    let value = Value::map(vec![
        (
            BoundedString::new("direct".into()).unwrap(),
            Value::string("borrowed".into()).unwrap(),
        ),
        (
            BoundedString::new("nested".into()).unwrap(),
            Value::list(vec![Value::string("owned".into()).unwrap()]).unwrap(),
        ),
    ])
    .unwrap();
    let encoded = encode_value(&value).unwrap();
    let entries = decode_borrowed_map_value(&encoded).unwrap().unwrap();
    assert_eq!(entries[0], ("direct", BorrowedMapValue::String("borrowed")));
    assert!(matches!(
        entries[1].1,
        BorrowedMapValue::Value(Value::List(_))
    ));
    let bounds = encoded.as_ptr_range();
    let BorrowedMapValue::String(text) = entries[0].1 else {
        unreachable!();
    };
    assert!(text.as_ptr() >= bounds.start && text.as_ptr() < bounds.end);
    assert_eq!(
        decode_borrowed_map_value(&encode_value(&Value::Null).unwrap()),
        Ok(None)
    );
    for cut in 0..encoded.len() {
        assert!(decode_borrowed_map_value(&encoded[..cut]).is_err());
    }
}

#[test]
fn selected_root_map_fields_borrow_recursively_without_converting_other_maps() {
    let nested = || {
        Value::map(vec![
            (
                BoundedString::new("kind".into()).unwrap(),
                Value::string("borrowed".into()).unwrap(),
            ),
            (
                BoundedString::new("leaf".into()).unwrap(),
                Value::map(vec![(
                    BoundedString::new("kind".into()).unwrap(),
                    Value::string("nested".into()).unwrap(),
                )])
                .unwrap(),
            ),
        ])
        .unwrap()
    };
    let encoded = encode_value(
        &Value::map(vec![
            (BoundedString::new("ordinary".into()).unwrap(), nested()),
            (BoundedString::new("selected".into()).unwrap(), nested()),
        ])
        .unwrap(),
    )
    .unwrap();
    let entries = decode_borrowed_map_value_with_fields(&encoded, &["selected"])
        .unwrap()
        .unwrap();
    assert!(matches!(
        entries[0].1,
        BorrowedMapValue::Value(Value::Map(_))
    ));
    let BorrowedMapValue::Map(selected) = &entries[1].1 else {
        panic!("selected map stayed owned");
    };
    assert_eq!(selected[0].1, BorrowedMapValue::String("borrowed"));
    let BorrowedMapValue::Map(leaf) = &selected[1].1 else {
        panic!("nested selected map stayed owned");
    };
    assert_eq!(leaf[0].1, BorrowedMapValue::String("nested"));
    let bounds = encoded.as_ptr_range();
    for text in [selected[0].0, selected[1].0, leaf[0].0] {
        assert!(text.as_ptr() >= bounds.start && text.as_ptr() < bounds.end);
    }
    for cut in 0..encoded.len() {
        assert!(decode_borrowed_map_value_with_fields(&encoded[..cut], &["selected"]).is_err());
    }
    let nested_duplicate = [
        0x08, 0x01, 0x08, b's', b'e', b'l', b'e', b'c', b't', b'e', b'd', 0x08, 0x02, 0x01, b'a',
        0x00, 0x01, b'a', 0x00,
    ];
    assert_eq!(
        decode_borrowed_map_value_with_fields(&frame(&nested_duplicate), &["selected"]),
        Err(DecodeError::MapKeysOutOfOrder)
    );
}

#[test]
fn every_golden_truncation_and_trailing_byte_is_rejected() {
    for line in VECTORS.lines().skip(1) {
        let bytes = decode_hex(line.split('\t').nth(2).expect("hex column"));
        for cut in 0..bytes.len() {
            assert!(decode_value(&bytes[..cut]).is_err(), "accepted cut {cut}");
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert_eq!(decode_value(&trailing), Err(DecodeError::TrailingBytes));
    }
}

#[test]
fn frame_dispatch_lengths_and_tags_fail_closed() {
    assert_eq!(
        decode_value(b"NOPE\x01\x01\x00\x01\x00"),
        Err(DecodeError::InvalidMagic)
    );

    for (kind, major, minor) in [(2, 1, 0), (1, 2, 0), (1, 1, 1)] {
        let bytes = [b'U', b'S', b'T', b'E', kind, major, minor, 1, 0];
        assert!(matches!(
            decode_value(&bytes),
            Err(DecodeError::UnsupportedVersion { .. })
        ));
    }

    assert_eq!(
        decode_value(b"USTE\x01\x01\x00\x81\x00\x00"),
        Err(DecodeError::NonMinimalInteger)
    );
    let mut oversized = b"USTE\x01\x01\x00".to_vec();
    append_uleb((MAX_VALUE_PAYLOAD + 1) as u128, &mut oversized);
    assert!(matches!(
        decode_value(&oversized),
        Err(DecodeError::PayloadTooLarge { .. })
    ));
    assert_eq!(
        decode_value(&frame(&[0xff])),
        Err(DecodeError::InvalidValueTag(0xff))
    );
}

#[test]
fn scalar_boundaries_and_ambiguities_are_explicit() {
    for value in [
        Value::Unsigned(u128::MAX),
        Value::Signed(i128::MIN),
        Value::Signed(i128::MAX),
        Value::Unsigned(1),
        Value::Signed(1),
    ] {
        let bytes = encode_value(&value).expect("endpoint encodes");
        assert_eq!(decode_value(&bytes), Ok(value));
    }
    assert_ne!(
        encode_value(&Value::Unsigned(1)).expect("unsigned"),
        encode_value(&Value::Signed(1)).expect("signed")
    );

    assert_eq!(
        decode_value(&frame(&[0x04, 0x01, 0x00])),
        Err(DecodeError::InvalidSignedInteger)
    );
    assert_eq!(
        decode_value(&frame(&[0x04, 0x02, 0x01])),
        Err(DecodeError::InvalidSignedInteger)
    );
    let mut too_positive = vec![0x04, 0x00];
    append_uleb(1_u128 << 127, &mut too_positive);
    assert_eq!(
        decode_value(&frame(&too_positive)),
        Err(DecodeError::InvalidSignedInteger)
    );
    let mut too_negative = vec![0x04, 0x01];
    append_uleb((1_u128 << 127) + 1, &mut too_negative);
    assert_eq!(
        decode_value(&frame(&too_negative)),
        Err(DecodeError::InvalidSignedInteger)
    );

    let mut bad_nineteenth_group = vec![0x03];
    bad_nineteenth_group.extend_from_slice(&[0x80; 18]);
    bad_nineteenth_group.push(0x04);
    assert_eq!(
        decode_value(&frame(&bad_nineteenth_group)),
        Err(DecodeError::IntegerOverflow)
    );

    let mut twentieth_group = vec![0x03];
    twentieth_group.extend_from_slice(&[0x80; 19]);
    twentieth_group.push(0x00);
    assert_eq!(
        decode_value(&frame(&twentieth_group)),
        Err(DecodeError::IntegerOverflow)
    );
}

#[test]
fn inline_collection_and_depth_limits_reject_before_body_allocation() {
    assert!(Value::bytes(vec![0; MAX_INLINE_BYTES]).is_ok());
    assert!(Value::bytes(vec![0; MAX_INLINE_BYTES + 1]).is_err());

    let mut huge_bytes_claim = vec![0x05];
    append_uleb((MAX_INLINE_BYTES + 1) as u128, &mut huge_bytes_claim);
    assert!(matches!(
        decode_value(&frame(&huge_bytes_claim)),
        Err(DecodeError::InlineValueTooLarge { .. })
    ));

    let mut huge_list_claim = vec![0x07];
    append_uleb((MAX_COLLECTION_ENTRIES + 1) as u128, &mut huge_list_claim);
    assert!(matches!(
        decode_value(&frame(&huge_list_claim)),
        Err(DecodeError::CollectionTooLarge { .. })
    ));

    let mut bounded_but_missing = vec![0x07];
    append_uleb(MAX_COLLECTION_ENTRIES as u128, &mut bounded_but_missing);
    assert_eq!(
        decode_value(&frame(&bounded_but_missing)),
        Err(DecodeError::UnexpectedEof)
    );

    let mut depth_33 = Vec::new();
    for _ in 0..=MAX_NESTING_DEPTH {
        depth_33.extend_from_slice(&[0x07, 0x01]);
    }
    depth_33.push(0x00);
    assert!(matches!(
        decode_value(&frame(&depth_33)),
        Err(DecodeError::NestingTooDeep { .. })
    ));

    let maximum_string = Value::string("x".repeat(MAX_INLINE_BYTES)).expect("inclusive maximum");
    assert_eq!(
        decode_value(&encode_value(&maximum_string).expect("encode maximum string")),
        Ok(maximum_string)
    );

    let maximum_list = Value::list(vec![Value::Null; MAX_COLLECTION_ENTRIES])
        .expect("inclusive collection maximum");
    assert_eq!(
        decode_value(&encode_value(&maximum_list).expect("encode maximum list")),
        Ok(maximum_list)
    );

    let maximum_map_entries = (0..MAX_COLLECTION_ENTRIES)
        .map(|index| {
            (
                BoundedString::new(format!("{index:05}")).expect("bounded key"),
                Value::Null,
            )
        })
        .collect();
    let maximum_map = Value::map(maximum_map_entries).expect("inclusive map maximum");
    assert_eq!(
        decode_value(&encode_value(&maximum_map).expect("encode maximum map")),
        Ok(maximum_map)
    );

    let mut depth_32 = Vec::new();
    for _ in 0..MAX_NESTING_DEPTH {
        depth_32.extend_from_slice(&[0x07, 0x01]);
    }
    depth_32.push(0x00);
    assert!(decode_value(&frame(&depth_32)).is_ok());
}

#[test]
fn map_key_order_utf8_and_normalization_are_not_ambiguous() {
    let duplicate = [0x08, 0x02, 0x01, b'a', 0x00, 0x01, b'a', 0x00];
    assert_eq!(
        decode_value(&frame(&duplicate)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    assert_eq!(
        decode_map_value(&frame(&duplicate)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    assert_eq!(
        decode_borrowed_map_value(&frame(&duplicate)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    let descending = [0x08, 0x02, 0x01, b'b', 0x00, 0x01, b'a', 0x00];
    assert_eq!(
        decode_value(&frame(&descending)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    assert_eq!(
        decode_map_value(&frame(&descending)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    assert_eq!(
        decode_borrowed_map_value(&frame(&descending)),
        Err(DecodeError::MapKeysOutOfOrder)
    );
    assert_eq!(
        decode_value(&frame(&[0x06, 0x01, 0xff])),
        Err(DecodeError::InvalidUtf8)
    );
    assert_eq!(
        decode_map_value(&frame(&[0x08, 0x01, 0x01, 0xff, 0x00])),
        Err(DecodeError::InvalidUtf8)
    );
    assert_eq!(
        decode_borrowed_map_value(&frame(&[0x08, 0x01, 0x01, 0xff, 0x00])),
        Err(DecodeError::InvalidUtf8)
    );

    let composed = Value::string("é".to_owned()).expect("bounded");
    let decomposed = Value::string("e\u{301}".to_owned()).expect("bounded");
    assert_ne!(composed, decomposed);
    assert_ne!(
        encode_value(&composed).expect("encode"),
        encode_value(&decomposed).expect("encode")
    );
}

#[test]
fn instant_and_scoped_reference_endpoints_round_trip() {
    for instant in [
        UtcInstant::new(MIN_EPOCH_SECONDS, 0).expect("minimum"),
        UtcInstant::new(-1, 500_000_000).expect("negative half"),
        UtcInstant::new(0, 0).expect("epoch"),
        UtcInstant::new(MAX_EPOCH_SECONDS, 999_999_999).expect("maximum"),
    ] {
        let value = Value::Instant(instant);
        assert_eq!(
            decode_value(&encode_value(&value).expect("encode")),
            Ok(value)
        );
    }

    let mut invalid_nanos = vec![0x09, 0x00, 0x00];
    append_uleb(1_000_000_000, &mut invalid_nanos);
    assert_eq!(
        decode_value(&frame(&invalid_nanos)),
        Err(DecodeError::InvalidInstant)
    );
    assert_eq!(
        decode_value(&frame(&[0x09, 0x01, 0x00, 0x00])),
        Err(DecodeError::InvalidSignedInteger)
    );
    let mut before_minimum = vec![0x09, 0x01];
    append_uleb(
        u128::from(MIN_EPOCH_SECONDS.unsigned_abs()) + 1,
        &mut before_minimum,
    );
    before_minimum.push(0);
    assert_eq!(
        decode_value(&frame(&before_minimum)),
        Err(DecodeError::InvalidInstant)
    );
    let mut after_maximum = vec![0x09, 0x00];
    append_uleb(
        u128::try_from(MAX_EPOCH_SECONDS).expect("positive") + 1,
        &mut after_maximum,
    );
    after_maximum.push(0);
    assert_eq!(
        decode_value(&frame(&after_maximum)),
        Err(DecodeError::InvalidInstant)
    );

    let reference = RecordRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
        RecordId::from_bytes([3; 16]),
    );
    let value = Value::RecordRef(reference);
    let bytes = encode_value(&value).expect("reference encodes");
    assert_eq!(decode_value(&bytes), Ok(value));

    let payload_start = 8;
    for (offset, replacement) in [
        (1, NamespaceId::TAG),
        (18, RecordId::TAG),
        (35, DatabaseId::TAG),
    ] {
        let mut wrong_tag = bytes.clone();
        wrong_tag[payload_start + offset] = replacement;
        assert!(matches!(
            decode_value(&wrong_tag),
            Err(DecodeError::InvalidIdentityTag { .. })
        ));
    }
}

#[test]
fn identity_text_rejects_case_length_prefix_and_whitespace_variants() {
    assert_eq!(
        [
            DatabaseId::TAG,
            NamespaceId::TAG,
            RecordId::TAG,
            TransactionId::TAG,
            SourceEventId::TAG,
            IdempotencyKey::TAG,
        ],
        [1, 2, 3, 4, 5, 6]
    );
    let zeroes = "00000000000000000000000000000000";
    assert_eq!(
        DatabaseId::from_str(&format!("db_{zeroes}")).map(|id| id.to_string()),
        Ok(format!("db_{zeroes}"))
    );
    assert_eq!(
        NamespaceId::from_str(&format!("ns_{zeroes}")).map(|id| id.to_string()),
        Ok(format!("ns_{zeroes}"))
    );
    assert_eq!(
        RecordId::from_str(&format!("rec_{zeroes}")).map(|id| id.to_string()),
        Ok(format!("rec_{zeroes}"))
    );
    assert_eq!(
        TransactionId::from_str(&format!("txn_{zeroes}")).map(|id| id.to_string()),
        Ok(format!("txn_{zeroes}"))
    );
    assert_eq!(
        SourceEventId::from_str(&format!("src_{zeroes}")).map(|id| id.to_string()),
        Ok(format!("src_{zeroes}"))
    );
    let good = "idem_00000000000000000000000000000000";
    assert_eq!(
        IdempotencyKey::from_str(good),
        Ok(IdempotencyKey::from_bytes([0; 16]))
    );
    for bad in [
        "idem_0000000000000000000000000000000",
        "idem_000000000000000000000000000000000",
        "idem_0000000000000000000000000000000A",
        "idem-00000000000000000000000000000000",
        " idem_00000000000000000000000000000000",
        "db_00000000000000000000000000000000",
    ] {
        assert!(IdempotencyKey::from_str(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn aggregate_payload_cap_is_enforced_before_output_allocation() {
    let chunk = Value::bytes(vec![0; MAX_INLINE_BYTES]).expect("bounded chunk");
    let value = Value::list(vec![chunk; 17]).expect("entry count and depth are bounded");
    assert!(encode_value(&value).is_err());
}

#[test]
fn aggregate_node_budget_blocks_compact_heap_amplification() {
    let mut amplified = vec![0x07];
    append_uleb(5, &mut amplified);
    for _ in 0..5 {
        amplified.push(0x07);
        append_uleb(MAX_COLLECTION_ENTRIES as u128, &mut amplified);
        amplified.extend(std::iter::repeat_n(0x00, MAX_COLLECTION_ENTRIES));
    }
    assert!(amplified.len() < MAX_VALUE_PAYLOAD);
    assert!(matches!(
        decode_value(&frame(&amplified)),
        Err(DecodeError::TooManyValueNodes {
            maximum: MAX_VALUE_NODES,
            ..
        })
    ));
}

#[test]
fn aggregate_node_budget_is_inclusive_for_construction_and_encoding() {
    let full_child =
        || Value::list(vec![Value::Null; MAX_COLLECTION_ENTRIES]).expect("bounded child");
    let final_child = Value::list(vec![Value::Null; 65_531]).expect("bounded final child");
    let maximum = Value::list(vec![full_child(), full_child(), full_child(), final_child])
        .expect("exact aggregate node maximum");
    let encoded = encode_value(&maximum).expect("exact node maximum encodes");
    assert_eq!(decode_value(&encoded), Ok(maximum));

    let overflowing_final = Value::list(vec![Value::Null; 65_532]).expect("bounded child");
    assert!(matches!(
        Value::list(vec![
            full_child(),
            full_child(),
            full_child(),
            overflowing_final,
        ]),
        Err(uste_types::ValidationError::TooManyValueNodes {
            maximum: MAX_VALUE_NODES,
            ..
        })
    ));
}

#[test]
fn deterministic_mutation_fuzz_preserves_strict_canonicality() {
    let corpus: Vec<Vec<u8>> = VECTORS
        .lines()
        .skip(1)
        .map(|line| decode_hex(line.split('\t').nth(2).expect("hex")))
        .collect();

    for seed in &corpus {
        for index in 0..seed.len() {
            for mask in [0x01, 0x80, 0xff] {
                let mut mutated = seed.clone();
                mutated[index] ^= mask;
                assert_canonical_if_accepted(&mutated);
            }
        }
    }

    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for case in 0..50_000_usize {
        state = xorshift(state);
        let length = (state as usize) & 0xff;
        let mut bytes = vec![0_u8; length];
        for byte in &mut bytes {
            state = xorshift(state);
            *byte = state as u8;
        }
        if case % 3 == 0 && bytes.len() >= 7 {
            bytes[..7].copy_from_slice(b"USTE\x01\x01\x00");
        }
        assert_canonical_if_accepted(&bytes);
    }

    for _ in 0..10_000 {
        let value = generated_value(&mut state, 0);
        let bytes = encode_value(&value).expect("bounded generated value encodes");
        assert_eq!(decode_value(&bytes), Ok(value));
    }
}

fn assert_canonical_if_accepted(bytes: &[u8]) {
    if let Ok(value) = decode_value(bytes) {
        assert_eq!(
            encode_value(&value).expect("accepted value re-encodes"),
            bytes
        );
    }
}

fn logical_value(specification: &str) -> Value {
    match specification {
        "null" => Value::Null,
        "false" => Value::Bool(false),
        "true" => Value::Bool(true),
        "list:" => Value::list(Vec::new()).expect("empty list"),
        "list:null,true" => Value::list(vec![Value::Null, Value::Bool(true)]).expect("list"),
        "map:" => Value::map(Vec::new()).expect("empty map"),
        "map:a=null,b=true" => Value::map(vec![
            (
                BoundedString::new("a".to_owned()).expect("key"),
                Value::Null,
            ),
            (
                BoundedString::new("b".to_owned()).expect("key"),
                Value::Bool(true),
            ),
        ])
        .expect("map"),
        "instant:-1,500000000" => {
            Value::Instant(UtcInstant::new(-1, 500_000_000).expect("instant"))
        }
        "instant:0,0" => Value::Instant(UtcInstant::new(0, 0).expect("epoch")),
        "ref:zero" => Value::RecordRef(RecordRef::new(
            DatabaseId::from_bytes([0; 16]),
            NamespaceId::from_bytes([0; 16]),
            RecordId::from_bytes([0; 16]),
        )),
        "ref:full" => Value::RecordRef(RecordRef::new(
            DatabaseId::from_bytes(core::array::from_fn(|index| index as u8)),
            NamespaceId::from_bytes(core::array::from_fn(|index| (index + 16) as u8)),
            RecordId::from_bytes(core::array::from_fn(|index| (index + 32) as u8)),
        )),
        other if other.starts_with("instant:") => {
            let mut parts = other[8..].split(',');
            let seconds = parts.next().expect("seconds").parse().expect("seconds");
            let nanoseconds = parts.next().expect("nanos").parse().expect("nanos");
            assert!(parts.next().is_none());
            Value::Instant(UtcInstant::new(seconds, nanoseconds).expect("instant"))
        }
        other if other.starts_with("u:") => {
            Value::Unsigned(other[2..].parse().expect("unsigned vector"))
        }
        other if other.starts_with("i:") => {
            Value::Signed(other[2..].parse().expect("signed vector"))
        }
        other if other.starts_with("str:") => {
            Value::string(other[4..].to_owned()).expect("string vector")
        }
        other if other.starts_with("bytes:") => {
            Value::bytes(decode_hex(&other[6..])).expect("bytes vector")
        }
        other => panic!("unknown logical vector {other}"),
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"USTE\x01\x01\x00".to_vec();
    append_uleb(payload.len() as u128, &mut bytes);
    bytes.extend_from_slice(payload);
    bytes
}

fn append_uleb(mut value: u128, output: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            output.push(byte);
            return;
        }
        output.push(byte | 0x80);
    }
}

fn decode_hex(encoded: &str) -> Vec<u8> {
    assert_eq!(encoded.len() % 2, 0);
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("non-lowercase hex vector"),
    }
}

fn xorshift(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^ (state << 17)
}

fn generated_value(state: &mut u64, depth: usize) -> Value {
    *state = xorshift(*state);
    let choice = if depth >= 4 { *state % 6 } else { *state % 10 };
    match choice {
        0 => Value::Null,
        1 => Value::Bool(*state & 1 == 1),
        2 => Value::Unsigned(u128::from(*state)),
        3 => Value::Signed(i128::from(*state as i64)),
        4 => {
            let length = (*state as usize) & 0x1f;
            let mut bytes = Vec::with_capacity(length);
            for _ in 0..length {
                *state = xorshift(*state);
                bytes.push(*state as u8);
            }
            Value::bytes(bytes).expect("small generated bytes")
        }
        5 => Value::Instant(
            UtcInstant::new(
                (*state % 2_000_000) as i64 - 1_000_000,
                (*state % 1_000_000_000) as u32,
            )
            .expect("generated instant"),
        ),
        6 => {
            let count = (*state as usize) & 0x03;
            let values = (0..count)
                .map(|_| generated_value(state, depth + 1))
                .collect();
            Value::list(values).expect("shallow generated list")
        }
        7 => {
            let count = (*state as usize) & 0x03;
            let entries = (0..count)
                .map(|index| {
                    (
                        BoundedString::new(format!("key-{index}")).expect("small key"),
                        generated_value(state, depth + 1),
                    )
                })
                .collect();
            Value::map(entries).expect("shallow generated map")
        }
        8 => Value::string(format!("value-{state:016x}")).expect("small string"),
        _ => Value::RecordRef(RecordRef::new(
            DatabaseId::from_bytes(state.to_le_bytes().repeat(2).try_into().expect("16 bytes")),
            NamespaceId::from_bytes([depth as u8; 16]),
            RecordId::from_bytes([choice as u8; 16]),
        )),
    }
}
