//! Validated generic values for the canonical v1 codec.

use crate::{RecordRef, UtcInstant, ValidationError};

/// Maximum encoded bytes in one inline bytes/string value.
pub const MAX_INLINE_BYTES: usize = 1_048_576;

/// Maximum entries in one list or map.
pub const MAX_COLLECTION_ENTRIES: usize = 65_536;

/// Maximum list/map container depth, counting the root container as one.
pub const MAX_NESTING_DEPTH: usize = 32;

/// Maximum scalar and container nodes in one generic value tree.
pub const MAX_VALUE_NODES: usize = 262_144;

/// An inline byte sequence admitted by format 1.0.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedBytes(Vec<u8>);

impl BoundedBytes {
    /// Validate and retain exact bytes without interpretation.
    pub fn new(bytes: Vec<u8>) -> Result<Self, ValidationError> {
        check_inline_length(bytes.len())?;
        Ok(Self(bytes))
    }

    /// Borrow the exact bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Consume the wrapper and return its exact bytes.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// A UTF-8 string admitted by format 1.0 without Unicode normalization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedString(String);

impl BoundedString {
    /// Validate the UTF-8 byte length and preserve the exact scalar sequence.
    pub fn new(value: String) -> Result<Self, ValidationError> {
        check_inline_length(value.len())?;
        Ok(Self(value))
    }

    /// Borrow the string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// A bounded canonical list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedList(Vec<Value>);

impl BoundedList {
    /// Validate entry count and complete nested depth.
    pub fn new(values: Vec<Value>) -> Result<Self, ValidationError> {
        check_collection_length(values.len())?;
        let mut nodes = 1;
        for value in &values {
            value.validate_limits(1, &mut nodes)?;
        }
        Ok(Self(values))
    }

    /// Borrow list elements in canonical order.
    #[must_use]
    pub fn as_slice(&self) -> &[Value] {
        &self.0
    }

    /// Consume the wrapper and return its elements.
    #[must_use]
    pub fn into_vec(self) -> Vec<Value> {
        self.0
    }

    pub(crate) fn from_decoded(values: Vec<Value>) -> Self {
        Self(values)
    }
}

/// A bounded map whose keys are unique and ordered by unsigned UTF-8 bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalMap(Vec<(BoundedString, Value)>);

impl CanonicalMap {
    /// Sort entries canonically and reject duplicate exact key bytes.
    pub fn new(mut entries: Vec<(BoundedString, Value)>) -> Result<Self, ValidationError> {
        check_collection_length(entries.len())?;
        entries.sort_by(|left, right| left.0.as_str().as_bytes().cmp(right.0.as_str().as_bytes()));
        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ValidationError::DuplicateMapKey);
        }
        let mut nodes = 1;
        for (_, value) in &entries {
            value.validate_limits(1, &mut nodes)?;
        }
        Ok(Self(entries))
    }

    /// Borrow entries in canonical key order.
    #[must_use]
    pub fn as_slice(&self) -> &[(BoundedString, Value)] {
        &self.0
    }

    /// Consume the wrapper and return canonical entries.
    #[must_use]
    pub fn into_vec(self) -> Vec<(BoundedString, Value)> {
        self.0
    }

    pub(crate) fn from_decoded(entries: Vec<(BoundedString, Value)>) -> Self {
        Self(entries)
    }
}

/// Closed generic value set for canonical format 1.0.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// Explicit null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Unsigned 128-bit integer.
    Unsigned(u128),
    /// Signed 128-bit integer, distinct from the unsigned domain.
    Signed(i128),
    /// Exact inline bytes.
    Bytes(BoundedBytes),
    /// Exact UTF-8 string.
    String(BoundedString),
    /// Bounded list.
    List(BoundedList),
    /// Canonically ordered string-keyed map.
    Map(CanonicalMap),
    /// Normalized POSIX UTC instant pair.
    Instant(UtcInstant),
    /// Explicitly database- and namespace-scoped record reference.
    RecordRef(RecordRef),
}

impl Value {
    /// Construct an inline byte value.
    pub fn bytes(bytes: Vec<u8>) -> Result<Self, ValidationError> {
        BoundedBytes::new(bytes).map(Self::Bytes)
    }

    /// Construct a UTF-8 string value.
    pub fn string(value: String) -> Result<Self, ValidationError> {
        BoundedString::new(value).map(Self::String)
    }

    /// Construct a list and validate its complete depth.
    pub fn list(values: Vec<Self>) -> Result<Self, ValidationError> {
        BoundedList::new(values).map(Self::List)
    }

    /// Construct a map, sorting keys and validating duplicates/depth.
    pub fn map(entries: Vec<(BoundedString, Self)>) -> Result<Self, ValidationError> {
        CanonicalMap::new(entries).map(Self::Map)
    }

    pub(crate) fn validate_limits(
        &self,
        parent_depth: usize,
        nodes: &mut usize,
    ) -> Result<(), ValidationError> {
        *nodes = nodes.saturating_add(1);
        if *nodes > MAX_VALUE_NODES {
            return Err(ValidationError::TooManyValueNodes {
                actual: *nodes,
                maximum: MAX_VALUE_NODES,
            });
        }
        let is_container = matches!(self, Self::List(_) | Self::Map(_));
        if !is_container {
            return Ok(());
        }
        let depth = parent_depth.saturating_add(1);
        if depth > MAX_NESTING_DEPTH {
            return Err(ValidationError::NestingTooDeep {
                actual: depth,
                maximum: MAX_NESTING_DEPTH,
            });
        }
        match self {
            Self::List(list) => {
                for child in list.as_slice() {
                    child.validate_limits(depth, nodes)?;
                }
            }
            Self::Map(map) => {
                for (_, child) in map.as_slice() {
                    child.validate_limits(depth, nodes)?;
                }
            }
            _ => unreachable!("non-container returned above"),
        }
        Ok(())
    }
}

fn check_inline_length(length: usize) -> Result<(), ValidationError> {
    if length > MAX_INLINE_BYTES {
        return Err(ValidationError::InlineValueTooLarge {
            actual: length,
            maximum: MAX_INLINE_BYTES,
        });
    }
    Ok(())
}

fn check_collection_length(length: usize) -> Result<(), ValidationError> {
    if length > MAX_COLLECTION_ENTRIES {
        return Err(ValidationError::CollectionTooLarge {
            actual: length,
            maximum: MAX_COLLECTION_ENTRIES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BoundedString, MAX_INLINE_BYTES, MAX_NESTING_DEPTH, Value};

    #[test]
    fn map_sorts_exact_utf8_and_rejects_duplicates() {
        let map = Value::map(vec![
            (
                BoundedString::new("b".to_owned()).expect("bounded"),
                Value::Null,
            ),
            (
                BoundedString::new("a".to_owned()).expect("bounded"),
                Value::Bool(true),
            ),
        ])
        .expect("map");
        let Value::Map(map) = map else {
            panic!("map variant")
        };
        assert_eq!(map.as_slice()[0].0.as_str(), "a");

        let key = BoundedString::new("same".to_owned()).expect("bounded");
        assert!(Value::map(vec![(key.clone(), Value::Null), (key, Value::Null)]).is_err());
    }

    #[test]
    fn bounds_are_inclusive_and_depth_is_checked_on_composition() {
        assert!(Value::bytes(vec![0; MAX_INLINE_BYTES]).is_ok());
        assert!(Value::bytes(vec![0; MAX_INLINE_BYTES + 1]).is_err());

        let mut value = Value::Null;
        for _ in 0..MAX_NESTING_DEPTH {
            value = Value::list(vec![value]).expect("depth within cap");
        }
        assert!(Value::list(vec![value]).is_err());
    }
}
