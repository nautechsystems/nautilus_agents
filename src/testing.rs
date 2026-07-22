//! Deterministic protocol values for consumer tests.

use std::collections::BTreeSet;

use crate::{
    assurance::trace::{AgentTrace, PolicyMetadata, TraceOutcome},
    protocol::{
        capability::{CapabilityGrant, ObservationCapability, ProposalCapability},
        error::{DecisionError, ErrorCode},
        identity::{IdempotencyKey, ObservationId},
        live::{
            InstrumentView, LiveIntent, LiveObservation, LiveProposal, LiveProposalRequest,
            PositionSide, PositionView, ReducePosition,
        },
        observation::{
            FieldOmission, Observation, ObservationError, ObservationPayload, OmissionReason,
            ProvenanceSource, RetentionClass, SourceProvenance,
        },
        receipt::{CorrelationRefs, DecisionReceipt, DecisionStatus, ProposalResponse},
        value::{ContentDigest, FieldPath, InstrumentId, PositionId, Quantity, TimestampNs},
        version::{PROTOCOL_VERSION, ProtocolVersion},
    },
};

/// Builds a validated observation from deterministic or caller-supplied fields.
pub struct ObservationBuilder {
    observation: Observation,
}

impl ObservationBuilder {
    /// Creates a builder populated with distinct deterministic field values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            observation: base_observation(),
        }
    }

    /// Sets the protocol version.
    #[must_use]
    pub fn version(mut self, version: ProtocolVersion) -> Self {
        self.observation.version = version;
        self
    }

    /// Sets the observation identity.
    #[must_use]
    pub fn id(mut self, id: ObservationId) -> Self {
        self.observation.id = id;
        self
    }

    /// Sets the construction time.
    #[must_use]
    pub fn created_at(mut self, created_at: TimestampNs) -> Self {
        self.observation.created_at = created_at;
        self
    }

    /// Sets the expiry time.
    #[must_use]
    pub fn expires_at(mut self, expires_at: TimestampNs) -> Self {
        self.observation.expires_at = expires_at;
        self
    }

    /// Sets the disclosed capability grant.
    #[must_use]
    pub fn grant(mut self, grant: CapabilityGrant) -> Self {
        self.observation.grant = grant;
        self
    }

    /// Sets field provenance.
    #[must_use]
    pub fn provenance(mut self, provenance: Vec<SourceProvenance>) -> Self {
        self.observation.provenance = provenance;
        self
    }

    /// Sets explicit field omissions.
    #[must_use]
    pub fn omissions(mut self, omissions: Vec<FieldOmission>) -> Self {
        self.observation.omissions = omissions;
        self
    }

    /// Sets the recording retention class.
    #[must_use]
    pub fn retention(mut self, retention: RetentionClass) -> Self {
        self.observation.retention = retention;
        self
    }

    /// Sets the protocol payload.
    #[must_use]
    pub fn payload(mut self, payload: ObservationPayload) -> Self {
        self.observation.payload = payload;
        self
    }

    /// Recomputes the digest and validates the complete observation.
    pub fn build(mut self) -> Result<Observation, ObservationError> {
        self.observation.refresh_digest()?;
        self.observation.validate()?;
        Ok(self.observation)
    }
}

impl Default for ObservationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns one fully populated valid live observation.
#[must_use]
pub fn valid_observation() -> Observation {
    ObservationBuilder::new()
        .build()
        .expect("deterministic observation must be valid")
}

/// Returns one fully populated semantic position-reduction request.
#[must_use]
pub fn reduce_position_request() -> LiveProposalRequest {
    let observation = valid_observation();
    LiveProposalRequest {
        version: PROTOCOL_VERSION,
        request_id: parse("20000000-0000-4000-8000-000000000021"),
        idempotency_key: IdempotencyKey::parse("principal-proposal-29")
            .expect("deterministic idempotency key must be valid"),
        observation: observation.reference(),
        agent_trace_id: Some(parse("30000000-0000-4000-8000-000000000031")),
        proposal: LiveProposal {
            intent: LiveIntent::ReducePosition(ReducePosition {
                position_id: PositionId::parse("P-197")
                    .expect("deterministic position ID must be valid"),
                instrument_id: InstrumentId::parse("BTCUSDT.BINANCE")
                    .expect("deterministic instrument ID must be valid"),
                quantity: Quantity::parse("1.25").expect("deterministic quantity must be valid"),
            }),
        },
    }
}

/// Returns one fully populated proposal trace.
#[must_use]
pub fn proposal_trace() -> AgentTrace {
    let request = reduce_position_request();
    AgentTrace {
        version: PROTOCOL_VERSION,
        trace_id: parse("30000000-0000-4000-8000-000000000032"),
        observation: request.observation,
        policy: PolicyMetadata {
            name: "defensive-reducer".to_owned(),
            version: "policy-3.7.11".to_owned(),
        },
        started_at: TimestampNs::new(1_712_400_000_100_000_101),
        completed_at: TimestampNs::new(1_712_400_000_200_000_202),
        outcome: TraceOutcome::Proposed(request.proposal),
    }
}

/// Returns one fully populated accepted decision receipt.
#[must_use]
pub fn accepted_receipt() -> DecisionReceipt {
    receipt(DecisionStatus::Accepted, None, 41)
}

/// Returns one fully populated decision-path rejection response.
#[must_use]
pub fn rejected_response() -> ProposalResponse {
    ProposalResponse::Receipt(receipt(
        DecisionStatus::Rejected,
        Some(DecisionError {
            code: ErrorCode::Rejected,
            message: "fresh position state no longer permits the proposal".to_owned(),
            retryable: false,
        }),
        42,
    ))
}

/// Returns one valid observation with explicit redaction metadata.
#[must_use]
pub fn redacted_observation() -> Observation {
    let mut observation = valid_observation();
    observation.id = parse("10000000-0000-4000-8000-000000000012");
    let ObservationPayload::Live(live) = &mut observation.payload;
    live.positions.clear();
    observation.omissions = vec![FieldOmission {
        field: FieldPath::parse("/payload/live/positions")
            .expect("deterministic field path must be valid"),
        reason: OmissionReason::Redacted,
    }];
    observation.retention = RetentionClass::Restricted;
    observation
        .refresh_digest()
        .expect("deterministic redacted observation must serialize");
    observation
        .validate()
        .expect("deterministic redacted observation must be valid");
    observation
}

/// Returns one structurally valid observation that is expired at its evaluation time.
#[must_use]
pub fn expired_observation() -> Observation {
    let mut observation = valid_observation();
    observation.id = parse("10000000-0000-4000-8000-000000000013");
    observation
        .refresh_digest()
        .expect("deterministic expired observation must serialize");
    observation
}

fn base_observation() -> Observation {
    let instrument_id =
        InstrumentId::parse("BTCUSDT.BINANCE").expect("deterministic instrument ID must be valid");
    Observation {
        version: PROTOCOL_VERSION,
        id: parse("10000000-0000-4000-8000-000000000011"),
        created_at: TimestampNs::new(1_712_400_000_000_000_101),
        expires_at: TimestampNs::new(1_712_400_030_000_000_202),
        grant: CapabilityGrant {
            observations: BTreeSet::from([
                ObservationCapability::PositionSummary,
                ObservationCapability::InstrumentSummary,
            ]),
            proposals: BTreeSet::from([ProposalCapability::ReducePosition]),
            instruments: BTreeSet::from([instrument_id.clone()]),
            expires_at: TimestampNs::new(1_712_400_060_000_000_303),
        },
        provenance: vec![
            SourceProvenance {
                field: FieldPath::parse("/payload/live/positions")
                    .expect("deterministic field path must be valid"),
                source: ProvenanceSource::PositionState,
                observed_at: TimestampNs::new(1_712_399_999_000_000_404),
                version: Some("position-revision-17".to_owned()),
            },
            SourceProvenance {
                field: FieldPath::parse("/payload/live/instruments")
                    .expect("deterministic field path must be valid"),
                source: ProvenanceSource::InstrumentDefinition,
                observed_at: TimestampNs::new(1_712_399_998_000_000_505),
                version: Some("instrument-revision-23".to_owned()),
            },
        ],
        omissions: Vec::new(),
        retention: RetentionClass::Derived,
        payload: ObservationPayload::Live(LiveObservation {
            positions: vec![PositionView {
                position_id: PositionId::parse("P-197")
                    .expect("deterministic position ID must be valid"),
                instrument_id: instrument_id.clone(),
                side: PositionSide::Long,
                quantity: Quantity::parse("2.75")
                    .expect("deterministic position quantity must be valid"),
                updated_at: TimestampNs::new(1_712_399_997_000_000_606),
            }],
            instruments: vec![InstrumentView {
                instrument_id,
                quantity_increment: Quantity::parse("0.25")
                    .expect("deterministic quantity increment must be valid"),
            }],
        }),
        digest: ContentDigest::new([0; 32]),
    }
}

fn receipt(status: DecisionStatus, error: Option<DecisionError>, offset: u64) -> DecisionReceipt {
    let receipt = DecisionReceipt {
        version: PROTOCOL_VERSION,
        request_id: parse("20000000-0000-4000-8000-000000000021"),
        observation: valid_observation().reference(),
        intent_id: parse(&format!("40000000-0000-4000-8000-{offset:012}")),
        status,
        error,
        correlation: CorrelationRefs {
            engine_correlation_id: Some(parse(&format!("50000000-0000-4000-8000-{offset:012}"))),
            engine_causation_id: Some(parse(&format!("60000000-0000-4000-8000-{offset:012}"))),
        },
        created_at: TimestampNs::new(1_712_400_001_000_000_707 + offset),
        updated_at: TimestampNs::new(1_712_400_002_000_000_808 + offset),
    };
    receipt
        .validate()
        .expect("deterministic receipt must be valid");
    receipt
}

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value
        .parse()
        .expect("deterministic protocol value must be valid")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_all_deterministic_values_are_complete_and_valid() {
        let observation = valid_observation();
        let request = reduce_position_request();
        let trace = proposal_trace();
        let accepted = accepted_receipt();
        let redacted = redacted_observation();

        assert_eq!(observation.validate(), Ok(()));
        assert_eq!(request.validate(), Ok(()));
        assert_eq!(request.observation, observation.reference());
        assert_eq!(trace.observation, observation.reference());
        assert_eq!(accepted.validate(), Ok(()));
        assert_eq!(accepted.status, DecisionStatus::Accepted);
        assert_eq!(redacted.validate(), Ok(()));
        assert_eq!(redacted.retention, RetentionClass::Restricted);
        assert_eq!(redacted.omissions.len(), 1);
    }

    #[rstest]
    fn test_rejected_and_expired_values_have_exact_outcomes() {
        let ProposalResponse::Receipt(rejected) = rejected_response() else {
            panic!("deterministic rejection must be a receipt");
        };
        let expired = expired_observation();
        let evaluation_time = TimestampNs::new(expired.expires_at.as_u64() + 1);

        assert_eq!(rejected.status, DecisionStatus::Rejected);
        assert_eq!(rejected.error.unwrap().code, ErrorCode::Rejected);
        assert_eq!(expired.validate(), Ok(()));
        assert_eq!(
            expired.validate_at(evaluation_time),
            Err(ObservationError::Expired {
                expires_at: expired.expires_at,
            })
        );
    }

    #[rstest]
    fn test_builder_sets_every_observation_field() {
        let baseline = valid_observation();
        let id: ObservationId = parse("10000000-0000-4000-8000-000000000014");
        let created_at = TimestampNs::new(1_712_400_000_000_001_117);
        let expires_at = TimestampNs::new(1_712_400_031_000_001_219);
        let mut grant = baseline.grant.clone();
        grant.expires_at = TimestampNs::new(1_712_400_061_000_001_323);
        let mut provenance = baseline.provenance.clone();
        provenance[0].version = Some("position-revision-53".to_owned());
        provenance[1].version = Some("instrument-revision-59".to_owned());
        let omissions = vec![FieldOmission {
            field: FieldPath::parse("/payload/live/optional_extension").unwrap(),
            reason: OmissionReason::Unsupported,
        }];
        let mut payload = baseline.payload;
        let ObservationPayload::Live(live) = &mut payload;
        live.positions[0].quantity = Quantity::parse("3.25").unwrap();
        live.instruments[0].quantity_increment = Quantity::parse("0.5").unwrap();

        let actual = ObservationBuilder::new()
            .version(PROTOCOL_VERSION)
            .id(id.clone())
            .created_at(created_at)
            .expires_at(expires_at)
            .grant(grant.clone())
            .provenance(provenance.clone())
            .omissions(omissions.clone())
            .retention(RetentionClass::ReferenceOnly)
            .payload(payload.clone())
            .build()
            .unwrap();
        let mut expected = Observation {
            version: PROTOCOL_VERSION,
            id,
            created_at,
            expires_at,
            grant,
            provenance,
            omissions,
            retention: RetentionClass::ReferenceOnly,
            payload,
            digest: ContentDigest::new([0; 32]),
        };
        expected.refresh_digest().unwrap();

        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_builder_returns_exact_validation_error() {
        assert_eq!(
            ObservationBuilder::new()
                .version(ProtocolVersion::new(2, 0))
                .build(),
            Err(ObservationError::UnsupportedVersion {
                version: ProtocolVersion::new(2, 0),
            })
        );
    }
}
