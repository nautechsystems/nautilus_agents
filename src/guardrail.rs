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

//! Guardrail traits for intent and action evaluation.
//!
//! Guardrails run twice per decision cycle. [`IntentGuardrail`] evaluates
//! the [`AgentIntent`](crate::intent::AgentIntent) before lowering (policy
//! violations, position limits, cooldown periods). [`ActionGuardrail`]
//! evaluates the [`RuntimeAction`](crate::action::RuntimeAction) after
//! lowering (venue constraints, compute limits).

use crate::{
    action::RuntimeAction, context::AgentContext, envelope::GuardrailResult, intent::AgentIntent,
};

/// Pre-lowering guardrail. Evaluates a semantic intent against the
/// current context before the intent is lowered to a runtime action.
pub trait IntentGuardrail: Send + Sync {
    fn evaluate(&self, intent: &AgentIntent, context: &AgentContext) -> GuardrailResult;
}

/// Post-lowering guardrail. Evaluates a concrete runtime action against
/// the current context after lowering, before execution.
pub trait ActionGuardrail: Send + Sync {
    fn evaluate(&self, action: &RuntimeAction, context: &AgentContext) -> GuardrailResult;
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::fixtures::{ApproveAllIntents, RejectAllIntents, test_context, test_intent};

    #[rstest]
    fn test_intent_guardrail_approve() {
        let guardrail = ApproveAllIntents;
        let result = guardrail.evaluate(&test_intent(), &test_context());
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_intent_guardrail_reject() {
        let guardrail = RejectAllIntents("position limit exceeded".to_string());
        let result = guardrail.evaluate(&test_intent(), &test_context());
        match result {
            GuardrailResult::Rejected { reason } => {
                assert_eq!(reason, "position limit exceeded");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}
