use std::time::Duration;

use nautilus_agents::{
    assurance::{
        shadow::{ShadowCase, ShadowEvaluator, ShadowResult},
        trace::{AgentFailure, AgentFailureKind, PolicyMetadata, TraceOutcome},
    },
    authoring::{
        policy::{ProposalDecision, ProposalError, ProposalFuture, ProposalPolicy},
        runner::RunnerConfig,
    },
    protocol::{
        live::{LiveIntent, LiveProposal, ReducePosition},
        observation::Observation,
        value::{FieldPath, InstrumentId, PositionId, Quantity},
    },
};

enum ResultPolicy {
    Proposal(LiveProposal),
    NoProposal,
    Error,
    Pending,
    Panic,
}

impl ProposalPolicy for ResultPolicy {
    fn propose<'a>(&'a self, _observation: &'a Observation) -> ProposalFuture<'a> {
        match self {
            Self::Proposal(proposal) => {
                let proposal = proposal.clone();
                Box::pin(async move { Ok(ProposalDecision::Propose(proposal)) })
            }
            Self::NoProposal => Box::pin(async { Ok(ProposalDecision::NoProposal) }),
            Self::Error => Box::pin(async {
                Err(ProposalError::Internal {
                    message: "shadow policy error".to_owned(),
                })
            }),
            Self::Pending => Box::pin(std::future::pending()),
            Self::Panic => Box::pin(async { panic!("shadow policy panic") }),
        }
    }
}

#[test]
fn test_equal_no_proposal_outcomes_match() {
    let result = evaluate(
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
    );
    assert_eq!(
        result,
        ShadowResult {
            case: "recorded-case-17".to_owned(),
            baseline: TraceOutcome::NoProposal,
            candidate: TraceOutcome::NoProposal,
            differing_fields: vec![],
        }
    );
    assert!(result.is_match());
}

#[test]
fn test_proposal_fields_are_compared_individually() {
    let result = evaluate(
        ResultPolicy::Proposal(proposal("P-197", "BTCUSDT.BINANCE", "1.25")),
        Duration::from_secs(1),
        ResultPolicy::Proposal(proposal("P-211", "ETHUSDT.BINANCE", "1.5")),
        Duration::from_secs(1),
    );

    assert_eq!(
        result,
        ShadowResult {
            case: "recorded-case-17".to_owned(),
            baseline: TraceOutcome::Proposed(proposal("P-197", "BTCUSDT.BINANCE", "1.25",)),
            candidate: TraceOutcome::Proposed(proposal("P-211", "ETHUSDT.BINANCE", "1.5",)),
            differing_fields: vec![
                FieldPath::parse("/outcome/proposed/intent/reduce_position/position_id").unwrap(),
                FieldPath::parse("/outcome/proposed/intent/reduce_position/instrument_id").unwrap(),
                FieldPath::parse("/outcome/proposed/intent/reduce_position/quantity").unwrap(),
            ],
        }
    );
    assert!(!result.is_match());
}

#[test]
fn test_outcome_variant_difference() {
    let result = evaluate(
        ResultPolicy::Proposal(proposal("P-197", "BTCUSDT.BINANCE", "1.25")),
        Duration::from_secs(1),
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
    );
    assert_eq!(
        result.differing_fields,
        vec![FieldPath::parse("/outcome").unwrap()]
    );
    assert_eq!(result.candidate, TraceOutcome::NoProposal);
}

#[test]
fn test_policy_error_timeout_and_panic_remain_distinct() {
    let policy_error = evaluate(
        ResultPolicy::Error,
        Duration::from_secs(1),
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
    );
    let timeout = evaluate(
        ResultPolicy::Pending,
        Duration::ZERO,
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
    );
    let panic = evaluate(
        ResultPolicy::Panic,
        Duration::from_secs(1),
        ResultPolicy::NoProposal,
        Duration::from_secs(1),
    );

    assert_eq!(
        policy_error.baseline,
        TraceOutcome::Failed(AgentFailure {
            kind: AgentFailureKind::PolicyError,
            message: "shadow policy error".to_owned(),
        })
    );
    assert_eq!(
        timeout.baseline,
        TraceOutcome::Failed(AgentFailure {
            kind: AgentFailureKind::Timeout,
            message: "policy evaluation timed out after 0 ms".to_owned(),
        })
    );
    assert_eq!(
        panic.baseline,
        TraceOutcome::Failed(AgentFailure {
            kind: AgentFailureKind::Panic,
            message: "shadow policy panic".to_owned(),
        })
    );
    assert_eq!(
        (
            policy_error.differing_fields,
            timeout.differing_fields,
            panic.differing_fields,
        ),
        (
            vec![FieldPath::parse("/outcome").unwrap()],
            vec![FieldPath::parse("/outcome").unwrap()],
            vec![FieldPath::parse("/outcome").unwrap()],
        )
    );
}

#[test]
fn test_failure_fields_are_compared_individually() {
    let result = evaluate(
        ResultPolicy::Error,
        Duration::from_secs(1),
        ResultPolicy::Panic,
        Duration::from_secs(1),
    );

    assert_eq!(
        result.differing_fields,
        vec![
            FieldPath::parse("/outcome/failed/kind").unwrap(),
            FieldPath::parse("/outcome/failed/message").unwrap(),
        ]
    );
    assert_eq!(
        result.baseline,
        TraceOutcome::Failed(AgentFailure {
            kind: AgentFailureKind::PolicyError,
            message: "shadow policy error".to_owned(),
        })
    );
    assert_eq!(
        result.candidate,
        TraceOutcome::Failed(AgentFailure {
            kind: AgentFailureKind::Panic,
            message: "shadow policy panic".to_owned(),
        })
    );
}

fn evaluate(
    baseline: ResultPolicy,
    baseline_timeout: Duration,
    candidate: ResultPolicy,
    candidate_timeout: Duration,
) -> ShadowResult {
    let evaluator = ShadowEvaluator::new(
        baseline,
        config("baseline-policy", "4.1.3", baseline_timeout),
        candidate,
        config("candidate-policy", "5.2.7", candidate_timeout),
    );
    pollster::block_on(evaluator.evaluate(&ShadowCase {
        name: "recorded-case-17".to_owned(),
        observation: observation(),
    }))
}

fn config(name: &str, version: &str, timeout: Duration) -> RunnerConfig {
    RunnerConfig {
        policy: PolicyMetadata {
            name: name.to_owned(),
            version: version.to_owned(),
        },
        timeout,
    }
}

fn proposal(position: &str, instrument: &str, quantity: &str) -> LiveProposal {
    LiveProposal {
        intent: LiveIntent::ReducePosition(ReducePosition {
            position_id: PositionId::parse(position).unwrap(),
            instrument_id: InstrumentId::parse(instrument).unwrap(),
            quantity: Quantity::parse(quantity).unwrap(),
        }),
    }
}

fn observation() -> Observation {
    serde_json::from_slice(include_bytes!(
        "../contract/v1/fixtures/valid/full-live-observation.json"
    ))
    .unwrap()
}
