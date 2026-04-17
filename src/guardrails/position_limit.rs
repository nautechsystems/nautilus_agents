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

//! Position size guardrail.
//!
//! Rejects [`ReducePosition`](crate::intent::AgentIntent::ReducePosition) and
//! [`ClosePosition`](crate::intent::AgentIntent::ClosePosition) intents when
//! the order quantity exceeds a configured maximum.

use nautilus_model::{identifiers::StrategyId, types::Quantity};

use crate::{
    context::AgentContext, envelope::GuardrailResult, guardrail::IntentGuardrail,
    intent::AgentIntent,
};

/// Rejects position-reducing intents whose order quantity exceeds
/// `max_order_quantity`.
///
/// For `ReducePosition`, the check uses the intent's quantity field.
/// For `ClosePosition`, the check looks up the position quantity from
/// context, filtering by both instrument ID and `strategy_id` so the
/// guardrail evaluates the same position that lowering will use.
pub struct PositionLimitGuardrail {
    pub strategy_id: StrategyId,
    pub max_order_quantity: Quantity,
}

impl PositionLimitGuardrail {
    pub fn new(strategy_id: StrategyId, max_order_quantity: Quantity) -> Self {
        Self {
            strategy_id,
            max_order_quantity,
        }
    }
}

impl IntentGuardrail for PositionLimitGuardrail {
    fn evaluate(&self, intent: &AgentIntent, context: &AgentContext) -> GuardrailResult {
        match intent {
            AgentIntent::ReducePosition { quantity, .. } if *quantity > self.max_order_quantity => {
                return GuardrailResult::Rejected {
                    reason: format!(
                        "order quantity {} exceeds max_order_quantity {}",
                        quantity, self.max_order_quantity
                    ),
                };
            }
            AgentIntent::ClosePosition { instrument_id, .. } => {
                let position_qty = context
                    .positions
                    .iter()
                    .find(|p| {
                        p.instrument_id == *instrument_id && p.strategy_id == self.strategy_id
                    })
                    .map(|p| p.quantity);

                if let Some(qty) = position_qty
                    && qty > self.max_order_quantity
                {
                    return GuardrailResult::Rejected {
                        reason: format!(
                            "position quantity {} exceeds max_order_quantity {}",
                            qty, self.max_order_quantity
                        ),
                    };
                }
            }
            _ => {}
        }
        GuardrailResult::Approved
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::{ClientOrderId, PositionId};
    use rstest::rstest;

    use super::*;
    use crate::{
        fixtures::{
            test_context, test_context_with_position, test_instrument_id, test_position_snapshot,
        },
        intent::ExecutionConstraints,
    };

    #[rstest]
    fn test_position_limit_approves_under_limit() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("1.0"));
        let intent = AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints::default(),
        };
        let result = guardrail.evaluate(&intent, &test_context());
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_position_limit_rejects_over_limit() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("0.3"));
        let intent = AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints::default(),
        };
        let result = guardrail.evaluate(&intent, &test_context());
        match result {
            GuardrailResult::Rejected { reason } => {
                assert!(reason.contains("exceeds max_order_quantity"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_position_limit_close_rejects_over_limit() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("1.0"));
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };
        let ctx = test_context_with_position();
        let result = guardrail.evaluate(&intent, &ctx);
        match result {
            GuardrailResult::Rejected { reason } => {
                assert!(reason.contains("exceeds max_order_quantity"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_position_limit_close_approves_under_limit() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("2.0"));
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };
        let ctx = test_context_with_position();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_position_limit_ignores_other_intents() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("0.001"));
        let intent = AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-123"),
        };
        let result = guardrail.evaluate(&intent, &test_context());
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_position_limit_reduce_at_exact_limit_approves() {
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("0.5"));
        let intent = AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints::default(),
        };
        let result = guardrail.evaluate(&intent, &test_context());
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_position_limit_close_filters_by_strategy() {
        let mut ctx = test_context_with_position();
        let mut other_position = test_position_snapshot();
        other_position.strategy_id = StrategyId::new("Other-001");
        other_position.position_id = PositionId::new("P-OTHER");
        other_position.quantity = Quantity::from("5.0");
        other_position.signed_qty = 5.0;
        ctx.positions.push(other_position);

        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("2.0"));
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }
}
