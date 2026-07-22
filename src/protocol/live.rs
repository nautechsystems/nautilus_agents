//! Live observations and semantic position-reduction proposals.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    identity::{IdempotencyKey, RequestId, TraceId},
    observation::ObservationRef,
    value::{InstrumentId, PositionId, Quantity, TimestampNs},
    version::ProtocolVersion,
};

/// Reports a structurally invalid live proposal request.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    /// The request uses an unsupported protocol version.
    #[error("unsupported protocol version {version:?}")]
    UnsupportedVersion {
        /// The unsupported version.
        version: ProtocolVersion,
    },
}

/// Contains the live views disclosed for one observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveObservation {
    /// The scoped position summaries.
    pub positions: Vec<PositionView>,
    /// The scoped instrument summaries.
    pub instruments: Vec<InstrumentView>,
}

/// Contains the minimum position state needed by a proposal policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PositionView {
    /// The position identity.
    pub position_id: PositionId,
    /// The position's instrument identity.
    pub instrument_id: InstrumentId,
    /// The open position side.
    pub side: PositionSide,
    /// The current open quantity.
    pub quantity: Quantity,
    /// The last position-state update time.
    pub updated_at: TimestampNs,
}

/// Identifies the direction of an open position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PositionSide {
    /// The position is long.
    Long,
    /// The position is short.
    Short,
}

/// Contains the minimum instrument rules needed by a proposal policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstrumentView {
    /// The instrument identity.
    pub instrument_id: InstrumentId,
    /// The permitted quantity increment.
    pub quantity_increment: Quantity,
}

/// Submits one semantic live proposal against an exact observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveProposalRequest {
    /// The external protocol version.
    pub version: ProtocolVersion,
    /// The caller-created request identity.
    pub request_id: RequestId,
    /// The caller-created key that NautilusTrader scopes to the principal.
    pub idempotency_key: IdempotencyKey,
    /// The exact observation used by the policy.
    pub observation: ObservationRef,
    /// An optional link to the agent-side trace.
    pub agent_trace_id: Option<TraceId>,
    /// The single semantic proposal.
    pub proposal: LiveProposal,
}

impl LiveProposalRequest {
    /// Validates request-level protocol invariants.
    pub fn validate(&self) -> Result<(), RequestError> {
        if !self.version.is_supported() {
            return Err(RequestError::UnsupportedVersion {
                version: self.version,
            });
        }
        Ok(())
    }
}

/// Wraps the single live intent carried by a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveProposal {
    /// The semantic intent for NautilusTrader to consider.
    pub intent: LiveIntent,
}

/// Enumerates the live proposal vocabulary for protocol 1.0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LiveIntent {
    /// Proposes reducing an existing position without adding risk.
    ReducePosition(ReducePosition),
}

/// Identifies an existing position and the quantity to reduce.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReducePosition {
    /// The position to reduce.
    pub position_id: PositionId,
    /// The instrument expected for the fresh position.
    pub instrument_id: InstrumentId,
    /// The strictly positive reduction quantity.
    pub quantity: Quantity,
}
