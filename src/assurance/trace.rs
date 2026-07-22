//! Agent-side policy traces that retain only an observation reference.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::protocol::{
    identity::TraceId, live::LiveProposal, observation::ObservationRef, value::TimestampNs,
    version::ProtocolVersion,
};

/// Records one complete agent-side policy evaluation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentTrace {
    /// The external protocol version.
    pub version: ProtocolVersion,
    /// The agent-created trace identity.
    pub trace_id: TraceId,
    /// The exact observation used by the policy.
    pub observation: ObservationRef,
    /// The caller-supplied policy identity.
    pub policy: PolicyMetadata,
    /// The policy evaluation start time.
    pub started_at: TimestampNs,
    /// The policy evaluation completion time.
    pub completed_at: TimestampNs,
    /// The complete agent-side policy outcome.
    pub outcome: TraceOutcome,
}

/// Identifies a caller's policy implementation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyMetadata {
    /// The policy name.
    pub name: String,
    /// The caller-defined policy version.
    pub version: String,
}

/// Captures a proposal, deliberate no-proposal result, or local failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    /// The policy produced one semantic proposal.
    Proposed(LiveProposal),
    /// The policy deliberately produced no proposal.
    NoProposal,
    /// The local policy evaluation failed.
    Failed(AgentFailure),
}

/// Describes a local policy failure without implying a NautilusTrader outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentFailure {
    /// The stable local failure category.
    pub kind: AgentFailureKind,
    /// A human-readable failure explanation.
    pub message: String,
}

/// Enumerates local policy failure categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentFailureKind {
    /// The policy returned an error.
    PolicyError,
    /// The local runner reached its configured timeout.
    Timeout,
    /// The policy future panicked.
    Panic,
}
