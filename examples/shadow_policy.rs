//! Compare two local reduction policies over the same scoped observation.

mod support;

use std::time::Duration;

use nautilus_agents::{
    assurance::{
        shadow::{ShadowCase, ShadowEvaluator},
        trace::PolicyMetadata,
    },
    authoring::{
        policy::{ProposalDecision, ProposalFuture, ProposalPolicy},
        runner::RunnerConfig,
    },
    protocol::{
        live::{LiveIntent, LiveProposal, ReducePosition},
        observation::{Observation, ObservationPayload},
        value::Quantity,
    },
};

struct FixedReductionPolicy {
    quantity: Quantity,
}

impl ProposalPolicy for FixedReductionPolicy {
    fn propose<'a>(&'a self, observation: &'a Observation) -> ProposalFuture<'a> {
        Box::pin(async move {
            let ObservationPayload::Live(live) = &observation.payload;
            let Some(position) = live.positions.first() else {
                return Ok(ProposalDecision::NoProposal);
            };
            Ok(ProposalDecision::Propose(LiveProposal {
                intent: LiveIntent::ReducePosition(ReducePosition {
                    position_id: position.position_id.clone(),
                    instrument_id: position.instrument_id.clone(),
                    quantity: self.quantity.clone(),
                }),
            }))
        })
    }
}

fn main() {
    let evaluator = ShadowEvaluator::new(
        FixedReductionPolicy {
            quantity: Quantity::parse("1.25").unwrap(),
        },
        config("baseline-policy", "0.3.0-alpha"),
        FixedReductionPolicy {
            quantity: Quantity::parse("1.5").unwrap(),
        },
        config("candidate-policy", "0.4.0-alpha"),
    );
    let result = pollster::block_on(evaluator.evaluate(&ShadowCase {
        name: "position-reduction-comparison".to_owned(),
        observation: support::live_observation(),
    }));

    println!("outcomes match: {}", result.is_match());
    println!("differing fields: {:?}", result.differing_fields);
}

fn config(name: &str, version: &str) -> RunnerConfig {
    RunnerConfig {
        policy: PolicyMetadata {
            name: name.to_owned(),
            version: version.to_owned(),
        },
        timeout: Duration::from_millis(250),
    }
}
