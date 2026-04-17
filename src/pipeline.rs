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

//! Decision pipeline: the core loop the server calls each cycle.
//!
//! Orchestrates: receive trigger, call policy, and for a planned
//! intent run capability check, intent guardrails, lowering, and
//! action guardrails. Produces a [`DecisionEnvelope`] carrying the
//! decision and the [`PlannedIntentOutcome`] for that intent.
//!
//! Every cycle produces an envelope. Capability denials, guardrail
//! rejections, and lowering failures are recorded in the outcome.
//! Policy failures are recorded as [`PolicyDecision::Failed`] with
//! the error inline. The canonical record has no gaps.

use nautilus_core::{UUID4, UnixNanos};

use crate::{
    action::RuntimeAction,
    context::AgentContext,
    envelope::{
        DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
        LoweringOutcome, PlannedIntentOutcome,
    },
    guardrail::{ActionGuardrail, IntentGuardrail},
    intent::AgentIntent,
    lowering::{LoweringContext, lower_planned_intent},
    policy::{AgentPolicy, PlannedIntent, PolicyDecision},
};

pub struct DecisionPipeline {
    policy: Box<dyn AgentPolicy>,
    intent_guardrails: Vec<Box<dyn IntentGuardrail>>,
    action_guardrails: Vec<Box<dyn ActionGuardrail>>,
    lowering: LoweringContext,
}

impl DecisionPipeline {
    pub fn new(policy: Box<dyn AgentPolicy>, lowering: LoweringContext) -> Self {
        Self {
            policy,
            intent_guardrails: Vec::new(),
            action_guardrails: Vec::new(),
            lowering,
        }
    }

    pub fn with_intent_guardrail(mut self, guardrail: Box<dyn IntentGuardrail>) -> Self {
        self.intent_guardrails.push(guardrail);
        self
    }

    pub fn with_action_guardrail(mut self, guardrail: Box<dyn ActionGuardrail>) -> Self {
        self.action_guardrails.push(guardrail);
        self
    }

    /// Run one decision cycle.
    ///
    /// Every cycle produces a [`DecisionEnvelope`]. `Execute`
    /// decisions carry a [`PlannedIntentOutcome`] even when capability
    /// checks, guardrails, or lowering fail. `Failed` decisions carry
    /// the policy error inline with `outcome: None`. `NoAction`
    /// decisions have `outcome: None`.
    pub async fn run(&self, trigger: DecisionTrigger, context: AgentContext) -> DecisionEnvelope {
        debug_assert!(
            context.validate().is_ok(),
            "agent context populated beyond its capability grant",
        );
        let ts_created = context.ts_context;
        let decision = match self.policy.evaluate(&context).await {
            Ok(decision) => decision,
            Err(e) => PolicyDecision::Failed(e),
        };

        let outcome = match &decision {
            PolicyDecision::Execute(planned_intent) => {
                Some(self.evaluate_planned_intent(planned_intent, &context, ts_created))
            }
            PolicyDecision::NoAction | PolicyDecision::Failed(_) => None,
        };

        DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger,
            context,
            decision,
            outcome,
            reconciliation: None,
            ts_created,
            ts_reconciled: None,
        }
    }

    fn evaluate_planned_intent(
        &self,
        planned_intent: &PlannedIntent,
        context: &AgentContext,
        ts_created: UnixNanos,
    ) -> PlannedIntentOutcome {
        let intent = &planned_intent.intent;

        if let Err(cap_err) = context.capabilities.check_intent(intent) {
            return PlannedIntentOutcome {
                intent_id: planned_intent.intent_id,
                intent_guardrail: Some(GuardrailResult::Rejected {
                    reason: cap_err.to_string(),
                }),
                lowering_result: None,
                lowered_action: None,
                action_guardrail: None,
            };
        }

        let intent_result = self.evaluate_intent_guardrails(intent, context);
        if matches!(intent_result, GuardrailResult::Rejected { .. }) {
            return PlannedIntentOutcome {
                intent_id: planned_intent.intent_id,
                intent_guardrail: Some(intent_result),
                lowering_result: None,
                lowered_action: None,
                action_guardrail: None,
            };
        }

        match lower_planned_intent(planned_intent, context, &self.lowering, ts_created) {
            Ok(action) => {
                let action_result = self.evaluate_action_guardrails(&action, context);
                PlannedIntentOutcome {
                    intent_id: planned_intent.intent_id,
                    intent_guardrail: Some(intent_result),
                    lowering_result: Some(LoweringOutcome::Success),
                    lowered_action: Some(action),
                    action_guardrail: Some(action_result),
                }
            }
            Err(lowering_err) => PlannedIntentOutcome {
                intent_id: planned_intent.intent_id,
                intent_guardrail: Some(intent_result),
                lowering_result: Some(LoweringOutcome::Failed {
                    reason: lowering_err.to_string(),
                }),
                lowered_action: None,
                action_guardrail: None,
            },
        }
    }

    fn evaluate_intent_guardrails(
        &self,
        intent: &AgentIntent,
        context: &AgentContext,
    ) -> GuardrailResult {
        for guardrail in &self.intent_guardrails {
            let result = guardrail.evaluate(intent, context);
            if !matches!(result, GuardrailResult::Approved) {
                return result;
            }
        }
        GuardrailResult::Approved
    }

    fn evaluate_action_guardrails(
        &self,
        action: &RuntimeAction,
        context: &AgentContext,
    ) -> GuardrailResult {
        for guardrail in &self.action_guardrails {
            let result = guardrail.evaluate(action, context);
            if !matches!(result, GuardrailResult::Approved) {
                return result;
            }
        }
        GuardrailResult::Approved
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nautilus_model::identifiers::ClientOrderId;
    use rstest::rstest;

    use super::*;
    use crate::{
        action::ResearchCommand,
        capability::{ActionCapability, CapabilitySet},
        envelope::{ENVELOPE_SCHEMA_VERSION, LoweringOutcome},
        fixtures::{
            ApproveAllActions, ApproveAllIntents, FailingPolicy, FixedPolicy, RejectAllActions,
            RejectAllIntents, execute, run_pipeline, test_context, test_context_with_position,
            test_instrument_id, test_intent, test_lowering_ctx, test_research_context,
            test_run_backtest_intent,
        },
        policy::{PolicyDecision, PolicyError},
    };

    #[rstest]
    fn test_pipeline_no_action() {
        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context());
        assert!(matches!(envelope.decision, PolicyDecision::NoAction));
        assert!(envelope.outcome.is_none());
    }

    #[rstest]
    fn test_pipeline_act_approved() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-123"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        assert!(outcome.lowered_action.is_some());
        assert!(matches!(
            outcome.action_guardrail,
            Some(GuardrailResult::Approved)
        ));
    }

    #[rstest]
    fn test_pipeline_intent_guardrail_rejected() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(RejectAllIntents(
                "exceeds position limit".to_string(),
            )));

        let trigger = DecisionTrigger::Timer {
            interval_ns: 30_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context_with_position());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        match &outcome.intent_guardrail {
            Some(GuardrailResult::Rejected { reason }) => {
                assert_eq!(reason, "exceeds position limit");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
    }

    #[rstest]
    fn test_pipeline_capability_denied_records_rejection() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let ctx = AgentContext {
            capabilities: CapabilitySet {
                observations: BTreeSet::new(),
                actions: BTreeSet::new(),
                instrument_scope: BTreeSet::new(),
            },
            quotes: vec![],
            ..test_context()
        };
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));
        assert!(outcome.lowered_action.is_none());
    }

    #[rstest]
    fn test_pipeline_lowering_failure_records_lowering_result() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents));

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        match &outcome.lowering_result {
            Some(LoweringOutcome::Failed { reason }) => {
                assert!(reason.contains("no position found"));
            }
            other => panic!("expected LoweringOutcome::Failed, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
    }

    #[rstest]
    fn test_pipeline_action_guardrail_rejection_is_recorded() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-777"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(RejectAllActions(
                "venue circuit breaker open".to_string(),
            )));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        assert!(outcome.lowered_action.is_some());
        match &outcome.action_guardrail {
            Some(GuardrailResult::Rejected { reason }) => {
                assert_eq!(reason, "venue circuit breaker open");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_pipeline_policy_error_is_recorded_as_failed_decision() {
        let policy = FailingPolicy(PolicyError::Timeout { timeout_ms: 250 });
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context());
        match &envelope.decision {
            PolicyDecision::Failed(PolicyError::Timeout { timeout_ms }) => {
                assert_eq!(*timeout_ms, 250);
            }
            other => panic!("expected Failed(Timeout), got {other:?}"),
        }
        assert!(envelope.outcome.is_none());
    }

    #[rstest]
    fn test_pipeline_round_trip_serialization() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-456"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        let json = serde_json::to_string(&envelope).unwrap();
        let restored: crate::envelope::DecisionEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.schema_version, ENVELOPE_SCHEMA_VERSION);
        assert!(matches!(restored.decision, PolicyDecision::Execute(_)));
        let outcome = restored.outcome.as_ref().expect("expected outcome");
        assert!(outcome.lowered_action.is_some());
    }

    #[rstest]
    fn test_pipeline_research_denied_without_capability() {
        let policy = FixedPolicy(execute(test_run_backtest_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_context());
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));
        assert!(outcome.lowered_action.is_none());
    }

    #[rstest]
    fn test_pipeline_research_intent_lowers_successfully() {
        let policy = FixedPolicy(execute(test_run_backtest_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_research_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        match &outcome.lowered_action {
            Some(RuntimeAction::Research(ResearchCommand::RunBacktest { .. })) => {}
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_pipeline_workflow_intent_records_lowering_failure() {
        let policy = FixedPolicy(execute(AgentIntent::SaveCandidate {
            run_id: "run-001".to_string(),
            label: "candidate".to_string(),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_research_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        match &outcome.lowering_result {
            Some(LoweringOutcome::Failed { reason }) => {
                assert!(reason.contains("not lowerable"));
            }
            other => panic!("expected LoweringOutcome::Failed, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
    }
}
