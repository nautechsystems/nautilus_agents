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

/// Stable correlation wrapper for an intent inside an action plan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedIntent {
    pub intent_id: UUID4,
    pub intent: AgentIntent,
}

/// Semantic equality ignores `intent_id`.
///
/// `intent_id` is a correlation key, not part of the intent meaning.
/// Compare `intent_id` directly when identity matters.
/// Do not derive `Eq` or `Hash` with this equality.
impl PartialEq for PlannedIntent {
    fn eq(&self, other: &Self) -> bool {
        self.intent == other.intent
    }
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

/// Ordered plan of semantic intents emitted by a policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ActionPlanDef")]
pub struct ActionPlan {
    intents: Vec<PlannedIntent>,
}

impl ActionPlan {
    #[must_use]
    pub fn new(intents: Vec<PlannedIntent>) -> Self {
        assert!(
            !intents.is_empty(),
            "action plan must contain at least one planned intent"
        );
        Self { intents }
    }

    #[must_use]
    pub fn single(intent: AgentIntent) -> Self {
        Self::new(vec![PlannedIntent::new(intent)])
    }

    #[must_use]
    pub fn intents(&self) -> &[PlannedIntent] {
        &self.intents
    }

    #[cfg(test)]
    pub(crate) fn new_unchecked(intents: Vec<PlannedIntent>) -> Self {
        Self { intents }
    }
}

/// Guardrails only evaluate the `Execute` variant.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Execute(ActionPlan),
    NoAction,
}

#[derive(Deserialize)]
struct ActionPlanDef {
    intents: Vec<PlannedIntent>,
}

impl TryFrom<ActionPlanDef> for ActionPlan {
    type Error = String;

    fn try_from(value: ActionPlanDef) -> Result<Self, Self::Error> {
        if value.intents.is_empty() {
            return Err("action plan must contain at least one planned intent".to_string());
        }

        Ok(Self {
            intents: value.intents,
        })
    }
}

/// Policy failures, not decisions. A timeout or panic is not the same
/// as choosing `NoAction`.
#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize)]
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
