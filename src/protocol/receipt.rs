//! Public NautilusTrader decision receipts and correlation references.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    error::{DecisionError, ErrorCode, ProtocolError},
    identity::{CorrelationId, IntentId, RequestId},
    observation::ObservationRef,
    value::TimestampNs,
    version::ProtocolVersion,
};

/// Reports an inconsistent public decision receipt.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    /// The receipt uses an unsupported protocol version.
    #[error("unsupported protocol version {version:?}")]
    UnsupportedVersion {
        /// The unsupported version.
        version: ProtocolVersion,
    },
    /// The receipt update precedes its creation.
    #[error("receipt update precedes creation")]
    UpdateBeforeCreation,
    /// The status requires a decision error.
    #[error("receipt status {status:?} requires an error")]
    MissingError {
        /// The inconsistent status.
        status: DecisionStatus,
    },
    /// The status cannot carry a decision error.
    #[error("receipt status {status:?} cannot carry an error")]
    UnexpectedError {
        /// The inconsistent status.
        status: DecisionStatus,
    },
    /// The error code is incompatible with the status.
    #[error("error code {code:?} is incompatible with receipt status {status:?}")]
    ErrorCodeMismatch {
        /// The inconsistent status.
        status: DecisionStatus,
        /// The inconsistent error code.
        code: ErrorCode,
    },
}

/// Reports the stable public outcome for an accepted request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionReceipt {
    /// The external protocol version.
    pub version: ProtocolVersion,
    /// The caller-created request identity.
    pub request_id: RequestId,
    /// The exact observation considered by NautilusTrader.
    pub observation: ObservationRef,
    /// The intent identity minted by NautilusTrader after request acceptance.
    pub intent_id: IntentId,
    /// The stable public decision status.
    pub status: DecisionStatus,
    /// An error for terminal negative or uncertain states.
    pub error: Option<DecisionError>,
    /// Public engine correlation references when available.
    pub correlation: CorrelationRefs,
    /// The receipt creation time.
    pub created_at: TimestampNs,
    /// The most recent receipt update time.
    pub updated_at: TimestampNs,
}

impl DecisionReceipt {
    /// Validates public receipt invariants.
    pub fn validate(&self) -> Result<(), ReceiptError> {
        if !self.version.is_supported() {
            return Err(ReceiptError::UnsupportedVersion {
                version: self.version,
            });
        }

        if self.updated_at < self.created_at {
            return Err(ReceiptError::UpdateBeforeCreation);
        }
        match (self.status, self.error.as_ref()) {
            (
                DecisionStatus::Rejected
                | DecisionStatus::NotDispatched
                | DecisionStatus::DispatchUnknown,
                None,
            ) => {
                return Err(ReceiptError::MissingError {
                    status: self.status,
                });
            }
            (
                DecisionStatus::Accepted
                | DecisionStatus::Authorized
                | DecisionStatus::Dispatched
                | DecisionStatus::Completed,
                Some(_),
            ) => {
                return Err(ReceiptError::UnexpectedError {
                    status: self.status,
                });
            }
            (_, _) => {}
        }

        if let Some(error) = &self.error
            && !error_matches_status(self.status, error.code)
        {
            return Err(ReceiptError::ErrorCodeMismatch {
                status: self.status,
                code: error.code,
            });
        }
        Ok(())
    }
}

fn error_matches_status(status: DecisionStatus, code: ErrorCode) -> bool {
    match status {
        DecisionStatus::Rejected => matches!(
            code,
            ErrorCode::Forbidden
                | ErrorCode::StaleObservation
                | ErrorCode::IndeterminateState
                | ErrorCode::Rejected
                | ErrorCode::IdempotencyConflict
        ),
        DecisionStatus::NotDispatched => {
            matches!(code, ErrorCode::CommitFailed | ErrorCode::DispatchFailed)
        }
        DecisionStatus::DispatchUnknown => code == ErrorCode::DispatchFailed,
        DecisionStatus::Accepted
        | DecisionStatus::Authorized
        | DecisionStatus::Dispatched
        | DecisionStatus::Completed => false,
    }
}

/// Enumerates stable public decision and dispatch progress states.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    /// NautilusTrader accepted and durably identified the request.
    Accepted,
    /// NautilusTrader rejected the semantic proposal.
    Rejected,
    /// NautilusTrader authorized the proposal for private lowering.
    Authorized,
    /// NautilusTrader did not dispatch a command.
    NotDispatched,
    /// NautilusTrader dispatched the private command.
    Dispatched,
    /// NautilusTrader cannot yet establish the dispatch outcome.
    DispatchUnknown,
    /// NautilusTrader completed the public decision lifecycle.
    Completed,
}

/// Carries public links into engine correlation without exposing private records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorrelationRefs {
    /// The engine correlation identity when one exists.
    pub engine_correlation_id: Option<CorrelationId>,
    /// The engine causation identity when one exists.
    pub engine_causation_id: Option<CorrelationId>,
}

/// Separates accepted decision receipts from request-level protocol errors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalResponse {
    /// NautilusTrader accepted the request into the decision path.
    Receipt(DecisionReceipt),
    /// NautilusTrader rejected the request before decision-path acceptance.
    Error(ProtocolError),
}
