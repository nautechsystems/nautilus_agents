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

//! Agent intent: the semantic action vocabulary with execution constraints.
//!
//! Each intent variant represents something the agent wants to do, not a
//! raw engine command. Lowering translates intents into
//! [`RuntimeAction`](crate::action::RuntimeAction) variants, filling
//! execution details (order type, time in force, algorithm) within the
//! bounds the constraint block provides.

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientOrderId, InstrumentId, StrategyId},
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};

/// Bounds for lowering when producing a trading command. All fields are
/// optional: the agent specifies only what it cares about.
///
/// v0 enforces `reduce_only` for position-reducing intents and rejects
/// `limit_price`, `target_price`, and `max_slippage_pct` because the
/// default lowering only produces market IOC orders. Setting these
/// fields returns `LoweringError::UnsupportedConstraint`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionConstraints {
    pub target_price: Option<Price>,
    pub limit_price: Option<Price>,
    pub max_slippage_pct: Option<f64>,
    pub reduce_only: bool,
}

/// Does NOT carry trader_id, strategy_id, order_type, time_in_force,
/// exec_algorithm_id, or position_id: lowering fills those.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentIntent {
    ReducePosition {
        instrument_id: InstrumentId,
        quantity: Quantity,
        constraints: ExecutionConstraints,
    },
    ClosePosition {
        instrument_id: InstrumentId,
        constraints: ExecutionConstraints,
    },
    CancelOrder {
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
    },
    CancelAllOrders {
        instrument_id: InstrumentId,
        order_side: OrderSide,
    },
    PauseStrategy {
        strategy_id: StrategyId,
    },
    ResumeStrategy {
        strategy_id: StrategyId,
    },
    AdjustRiskLimits {
        params: serde_json::Value,
    },
    EscalateToHuman {
        reason: String,
        severity: EscalationSeverity,
    },

    // Research mode: lower to ResearchCommand variants.
    // SaveCandidate and RejectHypothesis are workflow intents
    // that record decisions but do not produce runtime actions.
    RunBacktest {
        instrument_id: InstrumentId,
        catalog_path: String,
        data_cls: String,
        bar_spec: Option<String>,
        start_ns: Option<UnixNanos>,
        end_ns: Option<UnixNanos>,
    },
    AbortBacktest {
        run_id: String,
    },
    AdjustParameters {
        baseline_run_id: String,
        instrument_id: InstrumentId,
        catalog_path: String,
        data_cls: String,
        bar_spec: Option<String>,
        start_ns: Option<UnixNanos>,
        end_ns: Option<UnixNanos>,
    },
    CompareResults {
        run_ids: Vec<String>,
    },
    SaveCandidate {
        run_id: String,
        label: String,
    },
    RejectHypothesis {
        run_id: String,
        reason: String,
    },
}

/// Escalation severity for [`AgentIntent::EscalateToHuman`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscalationSeverity {
    Info,
    Warning,
    Critical,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::fixtures::{test_instrument_id, test_run_backtest_intent};

    #[rstest]
    fn test_escalate_to_human_round_trip() {
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
    fn test_research_round_trip() {
        let intent = test_run_backtest_intent();
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, AgentIntent::RunBacktest { .. }));
        assert_eq!(restored, intent);
    }

    #[rstest]
    fn test_abort_backtest_round_trip() {
        let intent = AgentIntent::AbortBacktest {
            run_id: "run-abc".to_string(),
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, intent);
    }

    #[rstest]
    fn test_compare_results_round_trip() {
        let intent = AgentIntent::CompareResults {
            run_ids: vec!["run-001".to_string(), "run-002".to_string()],
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, intent);
    }

    #[rstest]
    fn test_adjust_parameters_round_trip() {
        let intent = AgentIntent::AdjustParameters {
            baseline_run_id: "run-001".to_string(),
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("5-MINUTE-MID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, intent);
    }
}
