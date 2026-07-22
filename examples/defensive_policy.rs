//! Evaluate a defensive reduction policy and inspect its local trace and advisory report.

mod support;

use std::time::Duration;

use nautilus_agents::{
    assurance::{advisory::AdvisoryValidator, trace::TraceOutcome},
    authoring::{
        policy::{ProposalDecision, ProposalFuture, ProposalPolicy},
        runner::{ProposalRunner, RunnerConfig},
    },
    protocol::{
        live::{LiveIntent, LiveProposal, ReducePosition},
        observation::{Observation, ObservationPayload},
        value::Quantity,
    },
};

struct DefensivePolicy {
    threshold: Quantity,
    reduction: Quantity,
}

impl ProposalPolicy for DefensivePolicy {
    fn propose<'a>(&'a self, observation: &'a Observation) -> ProposalFuture<'a> {
        Box::pin(async move {
            let ObservationPayload::Live(live) = &observation.payload;
            let Some(position) = live.positions.first() else {
                return Ok(ProposalDecision::NoProposal);
            };
            if position.quantity <= self.threshold {
                return Ok(ProposalDecision::NoProposal);
            }
            Ok(ProposalDecision::Propose(LiveProposal {
                intent: LiveIntent::ReducePosition(ReducePosition {
                    position_id: position.position_id.clone(),
                    instrument_id: position.instrument_id.clone(),
                    quantity: self.reduction.clone(),
                }),
            }))
        })
    }
}

fn main() {
    let observation = support::live_observation();
    let runner = ProposalRunner::new(
        DefensivePolicy {
            threshold: Quantity::parse("3").unwrap(),
            reduction: Quantity::parse("1.25").unwrap(),
        },
        RunnerConfig {
            policy: nautilus_agents::assurance::trace::PolicyMetadata {
                name: "defensive-policy".to_owned(),
                version: "0.1.0-alpha".to_owned(),
            },
            timeout: Duration::from_millis(250),
        },
    );
    let trace = pollster::block_on(runner.run(&observation));

    match &trace.outcome {
        TraceOutcome::Proposed(proposal) => {
            let report = AdvisoryValidator.evaluate(&observation, proposal, observation.created_at);
            println!("trace: {}", trace.trace_id);
            println!("advisory clear: {}", report.is_clear());
        }
        outcome => println!("local outcome: {outcome:?}"),
    }
}
