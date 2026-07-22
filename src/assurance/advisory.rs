//! Advisory-only checks for semantic position-reduction proposals.
//!
//! A clear report is local evidence only. NautilusTrader revalidates every input and may reject a
//! proposal after every advisory check is clear.

use crate::protocol::{
    live::{LiveIntent, LiveProposal, ReducePosition},
    observation::Observation,
    value::{FieldPath, TimestampNs},
};

/// Runs local checks without granting production authority.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdvisoryValidator;

impl AdvisoryValidator {
    /// Checks one proposal against one exact observation and evaluation time.
    #[must_use]
    pub fn evaluate(
        &self,
        observation: &Observation,
        proposal: &LiveProposal,
        now: TimestampNs,
    ) -> AdvisoryReport {
        let mut findings = Vec::new();

        if !observation.version.is_supported() {
            findings.push(finding(
                AdvisoryCode::UnsupportedVersion,
                "/version",
                "the observation protocol version is unsupported",
            ));
        }

        if now > observation.expires_at {
            findings.push(finding(
                AdvisoryCode::ExpiredObservation,
                "/expires_at",
                "the observation has expired",
            ));
        }
        match observation.computed_digest() {
            Ok(expected) if expected != observation.digest => findings.push(finding(
                AdvisoryCode::DigestMismatch,
                "/digest",
                "the observation digest does not match its content",
            )),
            Err(e) => findings.push(finding(
                AdvisoryCode::DigestMismatch,
                "/digest",
                &format!("the observation digest cannot be computed: {e}"),
            )),
            Ok(_) => {}
        }
        for omission in &observation.omissions {
            if omission_is_required(omission.field.as_str()) {
                findings.push(AdvisoryFinding {
                    code: AdvisoryCode::RequiredFieldOmitted,
                    severity: AdvisorySeverity::Error,
                    field: omission.field.clone(),
                    message: "the observation omits data needed to check the proposal".to_owned(),
                });
            }
        }

        let LiveIntent::ReducePosition(reduction) = &proposal.intent;
        evaluate_reduction(observation, reduction, &mut findings);

        AdvisoryReport { findings }
    }
}

fn omission_is_required(path: &str) -> bool {
    let segments: Vec<_> = path.split('/').skip(1).collect();
    match segments.as_slice() {
        ["payload"] | ["payload", "live"] => true,
        ["payload", "live", "positions"] | ["payload", "live", "instruments"] => true,
        ["payload", "live", "positions", _] | ["payload", "live", "instruments", _] => true,
        ["payload", "live", "positions", _, field] => {
            matches!(*field, "position_id" | "instrument_id" | "quantity")
        }
        ["payload", "live", "instruments", _, field] => {
            matches!(*field, "instrument_id" | "quantity_increment")
        }
        _ => false,
    }
}

/// Contains every local finding for one advisory evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvisoryReport {
    /// The local findings in deterministic check order.
    pub findings: Vec<AdvisoryFinding>,
}

impl AdvisoryReport {
    /// Returns true when no local finding was produced.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Describes one local advisory finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvisoryFinding {
    /// The stable local finding code.
    pub code: AdvisoryCode,
    /// The local severity.
    pub severity: AdvisorySeverity,
    /// The public field associated with the finding when one exists.
    pub field: FieldPath,
    /// A human-readable local explanation.
    pub message: String,
}

/// Classifies local advisory finding severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdvisorySeverity {
    /// The proposal cannot be checked safely from the supplied public values.
    Error,
}

/// Enumerates stable local advisory finding categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdvisoryCode {
    /// The observation protocol version is unsupported.
    UnsupportedVersion,
    /// The observation has expired.
    ExpiredObservation,
    /// The observation digest does not match its content.
    DigestMismatch,
    /// A disclosed omission covers data needed by the proposal.
    RequiredFieldOmitted,
    /// The proposed position does not appear in the observation.
    PositionMissing,
    /// The proposed instrument does not appear in the observation.
    InstrumentMissing,
    /// The proposed instrument differs from the observed position instrument.
    PositionInstrumentMismatch,
    /// The reduction exceeds the observed open position quantity.
    QuantityExceedsPosition,
    /// The reduction does not align with the observed quantity increment.
    QuantityIncrementMismatch,
}

fn evaluate_reduction(
    observation: &Observation,
    reduction: &ReducePosition,
    findings: &mut Vec<AdvisoryFinding>,
) {
    let crate::protocol::observation::ObservationPayload::Live(payload) = &observation.payload;
    let position = payload
        .positions
        .iter()
        .find(|position| position.position_id == reduction.position_id);
    let Some(position) = position else {
        findings.push(finding(
            AdvisoryCode::PositionMissing,
            "/proposal/intent/reduce_position/position_id",
            "the proposed position does not appear in the observation",
        ));
        return;
    };

    if position.instrument_id != reduction.instrument_id {
        findings.push(finding(
            AdvisoryCode::PositionInstrumentMismatch,
            "/proposal/intent/reduce_position/instrument_id",
            "the proposed instrument differs from the observed position instrument",
        ));
    }

    let instrument = payload
        .instruments
        .iter()
        .find(|instrument| instrument.instrument_id == reduction.instrument_id);
    let Some(instrument) = instrument else {
        findings.push(finding(
            AdvisoryCode::InstrumentMissing,
            "/proposal/intent/reduce_position/instrument_id",
            "the proposed instrument does not appear in the observation",
        ));
        return;
    };

    if reduction.quantity > position.quantity {
        findings.push(finding(
            AdvisoryCode::QuantityExceedsPosition,
            "/proposal/intent/reduce_position/quantity",
            "the reduction exceeds the observed open position quantity",
        ));
    }

    if !reduction
        .quantity
        .is_multiple_of(&instrument.quantity_increment)
    {
        findings.push(finding(
            AdvisoryCode::QuantityIncrementMismatch,
            "/proposal/intent/reduce_position/quantity",
            "the reduction does not align with the observed quantity increment",
        ));
    }
}

fn finding(code: AdvisoryCode, field: &str, message: &str) -> AdvisoryFinding {
    AdvisoryFinding {
        code,
        severity: AdvisorySeverity::Error,
        field: FieldPath::parse(field).expect("static advisory field path must be valid"),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::protocol::{
        live::{LiveProposalRequest, ReducePosition},
        observation::{FieldOmission, OmissionReason, RetentionClass},
        value::{InstrumentId, PositionId, Quantity},
        version::ProtocolVersion,
    };

    #[rstest]
    fn test_clear_report() {
        let (observation, proposal) = case();
        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);
        assert_eq!(report, AdvisoryReport { findings: vec![] });
        assert!(report.is_clear());
    }

    #[rstest]
    fn test_version_and_expiry_findings() {
        let (mut observation, proposal) = case();
        observation.version = ProtocolVersion::new(2, 7);
        observation.refresh_digest().unwrap();
        let now = TimestampNs::new(observation.expires_at.as_u64() + 1);

        let report = AdvisoryValidator.evaluate(&observation, &proposal, now);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![
                    finding(
                        AdvisoryCode::UnsupportedVersion,
                        "/version",
                        "the observation protocol version is unsupported",
                    ),
                    finding(
                        AdvisoryCode::ExpiredObservation,
                        "/expires_at",
                        "the observation has expired",
                    ),
                ],
            }
        );
    }

    #[rstest]
    fn test_digest_finding() {
        let (mut observation, proposal) = case();
        observation.retention = RetentionClass::ReferenceOnly;

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![finding(
                    AdvisoryCode::DigestMismatch,
                    "/digest",
                    "the observation digest does not match its content",
                )],
            }
        );
    }

    #[rstest]
    fn test_omission_finding() {
        let (mut observation, proposal) = case();
        observation.omissions.push(FieldOmission {
            field: FieldPath::parse("/payload/live/positions/0/quantity").unwrap(),
            reason: OmissionReason::Redacted,
        });
        observation.refresh_digest().unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![AdvisoryFinding {
                    code: AdvisoryCode::RequiredFieldOmitted,
                    severity: AdvisorySeverity::Error,
                    field: FieldPath::parse("/payload/live/positions/0/quantity").unwrap(),
                    message: "the observation omits data needed to check the proposal".to_owned(),
                }],
            }
        );
    }

    #[rstest]
    fn test_nonessential_omission_remains_clear() {
        let (mut observation, proposal) = case();
        observation.omissions.push(FieldOmission {
            field: FieldPath::parse("/payload/live/positions/0/unrealized_pnl").unwrap(),
            reason: OmissionReason::Unsupported,
        });
        observation.refresh_digest().unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(report, AdvisoryReport { findings: vec![] });
        assert!(report.is_clear());
    }

    #[rstest]
    fn test_position_identity_finding() {
        let (observation, mut proposal) = case();
        let LiveIntent::ReducePosition(reduction) = &mut proposal.intent;
        reduction.position_id = PositionId::parse("P-404").unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![finding(
                    AdvisoryCode::PositionMissing,
                    "/proposal/intent/reduce_position/position_id",
                    "the proposed position does not appear in the observation",
                )],
            }
        );
    }

    #[rstest]
    fn test_instrument_identity_findings() {
        let (observation, mut proposal) = case();
        let LiveIntent::ReducePosition(reduction) = &mut proposal.intent;
        reduction.instrument_id = InstrumentId::parse("ETHUSDT.BINANCE").unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![
                    finding(
                        AdvisoryCode::PositionInstrumentMismatch,
                        "/proposal/intent/reduce_position/instrument_id",
                        "the proposed instrument differs from the observed position instrument",
                    ),
                    finding(
                        AdvisoryCode::InstrumentMissing,
                        "/proposal/intent/reduce_position/instrument_id",
                        "the proposed instrument does not appear in the observation",
                    ),
                ],
            }
        );
    }

    #[rstest]
    fn test_missing_instrument_view_finding() {
        let (mut observation, proposal) = case();
        let crate::protocol::observation::ObservationPayload::Live(live) = &mut observation.payload;
        live.instruments.clear();
        observation.refresh_digest().unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![finding(
                    AdvisoryCode::InstrumentMissing,
                    "/proposal/intent/reduce_position/instrument_id",
                    "the proposed instrument does not appear in the observation",
                )],
            }
        );
    }

    #[rstest]
    fn test_quantity_findings() {
        let (observation, mut proposal) = case();
        let LiveIntent::ReducePosition(reduction) = &mut proposal.intent;
        reduction.quantity = Quantity::parse("3").unwrap();

        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);

        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![finding(
                    AdvisoryCode::QuantityExceedsPosition,
                    "/proposal/intent/reduce_position/quantity",
                    "the reduction exceeds the observed open position quantity",
                )],
            }
        );

        let LiveIntent::ReducePosition(reduction) = &mut proposal.intent;
        *reduction = ReducePosition {
            position_id: PositionId::parse("P-197").unwrap(),
            instrument_id: InstrumentId::parse("BTCUSDT.BINANCE").unwrap(),
            quantity: Quantity::parse("1.1").unwrap(),
        };
        let report = AdvisoryValidator.evaluate(&observation, &proposal, observation.created_at);
        assert_eq!(
            report,
            AdvisoryReport {
                findings: vec![finding(
                    AdvisoryCode::QuantityIncrementMismatch,
                    "/proposal/intent/reduce_position/quantity",
                    "the reduction does not align with the observed quantity increment",
                )],
            }
        );
    }

    fn case() -> (Observation, LiveProposal) {
        let observation = serde_json::from_slice(include_bytes!(
            "../../contract/v1/fixtures/valid/full-live-observation.json"
        ))
        .unwrap();
        let request: LiveProposalRequest = serde_json::from_slice(include_bytes!(
            "../../contract/v1/fixtures/valid/reduce-position-request.json"
        ))
        .unwrap();
        (observation, request.proposal)
    }
}
