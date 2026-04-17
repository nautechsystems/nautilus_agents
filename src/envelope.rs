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

use crate::{action::RuntimeAction, context::AgentContext, policy::PolicyDecision};

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

/// Per-intent evaluation record produced by the decision pipeline.
///
/// Groups the capability, guardrail, and lowering results for a single
/// [`PlannedIntent`](crate::policy::PlannedIntent). `intent_id` mirrors
/// the planned intent's correlation key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedIntentOutcome {
    pub intent_id: UUID4,
    pub intent_guardrail: Option<GuardrailResult>,
    pub lowering_result: Option<LoweringOutcome>,
    pub lowered_action: Option<RuntimeAction>,
    pub action_guardrail: Option<GuardrailResult>,
}

/// `outcome` is `Some` iff `decision` is `Execute`, and `None` for
/// `NoAction` cycles. A guardrail rejection is a recorded outcome,
/// not a gap in the log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionEnvelope {
    pub envelope_id: UUID4,
    pub schema_version: u32,
    pub trigger: DecisionTrigger,
    pub context: AgentContext,
    pub decision: PolicyDecision,
    pub outcome: Option<PlannedIntentOutcome>,
    pub reconciliation: Option<ReconciliationOutcome>,
    pub ts_created: UnixNanos,
    pub ts_reconciled: Option<UnixNanos>,
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::{Price, Quantity};
    use rstest::rstest;

    use super::*;
    use crate::{
        fixtures::{execute, planned_intent, test_context, test_instrument_id, test_intent},
        policy::PolicyDecision,
    };

    #[rstest]
    fn test_decision_envelope_round_trip() {
        let decision = execute(test_intent());
        let intent_id = planned_intent(&decision).intent_id;
        let envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::MarketData {
                instrument_id: test_instrument_id(),
            },
            context: test_context(),
            decision,
            outcome: Some(PlannedIntentOutcome {
                intent_id,
                intent_guardrail: Some(GuardrailResult::Approved),
                lowering_result: Some(LoweringOutcome::Success),
                lowered_action: None,
                action_guardrail: None,
            }),
            reconciliation: Some(ReconciliationOutcome::Filled {
                fill_price: Price::from("68449.50"),
                fill_quantity: Quantity::from("0.5"),
            }),
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: Some(UnixNanos::from(1_712_400_000_500_000_000u64)),
        };

        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let restored: DecisionEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.schema_version, 1);
        assert_eq!(planned_intent(&restored.decision).intent, test_intent());
        let outcome = restored.outcome.as_ref().expect("expected outcome");
        assert_eq!(outcome.intent_id, intent_id);
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            restored.reconciliation,
            Some(ReconciliationOutcome::Filled { .. })
        ));
    }

    #[rstest]
    fn test_no_action_envelope_has_no_outcome() {
        let envelope = DecisionEnvelope {
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

        let json = serde_json::to_string(&envelope).unwrap();
        let restored: DecisionEnvelope = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored.decision, PolicyDecision::NoAction));
        assert!(restored.outcome.is_none());
        assert!(restored.reconciliation.is_none());
    }

    #[rstest]
    fn test_guardrail_rejected_round_trip() {
        let result = GuardrailResult::Rejected {
            reason: "position limit exceeded".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let restored: GuardrailResult = serde_json::from_str(&json).unwrap();
        match restored {
            GuardrailResult::Rejected { reason } => {
                assert_eq!(reason, "position limit exceeded");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_lowering_outcome_round_trip() {
        let success = LoweringOutcome::Success;
        let json = serde_json::to_string(&success).unwrap();
        let restored: LoweringOutcome = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, LoweringOutcome::Success));

        let failed = LoweringOutcome::Failed {
            reason: "no position found".to_string(),
        };
        let json = serde_json::to_string(&failed).unwrap();
        let restored: LoweringOutcome = serde_json::from_str(&json).unwrap();
        match restored {
            LoweringOutcome::Failed { reason } => {
                assert_eq!(reason, "no position found");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
