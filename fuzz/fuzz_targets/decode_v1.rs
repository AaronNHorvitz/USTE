#![no_main]

use libfuzzer_sys::fuzz_target;
use uste_types::{decode_value, encode_value};

fuzz_target!(|data: &[u8]| {
    assert_canonical_if_accepted(data);

    if let Some((&selector, payload)) = data.split_first() {
        let mut framed = b"USTE\x01\x01\x00".to_vec();
        if selector & 1 == 0 && payload.len() < 0x80 {
            framed.push(payload.len() as u8);
            framed.extend_from_slice(payload);
            assert_canonical_if_accepted(&framed);
        }
    }
});

fn assert_canonical_if_accepted(bytes: &[u8]) {
    if let Ok(value) = decode_value(bytes) {
        assert_eq!(
            encode_value(&value).expect("accepted value re-encodes"),
            bytes
        );
    }
}
