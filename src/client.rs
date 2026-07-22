//! Transport-neutral proposal submission and receipt retrieval.

use std::{future::Future, pin::Pin};

use crate::protocol::{
    identity::RequestId, live::LiveProposalRequest, receipt::ProposalResponse,
    version::ProtocolVersion,
};

/// A boxed client future with no transport- or runtime-specific public type.
pub type ClientFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ClientError>> + Send + 'a>>;

/// Submits semantic proposals through a caller-selected transport.
pub trait AgentClient: Send + Sync {
    /// Submits one proposal request and returns its public response.
    fn submit<'a>(&'a self, request: &'a LiveProposalRequest)
    -> ClientFuture<'a, ProposalResponse>;

    /// Retrieves the public response for one caller-created request identity.
    fn receipt<'a>(&'a self, request_id: &'a RequestId) -> ClientFuture<'a, ProposalResponse>;
}

/// Reports failures before a public proposal response is available.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The selected transport failed.
    #[error("transport failed: {message}")]
    Transport {
        /// A human-readable transport explanation.
        message: String,
    },
    /// The transport response could not be decoded.
    #[error("response decoding failed: {message}")]
    Decoding {
        /// A human-readable decoding explanation.
        message: String,
    },
    /// The remote protocol version is unsupported.
    #[error("unsupported protocol version {version:?}")]
    UnsupportedVersion {
        /// The unsupported version.
        version: ProtocolVersion,
    },
}
