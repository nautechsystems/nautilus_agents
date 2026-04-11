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
//! Every `Execute` decision produces an envelope, even when capability
//! checks, guardrails, or lowering fail. The canonical record has no
//! gaps. Policy failure is the only thing that prevents envelope
//! creation.

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
    policy::{AgentPolicy, PlannedIntent, PolicyDecision, PolicyError},
};

/// Policy failure is the only thing that prevents envelope creation.
/// Capability denials, guardrail rejections, and lowering failures
/// are all recorded in the envelope so the canonical record has no
/// gaps.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Policy(#[from] PolicyError),
}

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
    /// Every `Execute` decision produces a [`DecisionEnvelope`] with
    /// a [`PlannedIntentOutcome`], even when capability checks,
    /// guardrails, or lowering fail. Returns [`PipelineError::Policy`]
    /// only when the policy itself cannot produce a decision.
    pub async fn run(
        &self,
        trigger: DecisionTrigger,
        context: AgentContext,
    ) -> Result<DecisionEnvelope, PipelineError> {
        let ts_created = context.ts_context;
        let decision = self.policy.evaluate(&context).await?;

        let outcome = match &decision {
            PolicyDecision::NoAction => None,
            PolicyDecision::Execute(planned_intent) => {
                Some(self.evaluate_planned_intent(planned_intent, &context, ts_created))
            }
        };

        Ok(DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger,
            context,
            decision,
            outcome,
            reconciliation: None,
            ts_created,
            ts_reconciled: None,
        })
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
