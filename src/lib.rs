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

//! Agent protocol for [NautilusTrader](https://nautilustrader.io).
//!
//! This crate provides the types, traits, and contracts for building
//! autonomous trading agents on top of the Nautilus trading engine.
//!
//! **Status**: early development. The API is not yet stable.

pub mod action;
pub mod capability;
pub mod context;
pub mod envelope;
pub mod intent;
pub mod policy;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod prelude {
    pub use nautilus_core::{UUID4, UnixNanos};
    pub use nautilus_model::{
        data::{Bar, QuoteTick},
        enums::{OrderSide, OrderType, PositionSide},
        events::{AccountState, OrderSnapshot, PositionSnapshot},
        identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId},
        reports::PositionStatusReport,
        types::{Currency, Money, Price, Quantity},
    };

    pub use crate::{
        action::{ResearchCommand, RuntimeAction, TradeAction},
        capability::{ActionCapability, CapabilitySet, ObservationCapability},
        context::AgentContext,
        envelope::{
            DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
            ReconciliationOutcome,
        },
        intent::{AgentIntent, EscalationSeverity, ExecutionConstraints},
        policy::{AgentPolicy, PolicyDecision, PolicyError},
    };
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::QuoteTick,
        enums::OrderSide,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use crate::{
        capability::{ActionCapability, CapabilitySet, ObservationCapability},
        context::AgentContext,
        envelope::{
            DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
            ReconciliationOutcome,
        },
        intent::{AgentIntent, ExecutionConstraints},
        policy::PolicyDecision,
    };

    fn test_instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT.BINANCE")
    }

    fn test_capabilities() -> CapabilitySet {
        CapabilitySet {
            observations: BTreeSet::from([
                ObservationCapability::Quotes,
                ObservationCapability::Positions,
                ObservationCapability::AccountState,
            ]),
            actions: BTreeSet::from([ActionCapability::ManagePositions]),
            instrument_scope: BTreeSet::from([test_instrument_id()]),
        }
    }

    fn test_context() -> AgentContext {
        AgentContext {
            ts_context: UnixNanos::from(1_712_400_000_000_000_000u64),
            capabilities: test_capabilities(),
            quotes: vec![QuoteTick::new(
                test_instrument_id(),
                Price::from("68450.00"),
                Price::from("68451.00"),
                Quantity::from("2.5"),
                Quantity::from("1.8"),
                UnixNanos::from(1_712_399_999_500_000_000u64),
                UnixNanos::from(1_712_399_999_600_000_000u64),
            )],
            bars: vec![],
            account_state: None,
            positions: vec![],
            orders: vec![],
            position_reports: vec![],
        }
    }

    fn test_intent() -> AgentIntent {
        AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints {
                max_slippage_pct: Some(0.001),
                reduce_only: true,
                ..Default::default()
            },
        }
    }

    #[rstest]
    fn test_capability_check_intent_approved() {
        let caps = test_capabilities();
        let intent = test_intent();
        assert!(caps.check_intent(&intent).is_ok());
    }

    #[rstest]
    fn test_capability_check_intent_action_denied() {
        let caps = CapabilitySet {
            observations: BTreeSet::new(),
            actions: BTreeSet::new(),
            instrument_scope: BTreeSet::from([test_instrument_id()]),
        };
        let intent = test_intent();
        assert!(caps.check_intent(&intent).is_err());
    }

    #[rstest]
    fn test_capability_check_intent_instrument_denied() {
        let caps = CapabilitySet {
            observations: BTreeSet::new(),
            actions: BTreeSet::from([ActionCapability::ManagePositions]),
            instrument_scope: BTreeSet::new(),
        };
        let intent = test_intent();
        assert!(caps.check_intent(&intent).is_err());
    }

    #[rstest]
    fn test_policy_decision_round_trip() {
        let decision = PolicyDecision::Act(test_intent());
        let json = serde_json::to_string(&decision).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();

        match restored {
            PolicyDecision::Act(AgentIntent::ReducePosition {
                instrument_id,
                quantity,
                constraints,
            }) => {
                assert_eq!(instrument_id, test_instrument_id());
                assert_eq!(quantity, Quantity::from("0.5"));
                assert_eq!(constraints.max_slippage_pct, Some(0.001));
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
    fn test_agent_context_round_trip() {
        let ctx = test_context();
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: AgentContext = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.quotes.len(), 1);
        assert_eq!(restored.quotes[0].instrument_id, test_instrument_id());
    }

    #[rstest]
    fn test_decision_envelope_round_trip() {
        let envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::MarketData {
                instrument_id: test_instrument_id(),
            },
            context: test_context(),
            decision: PolicyDecision::Act(test_intent()),
            intent_guardrail: Some(GuardrailResult::Approved),
            lowered_action: None,
            action_guardrail: None,
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
        assert!(matches!(restored.decision, PolicyDecision::Act(_)));
        assert!(matches!(
            restored.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            restored.reconciliation,
            Some(ReconciliationOutcome::Filled { .. })
        ));
    }

    #[rstest]
    fn test_no_action_envelope_has_no_downstream_fields() {
        let envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            intent_guardrail: None,
            lowered_action: None,
            action_guardrail: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let json = serde_json::to_string(&envelope).unwrap();
        let restored: DecisionEnvelope = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored.decision, PolicyDecision::NoAction));
        assert!(restored.intent_guardrail.is_none());
        assert!(restored.lowered_action.is_none());
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
    fn test_escalate_to_human_round_trip() {
        use crate::intent::EscalationSeverity;

        let intent = AgentIntent::EscalateToHuman {
            reason: "drawdown limit approaching".to_string(),
            severity: EscalationSeverity::Warning,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        match restored {
            AgentIntent::EscalateToHuman { reason, severity } => {
                assert_eq!(reason, "drawdown limit approaching");
                assert_eq!(severity, EscalationSeverity::Warning);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_cancel_all_orders_round_trip() {
        let intent = AgentIntent::CancelAllOrders {
            instrument_id: test_instrument_id(),
            order_side: OrderSide::Buy,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        match restored {
            AgentIntent::CancelAllOrders {
                instrument_id,
                order_side,
            } => {
                assert_eq!(instrument_id, test_instrument_id());
                assert_eq!(order_side, OrderSide::Buy);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_research_stub_round_trip() {
        let intent = AgentIntent::RunBacktest;
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, AgentIntent::RunBacktest));
    }
}
