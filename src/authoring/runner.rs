//! Runtime-neutral local policy evaluation.

use std::{
    any::Any,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_timer::Delay;
use futures_util::{
    FutureExt,
    future::{Either, select},
};

use crate::{
    assurance::trace::{AgentFailure, AgentFailureKind, AgentTrace, PolicyMetadata, TraceOutcome},
    authoring::policy::{ProposalDecision, ProposalPolicy},
    protocol::{
        identity::TraceId, observation::Observation, value::TimestampNs, version::PROTOCOL_VERSION,
    },
};

type TimeoutFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Clock = Box<dyn Fn() -> TimestampNs + Send + Sync>;
type TraceIdFactory = Box<dyn Fn() -> TraceId + Send + Sync>;
type TimeoutFactory = Box<dyn Fn(Duration) -> TimeoutFuture + Send + Sync>;

/// Configures one proposal policy and its local evaluation timeout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerConfig {
    /// The caller-defined policy identity recorded in every trace.
    pub policy: PolicyMetadata,
    /// The maximum duration allowed for one local policy evaluation.
    pub timeout: Duration,
}

/// Evaluates a proposal policy locally and emits one agent-side trace.
pub struct ProposalRunner<P> {
    policy: P,
    config: RunnerConfig,
    runtime: RunnerRuntime,
}

impl<P: ProposalPolicy> ProposalRunner<P> {
    /// Creates a local proposal runner.
    #[must_use]
    pub fn new(policy: P, config: RunnerConfig) -> Self {
        Self {
            policy,
            config,
            runtime: RunnerRuntime::default(),
        }
    }

    /// Evaluates the policy and always returns exactly one agent-side trace.
    pub async fn run(&self, observation: &Observation) -> AgentTrace {
        let trace_id = (self.runtime.trace_id)();
        let started_at = (self.runtime.clock)();
        let outcome = self.evaluate(observation).await;
        let completed_at = (self.runtime.clock)();

        AgentTrace {
            version: PROTOCOL_VERSION,
            trace_id,
            observation: observation.reference(),
            policy: self.config.policy.clone(),
            started_at,
            completed_at,
            outcome,
        }
    }

    async fn evaluate(&self, observation: &Observation) -> TraceOutcome {
        let future = match catch_unwind(AssertUnwindSafe(|| self.policy.propose(observation))) {
            Ok(future) => AssertUnwindSafe(future).catch_unwind(),
            Err(payload) => return panic_outcome(payload.as_ref()),
        };
        let timeout = (self.runtime.timeout)(self.config.timeout);

        match select(Box::pin(future), timeout).await {
            Either::Left((result, _)) => match result {
                Ok(Ok(ProposalDecision::Propose(proposal))) => TraceOutcome::Proposed(proposal),
                Ok(Ok(ProposalDecision::NoProposal)) => TraceOutcome::NoProposal,
                Ok(Err(e)) => TraceOutcome::Failed(AgentFailure {
                    kind: AgentFailureKind::PolicyError,
                    message: e.to_string(),
                }),
                Err(payload) => panic_outcome(payload.as_ref()),
            },
            Either::Right(((), _)) => TraceOutcome::Failed(AgentFailure {
                kind: AgentFailureKind::Timeout,
                message: format!(
                    "policy evaluation timed out after {} ms",
                    self.config.timeout.as_millis()
                ),
            }),
        }
    }

    #[cfg(test)]
    fn with_runtime(policy: P, config: RunnerConfig, runtime: RunnerRuntime) -> Self {
        Self {
            policy,
            config,
            runtime,
        }
    }
}

struct RunnerRuntime {
    clock: Clock,
    trace_id: TraceIdFactory,
    timeout: TimeoutFactory,
}

impl Default for RunnerRuntime {
    fn default() -> Self {
        Self {
            clock: Box::new(system_time),
            trace_id: Box::new(TraceId::new),
            timeout: Box::new(|duration| Box::pin(Delay::new(duration))),
        }
    }
}

fn system_time() -> TimestampNs {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    TimestampNs::new(nanos.try_into().unwrap_or(u64::MAX))
}

fn panic_outcome(payload: &(dyn Any + Send)) -> TraceOutcome {
    let message = payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "policy panicked".to_owned());
    TraceOutcome::Failed(AgentFailure {
        kind: AgentFailureKind::Panic,
        message,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use futures_util::future::{pending, ready};
    use rstest::rstest;

    use super::*;
    use crate::{
        authoring::policy::{ProposalError, ProposalFuture},
        protocol::{
            live::{LiveIntent, LiveProposal, ReducePosition},
            value::{InstrumentId, PositionId, Quantity},
        },
    };

    const TRACE_ID: &str = "12345678-90ab-4cde-8fab-1234567890ab";

    enum PolicyResult {
        Proposal,
        NoProposal,
        Error,
        Pending,
        Panic,
    }

    struct TestPolicy(PolicyResult);

    impl ProposalPolicy for TestPolicy {
        fn propose<'a>(&'a self, _observation: &'a Observation) -> ProposalFuture<'a> {
            match self.0 {
                PolicyResult::Proposal => {
                    Box::pin(async { Ok(ProposalDecision::Propose(proposal())) })
                }
                PolicyResult::NoProposal => Box::pin(async { Ok(ProposalDecision::NoProposal) }),
                PolicyResult::Error => Box::pin(async {
                    Err(ProposalError::Internal {
                        message: "policy input was inconsistent".to_owned(),
                    })
                }),
                PolicyResult::Pending => Box::pin(pending()),
                PolicyResult::Panic => Box::pin(async { panic!("policy future panic") }),
            }
        }
    }

    #[rstest]
    fn test_proposal_produces_exact_trace() {
        assert_trace(
            PolicyResult::Proposal,
            pending_timeout(),
            TraceOutcome::Proposed(proposal()),
        );
    }

    #[rstest]
    fn test_no_proposal_produces_exact_trace() {
        assert_trace(
            PolicyResult::NoProposal,
            pending_timeout(),
            TraceOutcome::NoProposal,
        );
    }

    #[rstest]
    fn test_returned_error_produces_exact_trace() {
        assert_trace(
            PolicyResult::Error,
            pending_timeout(),
            TraceOutcome::Failed(AgentFailure {
                kind: AgentFailureKind::PolicyError,
                message: "policy input was inconsistent".to_owned(),
            }),
        );
    }

    #[rstest]
    fn test_timeout_produces_exact_trace() {
        assert_trace(
            PolicyResult::Pending,
            Box::new(|_| Box::pin(ready(()))),
            TraceOutcome::Failed(AgentFailure {
                kind: AgentFailureKind::Timeout,
                message: "policy evaluation timed out after 25 ms".to_owned(),
            }),
        );
    }

    #[rstest]
    fn test_panic_produces_exact_trace() {
        assert_trace(
            PolicyResult::Panic,
            pending_timeout(),
            TraceOutcome::Failed(AgentFailure {
                kind: AgentFailureKind::Panic,
                message: "policy future panic".to_owned(),
            }),
        );
    }

    fn assert_trace(result: PolicyResult, timeout: TimeoutFactory, outcome: TraceOutcome) {
        let observation = observation();
        let clock = AtomicU64::new(1_700_000_000_000_000_111);
        let runtime = RunnerRuntime {
            clock: Box::new(move || TimestampNs::new(clock.fetch_add(111, Ordering::SeqCst))),
            trace_id: Box::new(|| TraceId::parse(TRACE_ID).unwrap()),
            timeout,
        };
        let config = RunnerConfig {
            policy: PolicyMetadata {
                name: "defensive-reducer".to_owned(),
                version: "7.4.2".to_owned(),
            },
            timeout: Duration::from_millis(25),
        };
        let runner = ProposalRunner::with_runtime(TestPolicy(result), config, runtime);

        let actual = pollster::block_on(runner.run(&observation));

        assert_eq!(
            actual,
            AgentTrace {
                version: PROTOCOL_VERSION,
                trace_id: TraceId::parse(TRACE_ID).unwrap(),
                observation: observation.reference(),
                policy: PolicyMetadata {
                    name: "defensive-reducer".to_owned(),
                    version: "7.4.2".to_owned(),
                },
                started_at: TimestampNs::new(1_700_000_000_000_000_111),
                completed_at: TimestampNs::new(1_700_000_000_000_000_222),
                outcome,
            }
        );
    }

    fn pending_timeout() -> TimeoutFactory {
        Box::new(|_| Box::pin(pending()))
    }

    fn observation() -> Observation {
        serde_json::from_slice(include_bytes!(
            "../../contract/v1/fixtures/valid/full-live-observation.json"
        ))
        .unwrap()
    }

    fn proposal() -> LiveProposal {
        LiveProposal {
            intent: LiveIntent::ReducePosition(ReducePosition {
                position_id: PositionId::parse("position-authoring-17").unwrap(),
                instrument_id: InstrumentId::parse("BTCUSDT-PERP.SIM").unwrap(),
                quantity: Quantity::parse("1.25").unwrap(),
            }),
        }
    }
}
