//! Canonical byte encoding and strict decoding for format 1.0.

use crate::{
    BoundedBytes, BoundedList, BoundedString, CanonicalMap, DatabaseId, DecodeError, EncodeError,
    MAX_VALUE_NODES, NamespaceId, RecordId, RecordRef, UtcInstant, Value,
};

/// Four-byte canonical frame marker.
pub const MAGIC: [u8; 4] = *b"USTE";

/// Generic-value record kind assigned by Decision 0012.
pub const VALUE_RECORD_KIND: u8 = 0x01;

/// Canonical format major.
pub const FORMAT_MAJOR: u8 = 1;

/// Canonical format minor written by this implementation.
pub const FORMAT_MINOR: u8 = 0;

/// Maximum generic-value payload length.
pub const MAX_VALUE_PAYLOAD: usize = 16 * 1024 * 1024;

const NULL_TAG: u8 = 0x00;
const FALSE_TAG: u8 = 0x01;
const TRUE_TAG: u8 = 0x02;
const UNSIGNED_TAG: u8 = 0x03;
const SIGNED_TAG: u8 = 0x04;
const BYTES_TAG: u8 = 0x05;
const STRING_TAG: u8 = 0x06;
const LIST_TAG: u8 = 0x07;
const MAP_TAG: u8 = 0x08;
const INSTANT_TAG: u8 = 0x09;
const RECORD_REF_TAG: u8 = 0x0a;

/// Compute the exact canonical frame length without allocating output.
pub fn encoded_len(value: &Value) -> Result<usize, EncodeError> {
    let payload = payload_len(value)?;
    let header = MAGIC.len() + 3 + uleb_len(payload as u128);
    header
        .checked_add(payload)
        .ok_or(EncodeError::PayloadTooLarge {
            actual: usize::MAX,
            maximum: MAX_VALUE_PAYLOAD,
        })
}

/// Encode one validated value as one complete canonical format-1.0 frame.
pub fn encode_value(value: &Value) -> Result<Vec<u8>, EncodeError> {
    let payload = payload_len(value)?;
    let capacity = MAGIC.len() + 3 + uleb_len(payload as u128) + payload;
    let mut output = Vec::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| EncodeError::ResourceLimit)?;
    output.extend_from_slice(&MAGIC);
    output.extend_from_slice(&[VALUE_RECORD_KIND, FORMAT_MAJOR, FORMAT_MINOR]);
    write_uleb(payload as u128, &mut output);
    encode_payload(value, &mut output);
    Ok(output)
}

/// Decode exactly one canonical format-1.0 generic-value frame.
pub fn decode_value(input: &[u8]) -> Result<Value, DecodeError> {
    let payload_bytes = frame_payload(input)?;
    let mut payload = Cursor::new(payload_bytes);
    let mut budget = DecodeBudget::new();
    let value = decode_payload(&mut payload, 0, &mut budget)?;
    if payload.remaining() != 0 {
        return Err(DecodeError::TrailingBytes);
    }
    Ok(value)
}

/// Decode a complete canonical value when its root is a map, borrowing only its validated keys.
/// Nested values remain fully owned and obey the same depth, node and byte limits as
/// [`decode_value`]. A valid non-map root returns `None`.
pub fn decode_map_value(input: &[u8]) -> Result<Option<Vec<(&str, Value)>>, DecodeError> {
    decode_map_entries(input, |_, payload, depth, budget| {
        decode_payload(payload, depth, budget)
    })
}

/// One decoded root-map value whose direct string payload may borrow from the input frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BorrowedMapValue<'a> {
    /// A direct string payload borrowed from the input frame.
    String(&'a str),
    /// A recursively borrowed map selected explicitly by the decoder's caller.
    Map(Vec<(&'a str, BorrowedMapValue<'a>)>),
    /// Every other payload, including nested strings, in its ordinary owned representation.
    Value(Value),
}

/// Decode a complete canonical root map while borrowing its keys and direct string values.
///
/// Nested values remain fully owned and all validation and resource limits match [`decode_value`].
/// A valid non-map root returns `None`.
pub fn decode_borrowed_map_value(
    input: &[u8],
) -> Result<Option<Vec<(&str, BorrowedMapValue<'_>)>>, DecodeError> {
    decode_map_entries(input, |_, payload, depth, budget| {
        decode_borrowed_map_payload(payload, depth, budget)
    })
}

/// Decode a canonical root map while recursively borrowing maps in selected root fields.
///
/// Root keys and direct root strings always borrow as in [`decode_borrowed_map_value`]. A selected
/// field whose payload is a map becomes [`BorrowedMapValue::Map`], recursively borrowing its map
/// keys and direct strings. Unselected nested values remain fully owned.
pub fn decode_borrowed_map_value_with_fields<'a>(
    input: &'a [u8],
    borrowed_map_fields: &[&str],
) -> Result<Option<Vec<(&'a str, BorrowedMapValue<'a>)>>, DecodeError> {
    decode_map_entries(input, |key, payload, depth, budget| {
        if borrowed_map_fields.contains(&key) && payload.peek()? == MAP_TAG {
            decode_borrowed_map_tree(payload, depth, budget)
        } else {
            decode_borrowed_map_payload(payload, depth, budget)
        }
    })
}

fn decode_map_entries<'a, T>(
    input: &'a [u8],
    mut decode_entry: impl FnMut(
        &'a str,
        &mut Cursor<'a>,
        usize,
        &mut DecodeBudget,
    ) -> Result<T, DecodeError>,
) -> Result<Option<Vec<(&'a str, T)>>, DecodeError> {
    let payload_bytes = frame_payload(input)?;
    if payload_bytes.first().copied() != Some(MAP_TAG) {
        decode_value(input)?;
        return Ok(None);
    }
    let mut payload = Cursor::new(payload_bytes);
    let tag = payload.byte()?;
    debug_assert_eq!(tag, MAP_TAG);
    let mut budget = DecodeBudget::new();
    budget.take_node()?;
    let depth = check_depth(0)?;
    let count = read_collection_length(&mut payload)?;
    if count > payload.remaining() {
        return Err(DecodeError::UnexpectedEof);
    }
    budget.require_nodes(count)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::ResourceLimit)?;
    let mut previous: Option<&[u8]> = None;
    for _ in 0..count {
        let key = read_str(&mut payload)?;
        if previous.is_some_and(|old| old >= key.as_bytes()) {
            return Err(DecodeError::MapKeysOutOfOrder);
        }
        previous = Some(key.as_bytes());
        entries.push((key, decode_entry(key, &mut payload, depth, &mut budget)?));
    }
    if payload.remaining() != 0 {
        return Err(DecodeError::TrailingBytes);
    }
    Ok(Some(entries))
}

fn decode_borrowed_map_payload<'a>(
    payload: &mut Cursor<'a>,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<BorrowedMapValue<'a>, DecodeError> {
    if payload.peek()? != STRING_TAG {
        return decode_payload(payload, depth, budget).map(BorrowedMapValue::Value);
    }
    budget.take_node()?;
    let tag = payload.byte()?;
    debug_assert_eq!(tag, STRING_TAG);
    read_str(payload).map(BorrowedMapValue::String)
}

fn decode_borrowed_map_tree<'a>(
    payload: &mut Cursor<'a>,
    depth: usize,
    budget: &mut DecodeBudget,
) -> Result<BorrowedMapValue<'a>, DecodeError> {
    match payload.peek()? {
        STRING_TAG => decode_borrowed_map_payload(payload, depth, budget),
        MAP_TAG => {
            budget.take_node()?;
            let tag = payload.byte()?;
            debug_assert_eq!(tag, MAP_TAG);
            let depth = check_depth(depth)?;
            let count = read_collection_length(payload)?;
            if count > payload.remaining() {
                return Err(DecodeError::UnexpectedEof);
            }
            budget.require_nodes(count)?;
            let mut entries = Vec::new();
            entries
                .try_reserve_exact(count)
                .map_err(|_| DecodeError::ResourceLimit)?;
            let mut previous: Option<&[u8]> = None;
            for _ in 0..count {
                let key = read_str(payload)?;
                if previous.is_some_and(|old| old >= key.as_bytes()) {
                    return Err(DecodeError::MapKeysOutOfOrder);
                }
                previous = Some(key.as_bytes());
                entries.push((key, decode_borrowed_map_tree(payload, depth, budget)?));
            }
            Ok(BorrowedMapValue::Map(entries))
        }
        _ => decode_payload(payload, depth, budget).map(BorrowedMapValue::Value),
    }
}

fn frame_payload(input: &[u8]) -> Result<&[u8], DecodeError> {
    let mut frame = Cursor::new(input);
    if frame.take(MAGIC.len())? != MAGIC {
        return Err(DecodeError::InvalidMagic);
    }
    let record_kind = frame.byte()?;
    let major = frame.byte()?;
    let minor = frame.byte()?;
    if record_kind != VALUE_RECORD_KIND || major != FORMAT_MAJOR || minor > FORMAT_MINOR {
        return Err(DecodeError::UnsupportedVersion {
            record_kind,
            major,
            minor,
        });
    }
    let declared = read_length(&mut frame)?;
    if declared > MAX_VALUE_PAYLOAD {
        return Err(DecodeError::PayloadTooLarge {
            actual: declared,
            maximum: MAX_VALUE_PAYLOAD,
        });
    }
    let payload_bytes = frame.take(declared)?;
    if frame.remaining() != 0 {
        return Err(DecodeError::TrailingBytes);
    }
    Ok(payload_bytes)
}

fn payload_len(value: &Value) -> Result<usize, EncodeError> {
    fn add(total: &mut usize, amount: usize) -> Result<(), EncodeError> {
        *total = total
            .checked_add(amount)
            .ok_or(EncodeError::PayloadTooLarge {
                actual: usize::MAX,
                maximum: MAX_VALUE_PAYLOAD,
            })?;
        if *total > MAX_VALUE_PAYLOAD {
            return Err(EncodeError::PayloadTooLarge {
                actual: *total,
                maximum: MAX_VALUE_PAYLOAD,
            });
        }
        Ok(())
    }

    fn measure(value: &Value, total: &mut usize, nodes: &mut usize) -> Result<(), EncodeError> {
        *nodes = nodes.saturating_add(1);
        if *nodes > MAX_VALUE_NODES {
            return Err(EncodeError::TooManyValueNodes {
                actual: *nodes,
                maximum: MAX_VALUE_NODES,
            });
        }
        add(total, 1)?;
        match value {
            Value::Null | Value::Bool(_) => {}
            Value::Unsigned(number) => add(total, uleb_len(*number))?,
            Value::Signed(number) => {
                let (_, magnitude) = signed_parts(*number);
                add(total, 1 + uleb_len(magnitude))?;
            }
            Value::Bytes(bytes) => {
                add(total, uleb_len(bytes.as_slice().len() as u128))?;
                add(total, bytes.as_slice().len())?;
            }
            Value::String(string) => {
                add(total, uleb_len(string.as_str().len() as u128))?;
                add(total, string.as_str().len())?;
            }
            Value::List(list) => {
                add(total, uleb_len(list.as_slice().len() as u128))?;
                for child in list.as_slice() {
                    measure(child, total, nodes)?;
                }
            }
            Value::Map(map) => {
                add(total, uleb_len(map.as_slice().len() as u128))?;
                for (key, child) in map.as_slice() {
                    add(total, uleb_len(key.as_str().len() as u128))?;
                    add(total, key.as_str().len())?;
                    measure(child, total, nodes)?;
                }
            }
            Value::Instant(instant) => {
                let (_, seconds) = signed_parts(i128::from(instant.seconds()));
                add(total, 1 + uleb_len(seconds))?;
                add(total, uleb_len(u128::from(instant.nanoseconds())))?;
            }
            Value::RecordRef(_) => add(total, 3 * 17)?,
        }
        Ok(())
    }

    let mut total = 0;
    let mut nodes = 0;
    measure(value, &mut total, &mut nodes)?;
    Ok(total)
}

fn encode_payload(value: &Value, output: &mut Vec<u8>) {
    match value {
        Value::Null => output.push(NULL_TAG),
        Value::Bool(false) => output.push(FALSE_TAG),
        Value::Bool(true) => output.push(TRUE_TAG),
        Value::Unsigned(number) => {
            output.push(UNSIGNED_TAG);
            write_uleb(*number, output);
        }
        Value::Signed(number) => {
            output.push(SIGNED_TAG);
            write_signed(*number, output);
        }
        Value::Bytes(bytes) => {
            output.push(BYTES_TAG);
            write_uleb(bytes.as_slice().len() as u128, output);
            output.extend_from_slice(bytes.as_slice());
        }
        Value::String(string) => {
            output.push(STRING_TAG);
            write_uleb(string.as_str().len() as u128, output);
            output.extend_from_slice(string.as_str().as_bytes());
        }
        Value::List(list) => {
            output.push(LIST_TAG);
            write_uleb(list.as_slice().len() as u128, output);
            for child in list.as_slice() {
                encode_payload(child, output);
            }
        }
        Value::Map(map) => {
            output.push(MAP_TAG);
            write_uleb(map.as_slice().len() as u128, output);
            for (key, child) in map.as_slice() {
                write_uleb(key.as_str().len() as u128, output);
                output.extend_from_slice(key.as_str().as_bytes());
                encode_payload(child, output);
            }
        }
        Value::Instant(instant) => {
            output.push(INSTANT_TAG);
            write_signed(i128::from(instant.seconds()), output);
            write_uleb(u128::from(instant.nanoseconds()), output);
        }
        Value::RecordRef(reference) => {
            output.push(RECORD_REF_TAG);
            write_identity(DatabaseId::TAG, reference.database().as_bytes(), output);
            write_identity(NamespaceId::TAG, reference.namespace().as_bytes(), output);
            write_identity(RecordId::TAG, reference.record().as_bytes(), output);
        }
    }
}

fn decode_payload(
    cursor: &mut Cursor<'_>,
    parent_depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Value, DecodeError> {
    let tag = cursor.byte()?;
    budget.take_node()?;
    match tag {
        NULL_TAG => Ok(Value::Null),
        FALSE_TAG => Ok(Value::Bool(false)),
        TRUE_TAG => Ok(Value::Bool(true)),
        UNSIGNED_TAG => read_uleb(cursor).map(Value::Unsigned),
        SIGNED_TAG => read_signed(cursor).map(Value::Signed),
        BYTES_TAG => {
            let length = read_inline_length(cursor)?;
            let source = cursor.take(length)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| DecodeError::ResourceLimit)?;
            bytes.extend_from_slice(source);
            Ok(Value::Bytes(
                BoundedBytes::new(bytes).expect("decoded inline length was checked"),
            ))
        }
        STRING_TAG => {
            let string = read_string(cursor)?;
            Ok(Value::String(string))
        }
        LIST_TAG => decode_list(cursor, parent_depth, budget),
        MAP_TAG => decode_map(cursor, parent_depth, budget),
        INSTANT_TAG => {
            let seconds =
                i64::try_from(read_signed(cursor)?).map_err(|_| DecodeError::InvalidInstant)?;
            let nanoseconds =
                u32::try_from(read_uleb(cursor)?).map_err(|_| DecodeError::InvalidInstant)?;
            let instant =
                UtcInstant::new(seconds, nanoseconds).map_err(|_| DecodeError::InvalidInstant)?;
            Ok(Value::Instant(instant))
        }
        RECORD_REF_TAG => {
            let database = DatabaseId::from_bytes(read_identity::<{ DatabaseId::TAG }>(cursor)?);
            let namespace = NamespaceId::from_bytes(read_identity::<{ NamespaceId::TAG }>(cursor)?);
            let record = RecordId::from_bytes(read_identity::<{ RecordId::TAG }>(cursor)?);
            Ok(Value::RecordRef(RecordRef::new(
                database, namespace, record,
            )))
        }
        tag => Err(DecodeError::InvalidValueTag(tag)),
    }
}

fn decode_list(
    cursor: &mut Cursor<'_>,
    parent_depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Value, DecodeError> {
    let depth = check_depth(parent_depth)?;
    let count = read_collection_length(cursor)?;
    if count > cursor.remaining() {
        return Err(DecodeError::UnexpectedEof);
    }
    budget.require_nodes(count)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::ResourceLimit)?;
    for _ in 0..count {
        values.push(decode_payload(cursor, depth, budget)?);
    }
    Ok(Value::List(BoundedList::from_decoded(values)))
}

fn decode_map(
    cursor: &mut Cursor<'_>,
    parent_depth: usize,
    budget: &mut DecodeBudget,
) -> Result<Value, DecodeError> {
    let depth = check_depth(parent_depth)?;
    let count = read_collection_length(cursor)?;
    if count > cursor.remaining() {
        return Err(DecodeError::UnexpectedEof);
    }
    budget.require_nodes(count)?;
    let mut entries: Vec<(BoundedString, Value)> = Vec::new();
    entries
        .try_reserve_exact(count)
        .map_err(|_| DecodeError::ResourceLimit)?;
    for _ in 0..count {
        let key = read_string(cursor)?;
        if entries
            .last()
            .is_some_and(|(previous, _)| previous.as_str().as_bytes() >= key.as_str().as_bytes())
        {
            return Err(DecodeError::MapKeysOutOfOrder);
        }
        let value = decode_payload(cursor, depth, budget)?;
        entries.push((key, value));
    }
    Ok(Value::Map(CanonicalMap::from_decoded(entries)))
}

fn check_depth(parent: usize) -> Result<usize, DecodeError> {
    let depth = parent.saturating_add(1);
    if depth > crate::MAX_NESTING_DEPTH {
        return Err(DecodeError::NestingTooDeep {
            actual: depth,
            maximum: crate::MAX_NESTING_DEPTH,
        });
    }
    Ok(depth)
}

fn read_inline_length(cursor: &mut Cursor<'_>) -> Result<usize, DecodeError> {
    let length = read_length(cursor)?;
    if length > crate::MAX_INLINE_BYTES {
        return Err(DecodeError::InlineValueTooLarge {
            actual: length,
            maximum: crate::MAX_INLINE_BYTES,
        });
    }
    Ok(length)
}

fn read_collection_length(cursor: &mut Cursor<'_>) -> Result<usize, DecodeError> {
    let length = read_length(cursor)?;
    if length > crate::MAX_COLLECTION_ENTRIES {
        return Err(DecodeError::CollectionTooLarge {
            actual: length,
            maximum: crate::MAX_COLLECTION_ENTRIES,
        });
    }
    Ok(length)
}

fn read_string(cursor: &mut Cursor<'_>) -> Result<BoundedString, DecodeError> {
    let text = read_str(cursor)?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| DecodeError::ResourceLimit)?;
    owned.push_str(text);
    Ok(BoundedString::new(owned).expect("decoded inline length was checked"))
}

fn read_str<'a>(cursor: &mut Cursor<'a>) -> Result<&'a str, DecodeError> {
    let length = read_inline_length(cursor)?;
    let bytes = cursor.take(length)?;
    core::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)
}

struct DecodeBudget {
    used_nodes: usize,
}

impl DecodeBudget {
    const fn new() -> Self {
        Self { used_nodes: 0 }
    }

    fn take_node(&mut self) -> Result<(), DecodeError> {
        self.require_nodes(1)?;
        self.used_nodes += 1;
        Ok(())
    }

    fn require_nodes(&self, minimum_more: usize) -> Result<(), DecodeError> {
        let actual = self.used_nodes.saturating_add(minimum_more);
        if actual > MAX_VALUE_NODES {
            return Err(DecodeError::TooManyValueNodes {
                actual,
                maximum: MAX_VALUE_NODES,
            });
        }
        Ok(())
    }
}

fn write_identity(tag: u8, bytes: &[u8; 16], output: &mut Vec<u8>) {
    output.push(tag);
    output.extend_from_slice(bytes);
}

fn read_identity<const TAG: u8>(cursor: &mut Cursor<'_>) -> Result<[u8; 16], DecodeError> {
    let actual = cursor.byte()?;
    if actual != TAG {
        return Err(DecodeError::InvalidIdentityTag {
            expected: TAG,
            actual,
        });
    }
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(cursor.take(16)?);
    Ok(bytes)
}

fn signed_parts(value: i128) -> (u8, u128) {
    if value < 0 {
        (1, value.unsigned_abs())
    } else {
        (0, value as u128)
    }
}

fn write_signed(value: i128, output: &mut Vec<u8>) {
    let (sign, magnitude) = signed_parts(value);
    output.push(sign);
    write_uleb(magnitude, output);
}

fn read_signed(cursor: &mut Cursor<'_>) -> Result<i128, DecodeError> {
    let sign = cursor.byte()?;
    let magnitude = read_uleb(cursor)?;
    match sign {
        0 if magnitude <= i128::MAX as u128 => Ok(magnitude as i128),
        1 if magnitude == 0 => Err(DecodeError::InvalidSignedInteger),
        1 if magnitude == (1_u128 << 127) => Ok(i128::MIN),
        1 if magnitude <= i128::MAX as u128 => Ok(-(magnitude as i128)),
        _ => Err(DecodeError::InvalidSignedInteger),
    }
}

fn uleb_len(mut value: u128) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

fn write_uleb(mut value: u128, output: &mut Vec<u8>) {
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

fn read_length(cursor: &mut Cursor<'_>) -> Result<usize, DecodeError> {
    let value = read_uleb(cursor)?;
    usize::try_from(value).map_err(|_| DecodeError::IntegerOverflow)
}

fn read_uleb(cursor: &mut Cursor<'_>) -> Result<u128, DecodeError> {
    let mut value = 0_u128;
    for index in 0..19 {
        let byte = cursor.byte()?;
        let low = byte & 0x7f;
        if index == 18 && low > 0x03 {
            return Err(DecodeError::IntegerOverflow);
        }
        value |= u128::from(low) << (index * 7);
        if byte & 0x80 == 0 {
            if index > 0 && low == 0 {
                return Err(DecodeError::NonMinimalInteger);
            }
            return Ok(value);
        }
    }
    Err(DecodeError::IntegerOverflow)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn byte(&mut self) -> Result<u8, DecodeError> {
        let byte = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or(DecodeError::UnexpectedEof)?;
        self.offset += 1;
        Ok(byte)
    }

    fn peek(&self) -> Result<u8, DecodeError> {
        self.bytes
            .get(self.offset)
            .copied()
            .ok_or(DecodeError::UnexpectedEof)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(DecodeError::IntegerOverflow)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(DecodeError::UnexpectedEof)?;
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_value, encode_value};
    use crate::{DecodeError, Value};

    #[test]
    fn nonminimal_and_overflowing_uleb_are_rejected() {
        let nonminimal = b"USTE\x01\x01\x00\x03\x03\x80\x00";
        assert_eq!(
            decode_value(nonminimal),
            Err(DecodeError::NonMinimalInteger)
        );

        let mut overflow = b"USTE\x01\x01\x00".to_vec();
        overflow.extend_from_slice(&[0x81; 19]);
        assert_eq!(decode_value(&overflow), Err(DecodeError::IntegerOverflow));
    }

    #[test]
    fn encoded_frame_is_stable_across_threads() {
        let expected = encode_value(&Value::Unsigned(300)).expect("encode");
        let handles: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| encode_value(&Value::Unsigned(300)).expect("encode")))
            .collect();
        for handle in handles {
            assert_eq!(handle.join().expect("thread"), expected);
        }
    }
}
