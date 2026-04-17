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
//! decisions and outcomes against replayed outcomes.

use std::{fmt, fs, path::Path};

use crate::{
    action::{ManagementCommand, ResearchCommand, RuntimeAction},
    envelope::{
        DecisionEnvelope, ENVELOPE_SCHEMA_VERSION, GuardrailResult, LoweringOutcome,
        PlannedIntentOutcome,
    },
    intent::AgentIntent,
    pipeline::DecisionPipeline,
    policy::{PlannedIntent, PolicyDecision},
};

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
    /// Compares the policy decision and the per-intent outcome.
    /// Replay mints a fresh `intent_id` each run, so `PlannedIntent`
    /// equality ignores `intent_id`; only semantic identity matters.
    /// Lowered actions compare semantic payload and ignore
    /// correlation-only fields such as generated UUIDs and
    /// `intent_id`.
    pub fn decision_changed(&self) -> bool {
        !decisions_match(&self.original.decision, &self.replayed.decision)
            || !outcomes_match(&self.original.outcome, &self.replayed.outcome)
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

        match (&self.original.outcome, &self.replayed.outcome) {
            (Some(original), Some(replayed)) => {
                if let Some(detail) = outcome_diff_summary(original, replayed) {
                    return format!("envelope {id}: {detail}");
                }
            }
            (None, None) => {}
            (Some(_), None) => {
                return format!("envelope {id}: outcome present originally, absent on replay");
            }
            (None, Some(_)) => {
                return format!("envelope {id}: outcome absent originally, present on replay");
            }
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
    pub async fn run(
        &self,
        envelopes: Vec<DecisionEnvelope>,
    ) -> Result<Vec<ReplayResult>, ReplayError> {
        let mut results = Vec::new();
        for envelope in envelopes {
            if self.config.skip_no_action && matches!(envelope.decision, PolicyDecision::NoAction) {
                continue;
            }
            let replayed = self
                .pipeline
                .run(envelope.trigger.clone(), envelope.context.clone())
                .await;
            results.push(ReplayResult {
                original: envelope,
                replayed,
            });
        }
        Ok(results)
    }
}

fn outcomes_match(a: &Option<PlannedIntentOutcome>, b: &Option<PlannedIntentOutcome>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(original), Some(replayed)) => outcome_matches(original, replayed),
        _ => false,
    }
}

fn outcome_matches(a: &PlannedIntentOutcome, b: &PlannedIntentOutcome) -> bool {
    guardrails_match(&a.intent_guardrail, &b.intent_guardrail)
        && lowering_outcomes_match(&a.lowering_result, &b.lowering_result)
        && lowered_actions_match(&a.lowered_action, &b.lowered_action)
        && guardrails_match(&a.action_guardrail, &b.action_guardrail)
}

fn outcome_diff_summary(
    original: &PlannedIntentOutcome,
    replayed: &PlannedIntentOutcome,
) -> Option<String> {
    if !guardrails_match(&original.intent_guardrail, &replayed.intent_guardrail) {
        let from = guardrail_label(&original.intent_guardrail);
        let to = guardrail_label(&replayed.intent_guardrail);
        return Some(format!("intent guardrail changed from {from} to {to}"));
    }

    if !lowering_outcomes_match(&original.lowering_result, &replayed.lowering_result) {
        let from = lowering_label(&original.lowering_result);
        let to = lowering_label(&replayed.lowering_result);
        return Some(format!("lowering changed from {from} to {to}"));
    }

    if !lowered_actions_match(&original.lowered_action, &replayed.lowered_action) {
        let from = action_label(&original.lowered_action);
        let to = action_label(&replayed.lowered_action);
        return Some(format!("lowered action changed from {from} to {to}"));
    }

    if !guardrails_match(&original.action_guardrail, &replayed.action_guardrail) {
        let from = guardrail_label(&original.action_guardrail);
        let to = guardrail_label(&replayed.action_guardrail);
        return Some(format!("action guardrail changed from {from} to {to}"));
    }

    None
}

/// Compares lowered actions, ignoring fields that differ between runs
/// by construction. Trade actions use variant-discriminant comparison
/// because `lower_planned_intent` generates fresh UUIDs for the
/// nautilus `command_id` each run. Research and management commands
/// compare every semantic field except `intent_id`, which is
/// correlation metadata minted fresh on each replay.
fn lowered_actions_match(a: &Option<RuntimeAction>, b: &Option<RuntimeAction>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(RuntimeAction::Research(a)), Some(RuntimeAction::Research(b))) => {
            research_commands_match(a, b)
        }
        (Some(RuntimeAction::Management(a)), Some(RuntimeAction::Management(b))) => {
            management_commands_match(a, b)
        }
        (Some(RuntimeAction::Trade(a)), Some(RuntimeAction::Trade(b))) => {
            std::mem::discriminant(a.as_ref()) == std::mem::discriminant(b.as_ref())
        }
        _ => false,
    }
}

fn research_commands_match(a: &ResearchCommand, b: &ResearchCommand) -> bool {
    match (a, b) {
        (
            ResearchCommand::RunBacktest {
                instrument_id: instrument_a,
                catalog_path: catalog_a,
                data_cls: data_cls_a,
                bar_spec: bar_spec_a,
                start_ns: start_a,
                end_ns: end_a,
                baseline_run_id: baseline_a,
                ..
            },
            ResearchCommand::RunBacktest {
                instrument_id: instrument_b,
                catalog_path: catalog_b,
                data_cls: data_cls_b,
                bar_spec: bar_spec_b,
                start_ns: start_b,
                end_ns: end_b,
                baseline_run_id: baseline_b,
                ..
            },
        ) => {
            instrument_a == instrument_b
                && catalog_a == catalog_b
                && data_cls_a == data_cls_b
                && bar_spec_a == bar_spec_b
                && start_a == start_b
                && end_a == end_b
                && baseline_a == baseline_b
        }
        (
            ResearchCommand::CancelBacktest { run_id: run_a, .. },
            ResearchCommand::CancelBacktest { run_id: run_b, .. },
        ) => run_a == run_b,
        (
            ResearchCommand::GetBacktestStatus { run_id: run_a, .. },
            ResearchCommand::GetBacktestStatus { run_id: run_b, .. },
        ) => run_a == run_b,
        (
            ResearchCommand::GetBacktestResult { run_id: run_a, .. },
            ResearchCommand::GetBacktestResult { run_id: run_b, .. },
        ) => run_a == run_b,
        (
            ResearchCommand::CompareBacktests {
                run_ids: run_ids_a, ..
            },
            ResearchCommand::CompareBacktests {
                run_ids: run_ids_b, ..
            },
        ) => run_ids_a == run_ids_b,
        _ => false,
    }
}

fn management_commands_match(a: &ManagementCommand, b: &ManagementCommand) -> bool {
    match (a, b) {
        (
            ManagementCommand::PauseStrategy {
                strategy_id: strategy_a,
                ..
            },
            ManagementCommand::PauseStrategy {
                strategy_id: strategy_b,
                ..
            },
        ) => strategy_a == strategy_b,
        (
            ManagementCommand::ResumeStrategy {
                strategy_id: strategy_a,
                ..
            },
            ManagementCommand::ResumeStrategy {
                strategy_id: strategy_b,
                ..
            },
        ) => strategy_a == strategy_b,
        (
            ManagementCommand::AdjustRiskLimits {
                params: params_a, ..
            },
            ManagementCommand::AdjustRiskLimits {
                params: params_b, ..
            },
        ) => params_a == params_b,
        (
            ManagementCommand::EscalateToHuman {
                reason: reason_a,
                severity: severity_a,
                ..
            },
            ManagementCommand::EscalateToHuman {
                reason: reason_b,
                severity: severity_b,
                ..
            },
        ) => reason_a == reason_b && severity_a == severity_b,
        _ => false,
    }
}

fn action_label(action: &Option<RuntimeAction>) -> String {
    match action {
        None => "none".to_string(),
        Some(RuntimeAction::Trade(_)) => "Trade".to_string(),
        Some(RuntimeAction::Research(cmd)) => {
            let variant = match cmd {
                ResearchCommand::RunBacktest { .. } => "RunBacktest",
                ResearchCommand::CancelBacktest { .. } => "CancelBacktest",
                ResearchCommand::GetBacktestStatus { .. } => "GetBacktestStatus",
                ResearchCommand::GetBacktestResult { .. } => "GetBacktestResult",
                ResearchCommand::CompareBacktests { .. } => "CompareBacktests",
            };
            format!("Research({variant})")
        }
        Some(RuntimeAction::Management(cmd)) => {
            let variant = match cmd {
                ManagementCommand::PauseStrategy { .. } => "PauseStrategy",
                ManagementCommand::ResumeStrategy { .. } => "ResumeStrategy",
                ManagementCommand::AdjustRiskLimits { .. } => "AdjustRiskLimits",
                ManagementCommand::EscalateToHuman { .. } => "EscalateToHuman",
            };
            format!("Management({variant})")
        }
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
        (PolicyDecision::Execute(pa), PolicyDecision::Execute(pb)) => planned_intents_match(pa, pb),
        (PolicyDecision::Failed(ea), PolicyDecision::Failed(eb)) => ea == eb,
        _ => false,
    }
}

fn planned_intents_match(a: &PlannedIntent, b: &PlannedIntent) -> bool {
    a.intent == b.intent
}

fn decision_detail(decision: &PolicyDecision) -> String {
    match decision {
        PolicyDecision::NoAction => "NoAction".to_string(),
        PolicyDecision::Execute(planned_intent) => {
            format!("Execute({})", intent_variant_name(&planned_intent.intent))
        }
        PolicyDecision::Failed(_) => "Failed".to_string(),
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
        AgentIntent::RunBacktest { .. } => "RunBacktest",
        AgentIntent::AbortBacktest { .. } => "AbortBacktest",
        AgentIntent::AdjustParameters { .. } => "AdjustParameters",
        AgentIntent::CompareResults { .. } => "CompareResults",
        AgentIntent::SaveCandidate { .. } => "SaveCandidate",
        AgentIntent::RejectHypothesis { .. } => "RejectHypothesis",
    }
}

fn guardrail_label(result: &Option<GuardrailResult>) -> &'static str {
    match result {
        None => "none",
        Some(GuardrailResult::Approved) => "approved",
        Some(GuardrailResult::Rejected { .. }) => "rejected",
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::identifiers::{StrategyId, TraderId};
    use rstest::rstest;

    use super::*;
    use crate::{
        action::ManagementCommand,
        capability::ActionCapability,
        envelope::{DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION},
        fixtures::{
            ApproveAllActions, ApproveAllIntents, FixedPolicy, FreshPlanPolicy, RejectAllIntents,
            execute, planned_intent, run_pipeline, run_replay, test_context,
            test_context_with_position, test_instrument_id, test_intent, test_lowering_ctx,
        },
        intent::EscalationSeverity,
        lowering::LoweringContext,
        pipeline::DecisionPipeline,
        policy::{PolicyDecision, PolicyError},
        recording::DecisionRecorder,
    };

    fn action_strategy_id(action: &Option<RuntimeAction>) -> Option<StrategyId> {
        match action {
            Some(RuntimeAction::Management(ManagementCommand::PauseStrategy {
                strategy_id,
                ..
            })) => Some(*strategy_id),
            _ => None,
        }
    }

    #[rstest]
    fn test_record_then_read_fidelity() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut recorder = DecisionRecorder::new(&path).unwrap();

        let envelope1 = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let decision = execute(test_intent());
        let intent_id = planned_intent(&decision).intent_id;
        let envelope2 = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::MarketData {
                instrument_id: test_instrument_id(),
            },
            context: test_context(),
            decision,
            outcome: Some(crate::envelope::PlannedIntentOutcome {
                intent_id,
                intent_guardrail: Some(GuardrailResult::Approved),
                lowering_result: None,
                lowered_action: None,
                action_guardrail: None,
            }),
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_001_000_000_000u64),
            ts_reconciled: None,
        };

        recorder.record(&envelope1).unwrap();
        recorder.record(&envelope2).unwrap();

        let restored = read_envelopes(&path).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].envelope_id, envelope1.envelope_id);
        assert_eq!(restored[1].envelope_id, envelope2.envelope_id);
        assert!(matches!(restored[0].decision, PolicyDecision::NoAction));
        assert_eq!(planned_intent(&restored[1].decision).intent, test_intent());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_record_then_read_failed_envelope() {
        let dir = std::env::temp_dir().join(format!("nautilus_failed_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::Failed(PolicyError::Internal {
                message: "downstream LLM rejected request".to_string(),
            }),
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&envelope).unwrap();

        let restored = read_envelopes(&path).unwrap();
        assert_eq!(restored.len(), 1);
        match &restored[0].decision {
            PolicyDecision::Failed(PolicyError::Internal { message }) => {
                assert_eq!(message, "downstream LLM rejected request");
            }
            other => panic!("expected Failed(Internal), got {other:?}"),
        }
        assert!(restored[0].outcome.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_read_envelopes_malformed_line() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_bad_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(&path, "{\"valid\":false}\n{not json\n").unwrap();

        let err = read_envelopes(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("line 1"), "expected line 1 error, got: {msg}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_read_envelopes_missing_file() {
        let path = std::env::temp_dir().join(format!("nonexistent_{}.jsonl", UUID4::new()));
        let err = read_envelopes(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("I/O error"), "expected I/O error, got: {msg}");
    }

    #[rstest]
    fn test_replay_different_policy_changes_decision() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_diff_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let act_policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(act_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let original = run_pipeline(&pipeline, trigger, test_context_with_position());
        assert!(matches!(original.decision, PolicyDecision::Execute(_)));

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let no_action_policy = FixedPolicy(PolicyDecision::NoAction);
        let replay_pipeline =
            DecisionPipeline::new(Box::new(no_action_policy), test_lowering_ctx());
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());
        assert!(results[0].summary().contains("changed"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_ignores_fresh_intent_ids_when_intent_is_unchanged() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_intent_ids_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };

        let original_policy = FreshPlanPolicy(test_intent());
        let original_pipeline =
            DecisionPipeline::new(Box::new(original_policy), test_lowering_ctx())
                .with_intent_guardrail(Box::new(ApproveAllIntents))
                .with_action_guardrail(Box::new(ApproveAllActions));
        let original = run_pipeline(&original_pipeline, trigger, test_context_with_position());

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let replay_policy = FreshPlanPolicy(test_intent());
        let replay_pipeline = DecisionPipeline::new(Box::new(replay_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_stricter_guardrail_rejects() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_guard_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let original = run_pipeline(&pipeline, trigger, test_context_with_position());
        let original_outcome_approved = original.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            original_outcome_approved.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let replay_policy = FixedPolicy(execute(test_intent()));
        let replay_pipeline = DecisionPipeline::new(Box::new(replay_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(RejectAllIntents("stricter limit".to_string())));
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());
        let original_outcome = results[0]
            .original
            .outcome
            .as_ref()
            .expect("expected original outcome");
        let replayed_outcome = results[0]
            .replayed
            .outcome
            .as_ref()
            .expect("expected replayed outcome");
        assert!(matches!(
            original_outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            replayed_outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_skip_no_action() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_skip_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut recorder = DecisionRecorder::new(&path).unwrap();

        let no_action_envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };
        recorder.record(&no_action_envelope).unwrap();

        let envelopes = read_envelopes(&path).unwrap();
        assert_eq!(envelopes.len(), 1);

        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let config = ReplayConfig {
            skip_no_action: true,
        };
        let runner = ReplayRunner::new(pipeline, config);
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_noaction_replayed_as_noaction_is_unchanged() {
        let dir =
            std::env::temp_dir().join(format!("nautilus_replay_noaction_eq_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let no_action_envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };
        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&no_action_envelope).unwrap();

        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_failed_error_change() {
        let build = |error: PolicyError| DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::Failed(error),
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let same = ReplayResult {
            original: build(PolicyError::Timeout { timeout_ms: 100 }),
            replayed: build(PolicyError::Timeout { timeout_ms: 100 }),
        };
        assert!(!same.decision_changed());

        let different = ReplayResult {
            original: build(PolicyError::Timeout { timeout_ms: 100 }),
            replayed: build(PolicyError::Timeout { timeout_ms: 200 }),
        };
        assert!(different.decision_changed());
    }

    #[rstest]
    fn test_replay_detects_research_payload_change() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_research_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let intent_a = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("1-HOUR-BID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let mut ctx = test_context();
        ctx.capabilities.actions.insert(ActionCapability::Research);
        let original = run_pipeline(&pipeline_a, trigger, ctx);

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let intent_b = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("5-MINUTE-MID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_management_pause_strategy_change() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_mgmt_pause_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut ctx = test_context();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageStrategies);

        let lowering_a = LoweringContext {
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let intent_a = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), lowering_a);
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let original = run_pipeline(&pipeline_a, trigger, ctx);

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let lowering_b = LoweringContext {
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("EMACross-002"),
        };
        let intent_b = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-002"),
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), lowering_b);
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        let original_outcome = results[0]
            .original
            .outcome
            .as_ref()
            .expect("expected original outcome");
        let replayed_outcome = results[0]
            .replayed
            .outcome
            .as_ref()
            .expect("expected replayed outcome");
        assert_ne!(
            action_strategy_id(&original_outcome.lowered_action),
            action_strategy_id(&replayed_outcome.lowered_action),
            "fixture must produce different strategy_ids for the assertion to be meaningful",
        );
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_management_escalation_severity_change() {
        let dir =
            std::env::temp_dir().join(format!("nautilus_replay_mgmt_escalate_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut ctx = test_context();
        ctx.capabilities.actions.insert(ActionCapability::Escalate);

        let intent_a = AgentIntent::EscalateToHuman {
            reason: "drawdown approaching limit".to_string(),
            severity: EscalationSeverity::Warning,
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let original = run_pipeline(&pipeline_a, trigger, ctx.clone());

        let mut recorder = DecisionRecorder::new(&path).unwrap();
        recorder.record(&original).unwrap();

        let intent_b = AgentIntent::EscalateToHuman {
            reason: "drawdown approaching limit".to_string(),
            severity: EscalationSeverity::Critical,
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
