//! Side-by-side comparison of local proposal policy outcomes.

use crate::{
    assurance::trace::TraceOutcome,
    authoring::{
        policy::ProposalPolicy,
        runner::{ProposalRunner, RunnerConfig},
    },
    protocol::{live::LiveIntent, observation::Observation, value::FieldPath},
};

/// Evaluates two proposal policies against the same observation.
pub struct ShadowEvaluator<A, B> {
    baseline: ProposalRunner<A>,
    candidate: ProposalRunner<B>,
}

impl<A: ProposalPolicy, B: ProposalPolicy> ShadowEvaluator<A, B> {
    /// Creates a side-by-side local policy evaluator.
    #[must_use]
    pub fn new(
        baseline: A,
        baseline_config: RunnerConfig,
        candidate: B,
        candidate_config: RunnerConfig,
    ) -> Self {
        Self {
            baseline: ProposalRunner::new(baseline, baseline_config),
            candidate: ProposalRunner::new(candidate, candidate_config),
        }
    }

    /// Compares both policy outcomes for one recorded or synthetic case.
    pub async fn evaluate(&self, case: &ShadowCase) -> ShadowResult {
        let baseline = self.baseline.run(&case.observation).await.outcome;
        let candidate = self.candidate.run(&case.observation).await.outcome;
        let differing_fields = differing_fields(&baseline, &candidate);
        ShadowResult {
            case: case.name.clone(),
            baseline,
            candidate,
            differing_fields,
        }
    }
}

/// Names one recorded or synthetic observation used for local comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowCase {
    /// The caller-defined case name.
    pub name: String,
    /// The exact observation supplied to both policies.
    pub observation: Observation,
}

/// Contains both local outcomes and every differing outcome field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowResult {
    /// The caller-defined case name.
    pub case: String,
    /// The baseline policy outcome.
    pub baseline: TraceOutcome,
    /// The candidate policy outcome.
    pub candidate: TraceOutcome,
    /// The public outcome fields that differ.
    pub differing_fields: Vec<FieldPath>,
}

impl ShadowResult {
    /// Returns true when both complete local outcomes match.
    #[must_use]
    pub fn is_match(&self) -> bool {
        self.differing_fields.is_empty()
    }
}

fn differing_fields(baseline: &TraceOutcome, candidate: &TraceOutcome) -> Vec<FieldPath> {
    match (baseline, candidate) {
        (TraceOutcome::NoProposal, TraceOutcome::NoProposal) => Vec::new(),
        (TraceOutcome::Proposed(baseline), TraceOutcome::Proposed(candidate)) => {
            let (LiveIntent::ReducePosition(baseline), LiveIntent::ReducePosition(candidate)) =
                (&baseline.intent, &candidate.intent);
            let mut fields = Vec::new();

            if baseline.position_id != candidate.position_id {
                fields.push(path("/outcome/proposed/intent/reduce_position/position_id"));
            }

            if baseline.instrument_id != candidate.instrument_id {
                fields.push(path(
                    "/outcome/proposed/intent/reduce_position/instrument_id",
                ));
            }

            if baseline.quantity != candidate.quantity {
                fields.push(path("/outcome/proposed/intent/reduce_position/quantity"));
            }
            fields
        }
        (TraceOutcome::Failed(baseline), TraceOutcome::Failed(candidate)) => {
            let mut fields = Vec::new();

            if baseline.kind != candidate.kind {
                fields.push(path("/outcome/failed/kind"));
            }

            if baseline.message != candidate.message {
                fields.push(path("/outcome/failed/message"));
            }
            fields
        }
        (_, _) => vec![path("/outcome")],
    }
}

fn path(value: &str) -> FieldPath {
    FieldPath::parse(value).expect("static shadow field path must be valid")
}
