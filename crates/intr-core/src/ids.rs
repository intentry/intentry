use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Macro: generate ID newtypes
// ---------------------------------------------------------------------------

/// Generates a strongly-typed UUIDv7 ID newtype with a fixed string prefix.
///
/// # Serialization
///
/// Values serialize as `"<PREFIX>_<uuid>"` (e.g. `"prm_018f4c3d-..."`).
/// The deserializer accepts both `"<PREFIX>_<uuid>"` and bare UUIDs.
macro_rules! define_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            /// Generate a new random UUIDv7 ID.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wrap an existing UUID (useful when reading from storage).
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Return the inner UUID.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", Self::PREFIX, self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                // Accept "prefix_uuid" or bare UUID.
                let uuid_str = if let Some(rest) = s.strip_prefix(Self::PREFIX) {
                    rest.strip_prefix('_').ok_or_else(|| {
                        IdParseError::InvalidFormat(s.to_string())
                    })?
                } else {
                    s
                };
                Uuid::parse_str(uuid_str)
                    .map(Self)
                    .map_err(|e| IdParseError::InvalidUuid(e.to_string()))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse::<Self>().map_err(serde::de::Error::custom)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// ID newtypes
// ---------------------------------------------------------------------------

define_id!(SpaceId,   "spc");
define_id!(PromptId,  "prm");
define_id!(CommitId,  "cmt");
define_id!(AccountId, "acc");
define_id!(RunId,     "run");

// ---------------------------------------------------------------------------
// IdParseError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum IdParseError {
    #[error("invalid ID format: '{0}'")]
    InvalidFormat(String),

    #[error("invalid UUID: {0}")]
    InvalidUuid(String),
}

// ---------------------------------------------------------------------------
// ContentHashParseError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ContentHashParseError {
    #[error("expected 'sha256:' prefix")]
    MissingPrefix,

    #[error("invalid hex: {0}")]
    InvalidHex(String),

    #[error("hash must be 32 bytes")]
    WrongLength,
}

// ---------------------------------------------------------------------------
// ContentHash
// ---------------------------------------------------------------------------

/// A SHA-256 content-addressed blob identifier.
///
/// Serializes as `"sha256:<lowercase-hex>"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    /// Compute the hash of raw bytes.
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        Self(h.finalize().into())
    }

    /// Return the hex string (without prefix).
    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:{}", self.hex())
    }
}

impl std::str::FromStr for ContentHash {
    type Err = ContentHashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex_str = s
            .strip_prefix("sha256:")
            .ok_or(ContentHashParseError::MissingPrefix)?;
        let bytes = hex::decode(hex_str)
            .map_err(|e| ContentHashParseError::InvalidHex(e.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| ContentHashParseError::WrongLength)?;
        Ok(ContentHash(arr))
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_id_roundtrip() {
        let id = SpaceId::new();
        let serialized = serde_json::to_string(&id).unwrap();
        assert!(serialized.contains("\"spc_"));
        let back: SpaceId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn prompt_id_bare_uuid() {
        let uuid = Uuid::now_v7();
        let s = uuid.to_string();
        let id: PromptId = s.parse().unwrap();
        assert_eq!(id.as_uuid(), &uuid);
    }

    #[test]
    fn content_hash_roundtrip() {
        let hash = ContentHash::of(b"hello world");
        let s = serde_json::to_string(&hash).unwrap();
        assert!(s.contains("sha256:"));
        let back: ContentHash = serde_json::from_str(&s).unwrap();
        assert_eq!(hash, back);
    }

    #[test]
    fn content_hash_known_value() {
        // SHA-256 of "hello world" is known.
        let hash = ContentHash::of(b"hello world");
        assert_eq!(
            hash.hex(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        );
    }
}
