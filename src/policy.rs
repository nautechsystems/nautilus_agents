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

//! Policy trait and decision types.
//!
//! The [`AgentPolicy`] trait is the interface third parties implement.
//! It receives an [`AgentContext`] and returns a [`PolicyDecision`] or
//! a [`PolicyError`]. Policy failure (panic, timeout) is distinct from
//! a decision to not act.

use std::{future::Future, pin::Pin};

use nautilus_core::UUID4;
use serde::{Deserialize, Serialize};

use crate::{context::AgentContext, intent::AgentIntent};

pub type PolicyFuture<'a> =
    Pin<Box<dyn Future<Output = Result<PolicyDecision, PolicyError>> + Send + 'a>>;

/// Stable correlation wrapper around an [`AgentIntent`].
///
/// `intent_id` travels from the policy output through lowering into
/// command params so the server can reconcile envelope decisions with
/// downstream dispatch and execution results. Two `PlannedIntent`
/// values with different `intent_id`s are not equal; callers that
/// need correlation-agnostic comparison (like replay) handle it
/// explicitly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlannedIntent {
    pub intent_id: UUID4,
    pub intent: AgentIntent,
}

impl PlannedIntent {
    #[must_use]
    pub fn new(intent: AgentIntent) -> Self {
        Self {
            intent_id: UUID4::new(),
            intent,
        }
    }
}

/// Guardrails only evaluate the `Execute` variant.
///
/// `Failed` records a policy error inline in the decision so every
/// cycle produces a self-contained audit record. A `Failed` envelope
/// has `outcome: None` because there is no planned intent to evaluate.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Execute(PlannedIntent),
    NoAction,
    Failed(PolicyError),
}

impl PolicyDecision {
    /// Shorthand for constructing `Execute` from an [`AgentIntent`]
    /// with a fresh `intent_id`.
    #[must_use]
    pub fn execute(intent: AgentIntent) -> Self {
        Self::Execute(PlannedIntent::new(intent))
    }
}

impl From<AgentIntent> for PlannedIntent {
    fn from(intent: AgentIntent) -> Self {
        Self::new(intent)
    }
}

/// Policy failures, not decisions. A timeout or panic is not the same
/// as choosing `NoAction`. Recorded inline on the envelope via
/// [`PolicyDecision::Failed`] so every cycle has a complete audit
/// record.
#[derive(Clone, Debug, PartialEq, thiserror::Error, Serialize, Deserialize)]
pub enum PolicyError {
    #[error("policy timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("policy internal error: {message}")]
    Internal { message: String },
    #[error("insufficient context: {message}")]
    InsufficientContext { message: String },
}

/// Does not dictate how the LLM is called, what model is used, or how
/// prompts are structured. The context is borrowed so remote or
/// LLM-backed policies avoid cloning the full snapshot each cycle.
pub trait AgentPolicy: Send + Sync {
    fn evaluate<'a>(&'a self, context: &'a AgentContext) -> PolicyFuture<'a>;
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::Quantity;
    use rstest::rstest;

    use super::*;
    use crate::{
        fixtures::{execute, planned_intent, test_instrument_id, test_intent},
        intent::AgentIntent,
    };

    #[rstest]
    fn test_policy_decision_round_trip() {
        let decision = execute(test_intent());
        let original_intent_id = planned_intent(&decision).intent_id;
        let json = serde_json::to_string(&decision).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();
        let restored_intent = planned_intent(&restored);

        assert_eq!(restored_intent.intent_id, original_intent_id);

        match &restored_intent.intent {
            AgentIntent::ReducePosition {
                instrument_id,
                quantity,
                constraints,
            } => {
                assert_eq!(*instrument_id, test_instrument_id());
                assert_eq!(*quantity, Quantity::from("0.5"));
                assert!(constraints.reduce_only);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_no_action_round_trip() {
        let decision = PolicyDecision::NoAction;
        let json = serde_json::to_string(&decision).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, PolicyDecision::NoAction));
    }

    #[rstest]
    fn test_policy_decision_execute_constructs_execute_variant() {
        let intent = test_intent();
        match PolicyDecision::execute(intent.clone()) {
            PolicyDecision::Execute(planned) => assert_eq!(planned.intent, intent),
            other => panic!("expected Execute, got {other:?}"),
        }
    }
}
