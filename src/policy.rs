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

use serde::{Deserialize, Serialize};

use crate::context::AgentContext;
use crate::intent::AgentIntent;

/// Guardrails only evaluate the `Act` variant.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PolicyDecision {
    Act(AgentIntent),
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
/// prompts are structured. Takes owned context to support remote and
/// async policy paths.
pub trait AgentPolicy: Send + Sync {
    fn evaluate(&self, context: AgentContext) -> Result<PolicyDecision, PolicyError>;
}
