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

//! Decision envelope: the single canonical record per decision cycle.
//!
//! Separates lowering from reconciliation. Lowering answers "what command
//! do we send?" Reconciliation answers "what did the engine and venue
//! actually do?" Both are recorded in the envelope for replay and audit.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};

use crate::action::RuntimeAction;
use crate::context::AgentContext;
use crate::policy::PolicyDecision;

/// Stays at 1 until the envelope contract is finalized.
pub const ENVELOPE_SCHEMA_VERSION: u32 = 1;

#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DecisionTrigger {
    Timer { interval_ns: u64 },
    MarketData { instrument_id: InstrumentId },
    StateChange { description: String },
    Manual { reason: String },
}

/// Recorded at both intent-level and post-lowering stages.
///
/// Guardrails are pure checks: they approve or reject, never rewrite.
/// If intent rewriting is needed, add a separate pipeline stage.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum GuardrailResult {
    Approved,
    Rejected { reason: String },
}

/// Outcome of the lowering step, distinct from guardrail evaluation.
///
/// Recorded in the envelope so replay and audit can distinguish a
/// lowering failure from an action-guardrail rejection.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum LoweringOutcome {
    Success,
    Failed { reason: String },
}

/// Distinct from the lowered action because venue behavior diverges.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReconciliationOutcome {
    Filled {
        fill_price: Price,
        fill_quantity: Quantity,
    },
    PartialFill {
        fill_price: Price,
        filled_quantity: Quantity,
        remaining_quantity: Quantity,
    },
    Rejected {
        reason: String,
    },
    Timeout {
        elapsed_ns: u64,
    },
    Cancelled {
        reason: String,
    },
    Acknowledged,
    Pending,
}

/// Every field after `decision` is `None` for `NoAction` cycles.
/// A guardrail rejection is a recorded outcome, not a gap in the log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionEnvelope {
    pub envelope_id: UUID4,
    pub schema_version: u32,
    pub trigger: DecisionTrigger,
    pub context: AgentContext,
    pub decision: PolicyDecision,
    pub intent_guardrail: Option<GuardrailResult>,
    pub lowering_result: Option<LoweringOutcome>,
    pub lowered_action: Option<RuntimeAction>,
    pub action_guardrail: Option<GuardrailResult>,
    pub reconciliation: Option<ReconciliationOutcome>,
    pub ts_created: UnixNanos,
    pub ts_reconciled: Option<UnixNanos>,
}
