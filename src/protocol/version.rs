//! Protocol version and feature declarations.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The external protocol version implemented by this crate.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(1, 0);

/// Identifies an external protocol revision independently of crate SemVer.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    /// The compatibility-breaking protocol component.
    pub major: u16,
    /// The backward-compatible protocol component.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Creates a protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns whether this crate supports the version.
    #[must_use]
    pub fn is_supported(self) -> bool {
        self.major == PROTOCOL_VERSION.major && self.minor.cmp(&PROTOCOL_VERSION.minor).is_le()
    }
}

/// Declares a protocol capability understood by both peers.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// Supports semantic position-reduction proposals.
    LiveReducePosition,
    /// Supports public decision receipts.
    DecisionReceipt,
}

/// Describes a peer's protocol version and supported features.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtocolInfo {
    /// The peer's protocol version.
    pub version: ProtocolVersion,
    /// The features supported by the peer.
    pub features: BTreeSet<Feature>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_protocol_version_exact_json() {
        let json = serde_json::to_string(&PROTOCOL_VERSION).unwrap();
        assert_eq!(json, r#"{"major":1,"minor":0}"#);
    }

    #[rstest]
    fn test_protocol_version_support() {
        assert!(PROTOCOL_VERSION.is_supported());
        assert!(!ProtocolVersion::new(2, 0).is_supported());
        assert!(!ProtocolVersion::new(1, 1).is_supported());
    }

    #[rstest]
    fn test_protocol_info_rejects_unknown_field() {
        let json = r#"{"version":{"major":1,"minor":0},"features":[],"authority":true}"#;
        let error = serde_json::from_str::<ProtocolInfo>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field `authority`"));
    }
}
