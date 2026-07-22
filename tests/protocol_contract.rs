use std::collections::BTreeSet;

use nautilus_agents::protocol::{
    capability::{CapabilityGrant, ObservationCapability, ProposalCapability},
    error::{DecisionError, ErrorCode, ProtocolError},
    identity::{CorrelationId, IdempotencyKey, IntentId, ObservationId, RequestId, TraceId},
    live::{
        InstrumentView, LiveIntent, LiveObservation, LiveProposal, LiveProposalRequest,
        PositionSide, PositionView, ReducePosition,
    },
    observation::{
        FieldOmission, Observation, ObservationError, ObservationPayload, OmissionReason,
        ProvenanceSource, RetentionClass, SourceProvenance,
    },
    receipt::{CorrelationRefs, DecisionReceipt, DecisionStatus, ProposalResponse, ReceiptError},
    value::{ContentDigest, FieldPath, InstrumentId, PositionId, Quantity, TimestampNs},
    version::{Feature, PROTOCOL_VERSION, ProtocolInfo, ProtocolVersion},
};

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().unwrap()
}

fn observation() -> Observation {
    let instrument_id = InstrumentId::new("BTCUSDT.BINANCE").unwrap();
    let mut observation = Observation {
        version: PROTOCOL_VERSION,
        id: parse::<ObservationId>("10000000-0000-4000-8000-000000000001"),
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
                field: FieldPath::new("/payload/live/positions").unwrap(),
                source: ProvenanceSource::PositionState,
                observed_at: TimestampNs::new(1_712_399_999_000_000_404),
                version: Some("position-revision-17".to_string()),
            },
            SourceProvenance {
                field: FieldPath::new("/payload/live/instruments").unwrap(),
                source: ProvenanceSource::InstrumentDefinition,
                observed_at: TimestampNs::new(1_712_399_998_000_000_505),
                version: Some("instrument-revision-23".to_string()),
            },
        ],
        omissions: vec![FieldOmission {
            field: FieldPath::new("/payload/live/positions/0/unrealized_pnl").unwrap(),
            reason: OmissionReason::Unsupported,
        }],
        retention: RetentionClass::Derived,
        payload: ObservationPayload::Live(LiveObservation {
            positions: vec![PositionView {
                position_id: PositionId::new("P-197").unwrap(),
                instrument_id: instrument_id.clone(),
                side: PositionSide::Long,
                quantity: Quantity::new("2.75").unwrap(),
                updated_at: TimestampNs::new(1_712_399_997_000_000_606),
            }],
            instruments: vec![InstrumentView {
                instrument_id,
                quantity_increment: Quantity::new("0.25").unwrap(),
            }],
        }),
        digest: ContentDigest::new([0; 32]),
    };
    observation.refresh_digest().unwrap();
    observation
}

fn request(observation: &Observation) -> LiveProposalRequest {
    LiveProposalRequest {
        version: PROTOCOL_VERSION,
        request_id: parse::<RequestId>("20000000-0000-4000-8000-000000000002"),
        idempotency_key: IdempotencyKey::new("principal-proposal-29").unwrap(),
        observation: observation.reference(),
        agent_trace_id: Some(parse::<TraceId>("30000000-0000-4000-8000-000000000003")),
        proposal: LiveProposal {
            intent: LiveIntent::ReducePosition(ReducePosition {
                position_id: PositionId::new("P-197").unwrap(),
                instrument_id: InstrumentId::new("BTCUSDT.BINANCE").unwrap(),
                quantity: Quantity::new("1.25").unwrap(),
            }),
        },
    }
}

fn receipt(
    observation: &Observation,
    status: DecisionStatus,
    error: Option<DecisionError>,
) -> DecisionReceipt {
    DecisionReceipt {
        version: PROTOCOL_VERSION,
        request_id: parse::<RequestId>("20000000-0000-4000-8000-000000000002"),
        observation: observation.reference(),
        intent_id: parse::<IntentId>("40000000-0000-4000-8000-000000000004"),
        status,
        error,
        correlation: CorrelationRefs {
            engine_correlation_id: Some(parse::<CorrelationId>(
                "50000000-0000-4000-8000-000000000005",
            )),
            engine_causation_id: Some(parse::<CorrelationId>(
                "60000000-0000-4000-8000-000000000006",
            )),
        },
        created_at: TimestampNs::new(1_712_400_001_000_000_707),
        updated_at: TimestampNs::new(1_712_400_002_000_000_808),
    }
}

#[test]
fn test_protocol_whole_value_round_trips_cover_every_field() {
    let observation = observation();
    let request = request(&observation);
    let receipt = receipt(&observation, DecisionStatus::Completed, None);
    let protocol_info = ProtocolInfo {
        version: PROTOCOL_VERSION,
        features: BTreeSet::from([Feature::LiveReducePosition, Feature::DecisionReceipt]),
    };

    let observation_json = serde_json::to_vec(&observation).unwrap();
    let request_json = serde_json::to_vec(&request).unwrap();
    let receipt_json = serde_json::to_vec(&receipt).unwrap();
    let info_json = serde_json::to_vec(&protocol_info).unwrap();

    assert_eq!(
        serde_json::from_slice::<Observation>(&observation_json).unwrap(),
        observation
    );
    assert_eq!(
        serde_json::from_slice::<LiveProposalRequest>(&request_json).unwrap(),
        request
    );
    assert_eq!(
        serde_json::from_slice::<DecisionReceipt>(&receipt_json).unwrap(),
        receipt
    );
    assert_eq!(
        serde_json::from_slice::<ProtocolInfo>(&info_json).unwrap(),
        protocol_info
    );
}

#[test]
fn test_observation_validation_rejects_each_cross_field_violation() {
    let baseline = observation();
    assert_eq!(baseline.validate(), Ok(()));

    let mut expiry = baseline.clone();
    expiry.expires_at = TimestampNs::new(expiry.created_at.as_u64() - 1);
    expiry.refresh_digest().unwrap();
    assert_eq!(
        expiry.validate(),
        Err(ObservationError::ExpiryBeforeCreation)
    );

    let mut grant_expiry = baseline.clone();
    grant_expiry.grant.expires_at = TimestampNs::new(grant_expiry.expires_at.as_u64() - 1);
    grant_expiry.refresh_digest().unwrap();
    assert_eq!(
        grant_expiry.validate(),
        Err(ObservationError::GrantExpiresBeforeObservation)
    );

    let mut position_capability = baseline.clone();
    position_capability
        .grant
        .observations
        .remove(&ObservationCapability::PositionSummary);
    position_capability.refresh_digest().unwrap();
    assert_eq!(
        position_capability.validate(),
        Err(ObservationError::PositionCapabilityMissing)
    );

    let mut instrument_capability = baseline.clone();
    instrument_capability
        .grant
        .observations
        .remove(&ObservationCapability::InstrumentSummary);
    instrument_capability.refresh_digest().unwrap();
    assert_eq!(
        instrument_capability.validate(),
        Err(ObservationError::InstrumentCapabilityMissing)
    );

    let mut out_of_scope = baseline.clone();
    let ObservationPayload::Live(live) = &mut out_of_scope.payload;
    live.positions[0].instrument_id = InstrumentId::new("ETHUSDT.BINANCE").unwrap();
    out_of_scope.refresh_digest().unwrap();
    assert_eq!(
        out_of_scope.validate(),
        Err(ObservationError::InstrumentOutsideGrant {
            instrument_id: InstrumentId::new("ETHUSDT.BINANCE").unwrap(),
        })
    );

    let mut duplicate_position = baseline.clone();
    let ObservationPayload::Live(live) = &mut duplicate_position.payload;
    live.positions.push(live.positions[0].clone());
    duplicate_position.refresh_digest().unwrap();
    assert_eq!(
        duplicate_position.validate(),
        Err(ObservationError::DuplicatePosition {
            position_id: PositionId::new("P-197").unwrap(),
        })
    );

    let mut duplicate_instrument = baseline.clone();
    let ObservationPayload::Live(live) = &mut duplicate_instrument.payload;
    live.instruments.push(live.instruments[0].clone());
    duplicate_instrument.refresh_digest().unwrap();
    assert_eq!(
        duplicate_instrument.validate(),
        Err(ObservationError::DuplicateInstrument {
            instrument_id: InstrumentId::new("BTCUSDT.BINANCE").unwrap(),
        })
    );

    let mut future_position = baseline.clone();
    let ObservationPayload::Live(live) = &mut future_position.payload;
    live.positions[0].updated_at = TimestampNs::new(future_position.created_at.as_u64() + 1);
    future_position.refresh_digest().unwrap();
    assert_eq!(
        future_position.validate(),
        Err(ObservationError::PositionUpdatedAfterCreation {
            position_id: PositionId::new("P-197").unwrap(),
        })
    );

    let mut digest = baseline;
    digest.retention = RetentionClass::Restricted;
    assert!(matches!(
        digest.validate(),
        Err(ObservationError::DigestMismatch { .. })
    ));
}

#[test]
fn test_request_rejects_unknown_and_private_fields() {
    let observation = observation();
    let request = request(&observation);
    let mut value = serde_json::to_value(&request).unwrap();
    for field in [
        "intent_id",
        "lowered_command",
        "guardrail_result",
        "dispatch_claim",
        "authorization_record_id",
    ] {
        value
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), serde_json::json!("private"));
        let error = serde_json::from_value::<LiveProposalRequest>(value.clone()).unwrap_err();
        assert!(
            error.to_string().contains("unknown field"),
            "field: {field}"
        );
        value.as_object_mut().unwrap().remove(field);
    }

    let json = serde_json::to_string(&request).unwrap();
    assert!(!json.contains("intent_id"));
    assert!(!json.contains("order_type"));
    assert!(!json.contains("reduce_only"));
    assert!(!json.contains("client_order_id"));
}

#[test]
fn test_unknown_enum_variant_fails() {
    let error = serde_json::from_str::<Feature>(r#""research""#).unwrap_err();
    assert!(error.to_string().contains("unknown variant `research`"));
}

#[test]
fn test_receipt_status_and_error_contract_for_every_status() {
    let observation = observation();
    let rejected_error = DecisionError {
        code: ErrorCode::Rejected,
        message: "fresh position no longer permits the reduction".to_string(),
        retryable: false,
    };
    let dispatch_error = DecisionError {
        code: ErrorCode::DispatchFailed,
        message: "dispatch result is not yet known".to_string(),
        retryable: true,
    };

    let cases = [
        (DecisionStatus::Accepted, None),
        (DecisionStatus::Rejected, Some(rejected_error)),
        (DecisionStatus::Authorized, None),
        (DecisionStatus::NotDispatched, Some(dispatch_error.clone())),
        (DecisionStatus::Dispatched, None),
        (DecisionStatus::DispatchUnknown, Some(dispatch_error)),
        (DecisionStatus::Completed, None),
    ];
    for (status, error) in cases {
        let receipt = receipt(&observation, status, error);
        assert_eq!(receipt.validate(), Ok(()), "status: {status:?}");
        assert_eq!(
            serde_json::from_slice::<DecisionReceipt>(&serde_json::to_vec(&receipt).unwrap())
                .unwrap(),
            receipt
        );
    }

    let missing = receipt(&observation, DecisionStatus::Rejected, None);
    assert_eq!(
        missing.validate(),
        Err(ReceiptError::MissingError {
            status: DecisionStatus::Rejected,
        })
    );

    let unexpected = receipt(
        &observation,
        DecisionStatus::Accepted,
        Some(DecisionError {
            code: ErrorCode::Rejected,
            message: "not valid for this status".to_string(),
            retryable: false,
        }),
    );
    assert_eq!(
        unexpected.validate(),
        Err(ReceiptError::UnexpectedError {
            status: DecisionStatus::Accepted,
        })
    );

    let incompatible = [
        (DecisionStatus::Rejected, ErrorCode::CommitFailed),
        (DecisionStatus::NotDispatched, ErrorCode::Rejected),
        (DecisionStatus::DispatchUnknown, ErrorCode::CommitFailed),
    ];
    for (status, code) in incompatible {
        let receipt = receipt(
            &observation,
            status,
            Some(DecisionError {
                code,
                message: format!("{code:?} is incompatible with {status:?}"),
                retryable: false,
            }),
        );
        assert_eq!(
            receipt.validate(),
            Err(ReceiptError::ErrorCodeMismatch { status, code })
        );
    }
}

#[test]
fn test_all_error_codes_and_response_variants_round_trip() {
    let codes = [
        ErrorCode::Malformed,
        ErrorCode::UnsupportedVersion,
        ErrorCode::Forbidden,
        ErrorCode::StaleObservation,
        ErrorCode::IndeterminateState,
        ErrorCode::Rejected,
        ErrorCode::IdempotencyConflict,
        ErrorCode::CommitFailed,
        ErrorCode::DispatchFailed,
    ];
    for (index, code) in codes.into_iter().enumerate() {
        let error = ProtocolError {
            request_id: Some(parse::<RequestId>("20000000-0000-4000-8000-000000000002")),
            code,
            message: format!("protocol error case {index}"),
            retryable: index % 2 == 0,
        };
        let response = ProposalResponse::Error(error);
        assert_eq!(
            serde_json::from_slice::<ProposalResponse>(&serde_json::to_vec(&response).unwrap())
                .unwrap(),
            response
        );
    }

    let observation = observation();
    let response =
        ProposalResponse::Receipt(receipt(&observation, DecisionStatus::Completed, None));
    assert_eq!(
        serde_json::from_slice::<ProposalResponse>(&serde_json::to_vec(&response).unwrap())
            .unwrap(),
        response
    );
}

#[test]
fn test_request_version_validation() {
    let observation = observation();
    let mut request = request(&observation);
    assert_eq!(request.validate(), Ok(()));
    request.version = ProtocolVersion::new(2, 0);
    assert!(request.validate().is_err());
}
