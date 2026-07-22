//! Stable public errors for request and decision outcomes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::identity::RequestId;

/// Carries a stable decision-path error code and retry hint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionError {
    /// The stable machine-readable error code.
    pub code: ErrorCode,
    /// A human-readable, unstable explanation.
    pub message: String,
    /// Whether retrying the same logical operation may succeed.
    pub retryable: bool,
}

/// Carries an error before NautilusTrader accepts a request into the decision path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    /// The request identity when NautilusTrader could decode it.
    pub request_id: Option<RequestId>,
    /// The stable machine-readable error code.
    pub code: ErrorCode,
    /// A human-readable, unstable explanation.
    pub message: String,
    /// Whether retrying the same logical operation may succeed.
    pub retryable: bool,
}

/// Enumerates stable public request and decision error categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request could not be decoded or validated.
    Malformed,
    /// The protocol version is unsupported.
    UnsupportedVersion,
    /// The authenticated principal cannot submit the request.
    Forbidden,
    /// The referenced observation is no longer usable.
    StaleObservation,
    /// NautilusTrader could not establish a safe decision state.
    IndeterminateState,
    /// NautilusTrader rejected the semantic proposal.
    Rejected,
    /// A principal-scoped idempotency key was reused with different content.
    IdempotencyConflict,
    /// NautilusTrader could not durably commit the accepted decision.
    CommitFailed,
    /// NautilusTrader could not establish a safe dispatch result.
    DispatchFailed,
}
