//! Opaque typed identities and explicit scoped references.

use core::fmt;
use core::str::FromStr;

use crate::IdentityParseError;

macro_rules! identity_type {
    ($name:ident, $prefix:literal, $tag:literal, $docs:literal) => {
        #[doc = $docs]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Binary identity-kind tag in canonical records.
            pub const TAG: u8 = $tag;

            /// Construct an identity from its complete opaque bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// Return the opaque bytes without implying authority or scope.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($prefix)?;
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl FromStr for $name {
            type Err = IdentityParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                parse_identity(value, $prefix).map(Self)
            }
        }
    };
}

identity_type!(
    DatabaseId,
    "db_",
    0x01,
    "Opaque 128-bit database identity. This is not a capability."
);
identity_type!(
    NamespaceId,
    "ns_",
    0x02,
    "Opaque 128-bit namespace identity. This is not a capability."
);
identity_type!(
    RecordId,
    "rec_",
    0x03,
    "Opaque 128-bit record identity. Durable references must also carry database and namespace."
);
identity_type!(
    TransactionId,
    "txn_",
    0x04,
    "Opaque 128-bit transaction identity. Durable references must also carry scope."
);
identity_type!(
    SourceEventId,
    "src_",
    0x05,
    "Opaque 128-bit source-event identity. Durable references must also carry scope."
);
identity_type!(
    IdempotencyKey,
    "idem_",
    0x06,
    "Opaque 128-bit idempotency key. Its transaction scope also includes the principal."
);

fn parse_identity(value: &str, prefix: &str) -> Result<[u8; 16], IdentityParseError> {
    let encoded = value
        .strip_prefix(prefix)
        .filter(|digits| digits.len() == 32)
        .ok_or(IdentityParseError)?;
    let mut bytes = [0_u8; 16];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let high = lowercase_hex(pair[0]).ok_or(IdentityParseError)?;
        let low = lowercase_hex(pair[1]).ok_or(IdentityParseError)?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

const fn lowercase_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Explicit database/namespace scope.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NamespaceRef {
    database: DatabaseId,
    namespace: NamespaceId,
}

impl NamespaceRef {
    /// Construct an explicit namespace scope.
    #[must_use]
    pub const fn new(database: DatabaseId, namespace: NamespaceId) -> Self {
        Self {
            database,
            namespace,
        }
    }

    /// Database component.
    #[must_use]
    pub const fn database(self) -> DatabaseId {
        self.database
    }

    /// Namespace component.
    #[must_use]
    pub const fn namespace(self) -> NamespaceId {
        self.namespace
    }
}

macro_rules! scoped_reference {
    ($name:ident, $id:ident, $field:ident, $docs:literal) => {
        #[doc = $docs]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name {
            scope: NamespaceRef,
            $field: $id,
        }

        impl $name {
            /// Construct a fully scoped durable reference.
            #[must_use]
            pub const fn new(database: DatabaseId, namespace: NamespaceId, $field: $id) -> Self {
                Self {
                    scope: NamespaceRef::new(database, namespace),
                    $field,
                }
            }

            /// Database component.
            #[must_use]
            pub const fn database(self) -> DatabaseId {
                self.scope.database()
            }

            /// Namespace component.
            #[must_use]
            pub const fn namespace(self) -> NamespaceId {
                self.scope.namespace()
            }

            #[doc = concat!("Return the scoped `", stringify!($id), "` component.")]
            #[must_use]
            pub const fn $field(self) -> $id {
                self.$field
            }
        }
    };
}

scoped_reference!(
    RecordRef,
    RecordId,
    record,
    "A record identity with explicit database and namespace scope."
);
scoped_reference!(
    TransactionRef,
    TransactionId,
    transaction,
    "A transaction identity with explicit database and namespace scope."
);
scoped_reference!(
    SourceEventRef,
    SourceEventId,
    source_event,
    "A source-event identity with explicit database and namespace scope."
);

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::{DatabaseId, NamespaceId};

    #[test]
    fn identity_text_is_exact_and_typed() {
        let id = DatabaseId::from_bytes([0xab; 16]);
        let text = "db_abababababababababababababababab";
        assert_eq!(id.to_string(), text);
        assert_eq!(DatabaseId::from_str(text), Ok(id));
        assert!(DatabaseId::from_str("db_ABABABABABABABABABABABABABABABAB").is_err());
        assert!(DatabaseId::from_str("ns_abababababababababababababababab").is_err());
        assert!(NamespaceId::from_str(text).is_err());
    }
}
