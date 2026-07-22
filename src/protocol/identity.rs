//! Distinct request, observation, trace, intent, and correlation identities.

use std::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

const UUID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// Reports invalid protocol identity values.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// The UUID is not lowercase hyphenated form.
    #[error("UUID must use lowercase hyphenated form")]
    InvalidUuid,
    /// The idempotency key is empty.
    #[error("idempotency key must not be empty")]
    EmptyIdempotencyKey,
    /// The idempotency key is longer than 128 bytes.
    #[error("idempotency key exceeds 128 bytes")]
    IdempotencyKeyTooLong,
    /// The idempotency key contains a non-ASCII byte.
    #[error("idempotency key must contain only ASCII bytes")]
    NonAsciiIdempotencyKey,
}

macro_rules! uuid_identity {
    ($name:ident, $docs:literal) => {
        #[doc = $docs]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(#[schemars(regex(pattern = UUID_PATTERN))] String);

        impl $name {
            /// Creates a random UUID v4 identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4().hyphenated().to_string())
            }

            /// Parses a lowercase hyphenated UUID.
            pub fn parse(value: impl AsRef<str>) -> Result<Self, IdentityError> {
                let value = value.as_ref();
                let uuid = Uuid::parse_str(value).map_err(|_| IdentityError::InvalidUuid)?;
                if uuid.hyphenated().to_string() != value {
                    return Err(IdentityError::InvalidUuid);
                }
                Ok(Self(value.to_owned()))
            }

            /// Returns the lowercase hyphenated UUID.
            #[must_use]
            pub fn as_str(&self) -> &str {
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
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

uuid_identity!(RequestId, "Identifies a caller-created proposal request.");
uuid_identity!(
    ObservationId,
    "Identifies an observation constructed by NautilusTrader."
);
uuid_identity!(TraceId, "Identifies an agent-side trace.");
uuid_identity!(IntentId, "Identifies an intent accepted by NautilusTrader.");
uuid_identity!(
    CorrelationId,
    "Identifies public correlation or causation metadata."
);

/// Carries a caller-supplied key that NautilusTrader scopes to the authenticated principal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct IdempotencyKey(
    #[schemars(length(min = 1, max = 128), regex(pattern = r"^[\u0000-\u007f]+$"))] String,
);

impl IdempotencyKey {
    /// Creates a checked idempotency key.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        Self::parse(value.into())
    }

    /// Parses an opaque ASCII idempotency key of 1 to 128 bytes.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, IdentityError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(IdentityError::EmptyIdempotencyKey);
        }

        if value.len() > 128 {
            return Err(IdentityError::IdempotencyKeyTooLong);
        }

        if !value.is_ascii() {
            return Err(IdentityError::NonAsciiIdempotencyKey);
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the opaque key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IdempotencyKey {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const UUID: &str = "12345678-90ab-4cde-8fab-1234567890ab";

    #[rstest]
    fn test_uuid_identities_exact_json() {
        let request_id = RequestId::parse(UUID).unwrap();
        let json = serde_json::to_string(&request_id).unwrap();
        assert_eq!(json, format!(r#""{UUID}""#));
        assert_eq!(
            serde_json::from_str::<RequestId>(&json).unwrap(),
            request_id
        );
    }

    #[rstest]
    fn test_uuid_identity_rejects_noncanonical_forms() {
        assert_eq!(
            RequestId::parse(UUID.to_uppercase()),
            Err(IdentityError::InvalidUuid)
        );
        assert_eq!(
            RequestId::parse("1234567890ab4cde8fab1234567890ab"),
            Err(IdentityError::InvalidUuid)
        );
    }

    #[rstest]
    fn test_idempotency_key_boundaries_and_exact_json() {
        let key = IdempotencyKey::new("reduce-btc-001").unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), r#""reduce-btc-001""#);
        assert_eq!(key.as_str(), "reduce-btc-001");
        assert_eq!(
            IdempotencyKey::new(""),
            Err(IdentityError::EmptyIdempotencyKey)
        );
        assert_eq!(
            IdempotencyKey::new("x".repeat(129)),
            Err(IdentityError::IdempotencyKeyTooLong)
        );
        assert_eq!(
            IdempotencyKey::new("position-\u{2713}"),
            Err(IdentityError::NonAsciiIdempotencyKey)
        );
    }
}
