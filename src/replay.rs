// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Replay engine for recorded decision envelopes.
//!
//! Reads JSONL files produced by [`DecisionRecorder`](crate::recording::DecisionRecorder),
//! re-evaluates them through a [`DecisionPipeline`], and compares original
//! decisions against replayed outcomes.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::action::RuntimeAction;
use crate::envelope::{
    DecisionEnvelope, ENVELOPE_SCHEMA_VERSION, GuardrailResult, LoweringOutcome,
};
use crate::intent::AgentIntent;
use crate::pipeline::{DecisionPipeline, PipelineError};
use crate::policy::PolicyDecision;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("I/O error: {message}")]
    Io { message: String },
    #[error("malformed JSON on line {line}: {message}")]
    MalformedLine { line: usize, message: String },
    #[error("unsupported schema version {version} on line {line} (expected {expected})")]
    UnsupportedSchema {
        line: usize,
        version: u32,
        expected: u32,
    },
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
}

/// Read a JSONL file into a vector of decision envelopes.
///
/// Returns an error with the line number for any line that fails to
/// deserialize.
pub fn read_envelopes(path: &Path) -> Result<Vec<DecisionEnvelope>, ReplayError> {
    let content = fs::read_to_string(path).map_err(|e| ReplayError::Io {
        message: e.to_string(),
    })?;

    let mut envelopes = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;
        let envelope: DecisionEnvelope =
            serde_json::from_str(line).map_err(|e| ReplayError::MalformedLine {
                line: line_num,
                message: e.to_string(),
            })?;

        if envelope.schema_version != ENVELOPE_SCHEMA_VERSION {
            return Err(ReplayError::UnsupportedSchema {
                line: line_num,
                version: envelope.schema_version,
                expected: ENVELOPE_SCHEMA_VERSION,
            });
        }
        envelopes.push(envelope);
    }
    Ok(envelopes)
}

/// Controls which envelopes the runner re-evaluates.
#[derive(Clone, Debug, Default)]
pub struct ReplayConfig {
    pub skip_no_action: bool,
}

/// Pairs a recorded envelope with its replayed counterpart.
pub struct ReplayResult {
    pub original: DecisionEnvelope,
    pub replayed: DecisionEnvelope,
}

impl ReplayResult {
    /// Returns `true` when the replayed outcome differs from the original.
    ///
    /// Compares the policy decision (including intent content), intent
    /// guardrail result, lowering outcome, lowered action variant, and
    /// action guardrail result. Trade action payloads are not compared
    /// because `lower_intent` generates fresh UUIDs each run; only the
    /// variant discriminant (SubmitOrder vs CancelOrder, etc.) is checked.
    pub fn decision_changed(&self) -> bool {
        !decisions_match(&self.original.decision, &self.replayed.decision)
            || !guardrails_match(
                &self.original.intent_guardrail,
                &self.replayed.intent_guardrail,
            )
            || !lowering_outcomes_match(
                &self.original.lowering_result,
                &self.replayed.lowering_result,
            )
            || !lowered_actions_match(&self.original.lowered_action, &self.replayed.lowered_action)
            || !guardrails_match(
                &self.original.action_guardrail,
                &self.replayed.action_guardrail,
            )
    }

    /// One-line summary of the comparison.
    pub fn summary(&self) -> String {
        let id = &self.original.envelope_id;

        if !decisions_match(&self.original.decision, &self.replayed.decision) {
            let from = decision_detail(&self.original.decision);
            let to = decision_detail(&self.replayed.decision);
            if from == to {
                return format!("envelope {id}: decision parameters changed within {from}");
            }
            return format!("envelope {id}: decision changed from {from} to {to}");
        }

        if !guardrails_match(
            &self.original.intent_guardrail,
            &self.replayed.intent_guardrail,
        ) {
            let from = guardrail_label(&self.original.intent_guardrail);
            let to = guardrail_label(&self.replayed.intent_guardrail);
            return format!("envelope {id}: intent guardrail changed from {from} to {to}");
        }

        if !lowering_outcomes_match(
            &self.original.lowering_result,
            &self.replayed.lowering_result,
        ) {
            let from = lowering_label(&self.original.lowering_result);
            let to = lowering_label(&self.replayed.lowering_result);
            return format!("envelope {id}: lowering changed from {from} to {to}");
        }

        if !lowered_actions_match(&self.original.lowered_action, &self.replayed.lowered_action) {
            let from = action_label(&self.original.lowered_action);
            let to = action_label(&self.replayed.lowered_action);
            return format!("envelope {id}: lowered action changed from {from} to {to}");
        }

        if !guardrails_match(
            &self.original.action_guardrail,
            &self.replayed.action_guardrail,
        ) {
            let from = guardrail_label(&self.original.action_guardrail);
            let to = guardrail_label(&self.replayed.action_guardrail);
            return format!("envelope {id}: action guardrail changed from {from} to {to}");
        }

        let label = decision_detail(&self.original.decision);
        format!("envelope {id}: outcome unchanged ({label})")
    }
}

impl fmt::Display for ReplayResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Re-evaluates recorded envelopes through a pipeline.
pub struct ReplayRunner {
    pipeline: DecisionPipeline,
    config: ReplayConfig,
}

impl ReplayRunner {
    pub fn new(pipeline: DecisionPipeline, config: ReplayConfig) -> Self {
        Self { pipeline, config }
    }

    /// Re-evaluate all envelopes, returning a `ReplayResult` per envelope.
    pub fn run(&self, envelopes: Vec<DecisionEnvelope>) -> Result<Vec<ReplayResult>, ReplayError> {
        let mut results = Vec::new();
        for envelope in envelopes {
            if self.config.skip_no_action && matches!(envelope.decision, PolicyDecision::NoAction) {
                continue;
            }
            let replayed = self
                .pipeline
                .run(envelope.trigger.clone(), envelope.context.clone())?;
            results.push(ReplayResult {
                original: envelope,
                replayed,
            });
        }
        Ok(results)
    }
}

/// Compares lowered actions by variant discriminant. UUID fields
/// inside trade payloads differ between runs, so payload comparison
/// is skipped. Variant-level comparison still catches changes like
/// SubmitOrder to CancelOrder or RunBacktest to CompareBacktests.
fn lowered_actions_match(a: &Option<RuntimeAction>, b: &Option<RuntimeAction>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(RuntimeAction::Research(a)), Some(RuntimeAction::Research(b))) => {
            std::mem::discriminant(a) == std::mem::discriminant(b)
        }
        (Some(RuntimeAction::Trade(a)), Some(RuntimeAction::Trade(b))) => {
            std::mem::discriminant(a.as_ref()) == std::mem::discriminant(b.as_ref())
        }
        _ => false,
    }
}

fn action_label(action: &Option<RuntimeAction>) -> &'static str {
    match action {
        None => "none",
        Some(RuntimeAction::Trade(_)) => "Trade",
        Some(RuntimeAction::Research(_)) => "Research",
    }
}

fn lowering_outcomes_match(a: &Option<LoweringOutcome>, b: &Option<LoweringOutcome>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(LoweringOutcome::Success), Some(LoweringOutcome::Success)) => true,
        (
            Some(LoweringOutcome::Failed { reason: a }),
            Some(LoweringOutcome::Failed { reason: b }),
        ) => a == b,
        _ => false,
    }
}

fn lowering_label(result: &Option<LoweringOutcome>) -> &'static str {
    match result {
        None => "none",
        Some(LoweringOutcome::Success) => "success",
        Some(LoweringOutcome::Failed { .. }) => "failed",
    }
}

fn guardrails_match(a: &Option<GuardrailResult>, b: &Option<GuardrailResult>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(GuardrailResult::Approved), Some(GuardrailResult::Approved)) => true,
        (
            Some(GuardrailResult::Rejected { reason: a }),
            Some(GuardrailResult::Rejected { reason: b }),
        ) => a == b,
        _ => false,
    }
}

fn decisions_match(a: &PolicyDecision, b: &PolicyDecision) -> bool {
    match (a, b) {
        (PolicyDecision::NoAction, PolicyDecision::NoAction) => true,
        (PolicyDecision::Act(intent_a), PolicyDecision::Act(intent_b)) => intent_a == intent_b,
        _ => false,
    }
}

fn decision_detail(decision: &PolicyDecision) -> String {
    match decision {
        PolicyDecision::NoAction => "NoAction".to_string(),
        PolicyDecision::Act(intent) => format!("Act({})", intent_variant_name(intent)),
    }
}

fn intent_variant_name(intent: &AgentIntent) -> &'static str {
    match intent {
        AgentIntent::ReducePosition { .. } => "ReducePosition",
        AgentIntent::ClosePosition { .. } => "ClosePosition",
        AgentIntent::CancelOrder { .. } => "CancelOrder",
        AgentIntent::CancelAllOrders { .. } => "CancelAllOrders",
        AgentIntent::PauseStrategy { .. } => "PauseStrategy",
        AgentIntent::ResumeStrategy { .. } => "ResumeStrategy",
        AgentIntent::AdjustRiskLimits { .. } => "AdjustRiskLimits",
        AgentIntent::EscalateToHuman { .. } => "EscalateToHuman",
        AgentIntent::RunBacktest => "RunBacktest",
        AgentIntent::AbortBacktest => "AbortBacktest",
        AgentIntent::AdjustParameters => "AdjustParameters",
        AgentIntent::CompareResults => "CompareResults",
        AgentIntent::SaveCandidate => "SaveCandidate",
        AgentIntent::RejectHypothesis => "RejectHypothesis",
    }
}

fn guardrail_label(result: &Option<GuardrailResult>) -> &'static str {
    match result {
        None => "none",
        Some(GuardrailResult::Approved) => "approved",
        Some(GuardrailResult::Rejected { .. }) => "rejected",
    }
}
