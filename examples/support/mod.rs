use std::collections::BTreeSet;

use nautilus_agents::protocol::{
    capability::{CapabilityGrant, ObservationCapability, ProposalCapability},
    identity::ObservationId,
    live::{InstrumentView, LiveObservation, PositionSide, PositionView},
    observation::{
        Observation, ObservationPayload, ProvenanceSource, RetentionClass, SourceProvenance,
    },
    value::{ContentDigest, FieldPath, InstrumentId, PositionId, Quantity, TimestampNs},
    version::PROTOCOL_VERSION,
};

pub fn live_observation() -> Observation {
    let instrument_id = InstrumentId::parse("BTCUSDT.BINANCE").unwrap();
    let mut observation = Observation {
        version: PROTOCOL_VERSION,
        id: ObservationId::parse("71000000-0000-4000-8000-000000000017").unwrap(),
        created_at: TimestampNs::new(1_712_500_000_000_000_101),
        expires_at: TimestampNs::new(1_712_500_030_000_000_202),
        grant: CapabilityGrant {
            observations: BTreeSet::from([
                ObservationCapability::PositionSummary,
                ObservationCapability::InstrumentSummary,
            ]),
            proposals: BTreeSet::from([ProposalCapability::ReducePosition]),
            instruments: BTreeSet::from([instrument_id.clone()]),
            expires_at: TimestampNs::new(1_712_500_060_000_000_303),
        },
        provenance: vec![
            SourceProvenance {
                field: FieldPath::parse("/payload/live/positions").unwrap(),
                source: ProvenanceSource::PositionState,
                observed_at: TimestampNs::new(1_712_499_999_000_000_404),
                version: Some("position-revision-41".to_owned()),
            },
            SourceProvenance {
                field: FieldPath::parse("/payload/live/instruments").unwrap(),
                source: ProvenanceSource::InstrumentDefinition,
                observed_at: TimestampNs::new(1_712_499_998_000_000_505),
                version: Some("instrument-revision-43".to_owned()),
            },
        ],
        omissions: Vec::new(),
        retention: RetentionClass::Derived,
        payload: ObservationPayload::Live(LiveObservation {
            positions: vec![PositionView {
                position_id: PositionId::parse("POSITION-47").unwrap(),
                instrument_id: instrument_id.clone(),
                side: PositionSide::Long,
                quantity: Quantity::parse("3.75").unwrap(),
                updated_at: TimestampNs::new(1_712_499_997_000_000_606),
            }],
            instruments: vec![InstrumentView {
                instrument_id,
                quantity_increment: Quantity::parse("0.25").unwrap(),
            }],
        }),
        digest: ContentDigest::new([0; 32]),
    };
    observation.refresh_digest().unwrap();
    observation.validate().unwrap();
    observation
}
