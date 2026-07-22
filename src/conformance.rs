//! Embedded schemas and fixtures for consumer contract tests.

use serde::Deserialize;

use crate::protocol::{
    error::ErrorCode,
    value::ContentDigest,
    version::{PROTOCOL_VERSION, ProtocolVersion},
};

/// Describes one embedded JSON Schema document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schema {
    /// The stable asset name.
    pub name: &'static str,
    /// The public Rust root type represented by the schema.
    pub root_type: &'static str,
    /// The exact published schema bytes.
    pub bytes: &'static [u8],
}

/// Declares whether a fixture must pass or fail consumer validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureExpectation {
    /// The fixture is a valid canonical public value.
    Valid,
    /// The fixture must be rejected.
    Invalid,
}

/// Describes one embedded public contract fixture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixture {
    /// The stable asset name.
    pub name: &'static str,
    /// The public root type or paired consumer case represented by the fixture.
    pub root_type: &'static str,
    /// The exact published fixture bytes.
    pub bytes: &'static [u8],
    /// Whether the fixture must pass or fail.
    pub expectation: FixtureExpectation,
    /// The expected stable public error for invalid consumer cases.
    pub expected_error: Option<ErrorCode>,
}

/// Returns the protocol version covered by the embedded contract.
#[must_use]
pub const fn contract_version() -> ProtocolVersion {
    PROTOCOL_VERSION
}

/// Returns the aggregate digest recorded in the generated manifest.
#[must_use]
pub fn contract_digest() -> ContentDigest {
    let manifest: Manifest = serde_json::from_slice(include_bytes!("../contract/v1/manifest.json"))
        .expect("generated contract manifest must be valid JSON");
    ContentDigest::parse(manifest.contract_digest)
        .expect("generated contract digest must use the public digest format")
}

/// Returns every published schema in stable path order.
#[must_use]
pub const fn schemas() -> &'static [Schema] {
    SCHEMAS
}

/// Returns every published valid and invalid fixture in stable path order.
#[must_use]
pub const fn fixtures() -> &'static [Fixture] {
    FIXTURES
}

#[derive(Deserialize)]
struct Manifest {
    contract_digest: String,
}

const SCHEMAS: &[Schema] = &[
    Schema {
        name: "agent-trace.schema.json",
        root_type: "AgentTrace",
        bytes: include_bytes!("../contract/v1/schema/agent-trace.schema.json"),
    },
    Schema {
        name: "decision-receipt.schema.json",
        root_type: "DecisionReceipt",
        bytes: include_bytes!("../contract/v1/schema/decision-receipt.schema.json"),
    },
    Schema {
        name: "live-proposal-request.schema.json",
        root_type: "LiveProposalRequest",
        bytes: include_bytes!("../contract/v1/schema/live-proposal-request.schema.json"),
    },
    Schema {
        name: "observation.schema.json",
        root_type: "Observation",
        bytes: include_bytes!("../contract/v1/schema/observation.schema.json"),
    },
    Schema {
        name: "proposal-response.schema.json",
        root_type: "ProposalResponse",
        bytes: include_bytes!("../contract/v1/schema/proposal-response.schema.json"),
    },
];

macro_rules! valid_fixture {
    ($name:literal, $root:literal) => {
        Fixture {
            name: $name,
            root_type: $root,
            bytes: include_bytes!(concat!("../contract/v1/fixtures/valid/", $name)),
            expectation: FixtureExpectation::Valid,
            expected_error: None,
        }
    };
}

macro_rules! invalid_fixture {
    ($name:literal, $root:literal, $error:expr) => {
        Fixture {
            name: $name,
            root_type: $root,
            bytes: include_bytes!(concat!("../contract/v1/fixtures/invalid/", $name)),
            expectation: FixtureExpectation::Invalid,
            expected_error: $error,
        }
    };
}

const FIXTURES: &[Fixture] = &[
    valid_fixture!("error-forbidden.json", "ProposalResponse"),
    valid_fixture!("error-unsupported-version.json", "ProposalResponse"),
    valid_fixture!("full-live-observation.json", "Observation"),
    valid_fixture!("receipt-accepted.json", "DecisionReceipt"),
    valid_fixture!("receipt-authorized.json", "DecisionReceipt"),
    valid_fixture!("receipt-completed.json", "DecisionReceipt"),
    valid_fixture!("receipt-dispatch-unknown.json", "DecisionReceipt"),
    valid_fixture!("receipt-dispatched.json", "DecisionReceipt"),
    valid_fixture!("receipt-not-dispatched.json", "DecisionReceipt"),
    valid_fixture!("receipt-rejected.json", "DecisionReceipt"),
    valid_fixture!("redacted-observation.json", "Observation"),
    valid_fixture!("reduce-position-request.json", "LiveProposalRequest"),
    valid_fixture!("trace-no-proposal.json", "AgentTrace"),
    valid_fixture!("trace-policy-failure.json", "AgentTrace"),
    valid_fixture!("trace-proposal.json", "AgentTrace"),
    valid_fixture!("trace-timeout.json", "AgentTrace"),
    invalid_fixture!(
        "client-intent-id.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "digest-mismatch.json",
        "Observation",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "dispatch-claim.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "expired-observation.json",
        "Observation",
        Some(ErrorCode::StaleObservation)
    ),
    invalid_fixture!(
        "expiry-before-creation.json",
        "Observation",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "exponent-quantity.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "guardrail-result.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "idempotency-conflict.json",
        "LiveProposalRequest",
        Some(ErrorCode::IdempotencyConflict)
    ),
    Fixture {
        name: "idempotency-original.json",
        root_type: "LiveProposalRequest",
        bytes: include_bytes!("../contract/v1/fixtures/invalid/idempotency-original.json"),
        expectation: FixtureExpectation::Valid,
        expected_error: None,
    },
    invalid_fixture!(
        "increment-misaligned-quantity.json",
        "ProposalCase",
        Some(ErrorCode::Rejected)
    ),
    invalid_fixture!(
        "lowered-command.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "missing-digest.json",
        "Observation",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "missing-instrument-view.json",
        "ProposalCase",
        Some(ErrorCode::Rejected)
    ),
    invalid_fixture!(
        "missing-position-view.json",
        "ProposalCase",
        Some(ErrorCode::Rejected)
    ),
    invalid_fixture!(
        "missing-source-timestamp.json",
        "Observation",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "negative-quantity.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "over-precision-quantity.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "position-instrument-mismatch.json",
        "ProposalCase",
        Some(ErrorCode::Rejected)
    ),
    invalid_fixture!(
        "private-authorization-field.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "unknown-enum-variant.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "unknown-field.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
    invalid_fixture!(
        "unsupported-feature.json",
        "ProtocolInfo",
        Some(ErrorCode::UnsupportedVersion)
    ),
    invalid_fixture!(
        "unsupported-protocol-major.json",
        "LiveProposalRequest",
        Some(ErrorCode::UnsupportedVersion)
    ),
    invalid_fixture!(
        "zero-quantity.json",
        "LiveProposalRequest",
        Some(ErrorCode::Malformed)
    ),
];
