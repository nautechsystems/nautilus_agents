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
//! Orchestrates: receive trigger, call policy, check capabilities,
//! run intent guardrail, lower intent, run action guardrail, and
//! produce a [`DecisionEnvelope`].
//!
//! Every `Act` decision produces an envelope, even when capability
//! checks or lowering fail. The only failure that prevents an
//! envelope is a [`PolicyError`] (the policy itself failed to
//! produce a decision).

use nautilus_core::UUID4;

use crate::{
    action::RuntimeAction,
    context::AgentContext,
    envelope::{
        DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
        LoweringOutcome,
    },
    guardrail::{ActionGuardrail, IntentGuardrail},
    intent::AgentIntent,
    lowering::{LoweringContext, lower_intent},
    policy::{AgentPolicy, PolicyDecision, PolicyError},
};

/// The only failure mode is a policy error: the policy itself failed
/// to produce a decision. Capability denials and lowering failures
/// are recorded in the envelope so the canonical record has no gaps.
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
    /// Every `Act` decision produces a [`DecisionEnvelope`], including
    /// when capability checks or lowering fail. Returns
    /// [`PipelineError`] only when the policy itself cannot produce a
    /// decision.
    pub fn run(
        &self,
        trigger: DecisionTrigger,
        context: AgentContext,
    ) -> Result<DecisionEnvelope, PipelineError> {
        let ts_created = context.ts_context;
        let decision = self.policy.evaluate(context.clone())?;

        let mut intent_guardrail = None;
        let mut lowering_result = None;
        let mut lowered_action = None;
        let mut action_guardrail = None;

        if let PolicyDecision::Act(ref intent) = decision {
            if let Err(cap_err) = context.capabilities.check_intent(intent) {
                intent_guardrail = Some(GuardrailResult::Rejected {
                    reason: cap_err.to_string(),
                });
            } else {
                let intent_result = self.evaluate_intent_guardrails(intent, &context);
                if matches!(intent_result, GuardrailResult::Rejected { .. }) {
                    intent_guardrail = Some(intent_result);
                } else {
                    match lower_intent(intent, &context, &self.lowering, ts_created) {
                        Ok(action) => {
                            let action_result = self.evaluate_action_guardrails(&action, &context);
                            intent_guardrail = Some(intent_result);
                            lowering_result = Some(LoweringOutcome::Success);
                            lowered_action = Some(action);
                            action_guardrail = Some(action_result);
                        }
                        Err(lowering_err) => {
                            intent_guardrail = Some(intent_result);
                            lowering_result = Some(LoweringOutcome::Failed {
                                reason: lowering_err.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger,
            context,
            decision,
            intent_guardrail,
            lowering_result,
            lowered_action,
            action_guardrail,
            reconciliation: None,
            ts_created,
            ts_reconciled: None,
        })
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
