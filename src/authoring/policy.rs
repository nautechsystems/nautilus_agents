//! Proposal policy interface and local outcomes.

use std::{future::Future, pin::Pin};

use crate::protocol::{live::LiveProposal, observation::Observation, value::FieldPath};

/// A boxed proposal-policy future with no runtime-specific public type.
pub type ProposalFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProposalDecision, ProposalError>> + Send + 'a>>;

/// Evaluates one scoped observation and optionally proposes a semantic action.
pub trait ProposalPolicy: Send + Sync {
    /// Produces a local proposal decision for the supplied observation.
    fn propose<'a>(&'a self, observation: &'a Observation) -> ProposalFuture<'a>;
}

/// Captures the complete successful result of a local policy evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalDecision {
    /// The policy produced one semantic proposal.
    Propose(LiveProposal),
    /// The policy deliberately produced no proposal.
    NoProposal,
}

/// Reports a local policy evaluation error.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProposalError {
    /// The observation lacks fields required by the policy.
    #[error("observation lacks required fields: {fields:?}")]
    InsufficientObservation {
        /// The missing public field paths.
        fields: Vec<FieldPath>,
    },
    /// The policy encountered an internal error.
    #[error("{message}")]
    Internal {
        /// A human-readable local explanation.
        message: String,
    },
}
