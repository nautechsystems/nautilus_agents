//! Descriptive capability vocabulary disclosed by NautilusTrader.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::value::{InstrumentId, TimestampNs};

/// Names an observation category that NautilusTrader may disclose.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ObservationCapability {
    /// Includes scoped position summaries.
    PositionSummary,
    /// Includes scoped instrument summaries.
    InstrumentSummary,
}

/// Names a semantic proposal category that NautilusTrader may accept.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ProposalCapability {
    /// Includes position-reduction proposals.
    ReducePosition,
}

/// Describes the effective scope disclosed with one observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityGrant {
    /// The observation categories disclosed in the payload.
    pub observations: BTreeSet<ObservationCapability>,
    /// The proposal categories NautilusTrader may consider.
    pub proposals: BTreeSet<ProposalCapability>,
    /// The instruments covered by the grant.
    pub instruments: BTreeSet<InstrumentId>,
    /// The time at which the disclosed grant expires.
    pub expires_at: TimestampNs,
}
