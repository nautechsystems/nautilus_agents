use std::time::Duration;

use nautilus_agents::{
    assurance::trace::{PolicyMetadata, TraceOutcome},
    authoring::{
        policy::{ProposalDecision, ProposalFuture, ProposalPolicy},
        runner::{ProposalRunner, RunnerConfig},
    },
    protocol::{observation::Observation, version::PROTOCOL_VERSION},
};

struct NoProposalPolicy;

impl ProposalPolicy for NoProposalPolicy {
    fn propose<'a>(&'a self, _observation: &'a Observation) -> ProposalFuture<'a> {
        Box::pin(async { Ok(ProposalDecision::NoProposal) })
    }
}

#[test]
fn test_public_runner_records_complete_local_result() {
    let observation: Observation = serde_json::from_slice(include_bytes!(
        "../contract/v1/fixtures/valid/full-live-observation.json"
    ))
    .unwrap();
    let policy = PolicyMetadata {
        name: "public-runner-policy".to_owned(),
        version: "2.3.5".to_owned(),
    };
    let runner = ProposalRunner::new(
        NoProposalPolicy,
        RunnerConfig {
            policy: policy.clone(),
            timeout: Duration::from_secs(1),
        },
    );

    let trace = pollster::block_on(runner.run(&observation));

    assert_eq!(trace.version, PROTOCOL_VERSION);
    assert_eq!(trace.observation, observation.reference());
    assert_eq!(trace.policy, policy);
    assert_eq!(trace.outcome, TraceOutcome::NoProposal);
    assert_eq!(trace.trace_id.as_str().len(), 36);
    assert!(trace.completed_at >= trace.started_at);
}
