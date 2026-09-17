#![no_main]

use libfuzzer_sys::fuzz_target;
use uste_types::{
    BoundedString, DatabaseId, NamespaceId, RecordId, RecordRef, UtcInstant, Value, decode_value,
    encode_value, encoded_len,
};

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let value = generate_value(&mut input, 0);
    let encoded = encode_value(&value).expect("small generated value is valid");
    assert_eq!(encoded_len(&value), Ok(encoded.len()));
    assert_eq!(decode_value(&encoded), Ok(value));
});

fn generate_value(input: &mut Input<'_>, depth: usize) -> Value {
    match input.byte() % if depth < 4 { 10 } else { 7 } {
        0 => Value::Null,
        1 => Value::Bool(input.byte() & 1 == 1),
        2 => Value::Unsigned(input.u128()),
        3 => Value::Signed(input.u128() as i128),
        4 => Value::bytes(input.bytes(32)).expect("bounded"),
        5 => {
            Value::string(String::from_utf8_lossy(&input.bytes(32)).into_owned()).expect("bounded")
        }
        6 => {
            let seconds = i64::from(input.byte()) - 128;
            Value::Instant(UtcInstant::new(seconds, u32::from(input.byte())).expect("in range"))
        }
        7 => {
            let count = usize::from(input.byte() % 8);
            let values = (0..count)
                .map(|_| generate_value(input, depth + 1))
                .collect();
            Value::list(values).expect("small bounded list")
        }
        8 => {
            let count = usize::from(input.byte() % 8);
            let entries = (0..count)
                .map(|index| {
                    (
                        BoundedString::new(format!("{index:02x}")).expect("bounded key"),
                        generate_value(input, depth + 1),
                    )
                })
                .collect();
            Value::map(entries).expect("small bounded map")
        }
        9 => Value::RecordRef(RecordRef::new(
            DatabaseId::from_bytes(input.array()),
            NamespaceId::from_bytes(input.array()),
            RecordId::from_bytes(input.array()),
        )),
        _ => unreachable!(),
    }
}

struct Input<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Input<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn byte(&mut self) -> u8 {
        let value = self.data.get(self.offset).copied().unwrap_or(0);
        self.offset = self.offset.saturating_add(1);
        value
    }

    fn bytes(&mut self, maximum: usize) -> Vec<u8> {
        let length = usize::from(self.byte()) % (maximum + 1);
        (0..length).map(|_| self.byte()).collect()
    }

    fn array(&mut self) -> [u8; 16] {
        core::array::from_fn(|_| self.byte())
    }

    fn u128(&mut self) -> u128 {
        u128::from_le_bytes(self.array())
    }
}
