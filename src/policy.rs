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
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Execute(PlannedIntent),
    NoAction,
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
