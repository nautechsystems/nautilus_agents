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

//! Order rate guardrail.
//!
//! Limits how many orders a strategy can submit within a rolling time
//! window. Counts orders in the context snapshot whose `ts_init` falls
//! within the window ending at `ts_context`. Also serves as a cooldown
//! when `max_count` is 1.

use nautilus_model::identifiers::StrategyId;

use crate::{
    context::AgentContext, envelope::GuardrailResult, guardrail::IntentGuardrail,
    intent::AgentIntent,
};

/// Rejects trading intents when the strategy already has
/// `max_count` or more orders initialized within `window_ns`
/// nanoseconds of the context timestamp.
pub struct OrderRateGuardrail {
    pub strategy_id: StrategyId,
    pub max_count: usize,
    pub window_ns: u64,
}

impl OrderRateGuardrail {
    pub fn new(strategy_id: StrategyId, max_count: usize, window_ns: u64) -> Self {
        Self {
            strategy_id,
            max_count,
            window_ns,
        }
    }
}

impl IntentGuardrail for OrderRateGuardrail {
    fn evaluate(&self, intent: &AgentIntent, context: &AgentContext) -> GuardrailResult {
        // Only gate order-producing intents; cancel intents are
        // defensive and must not be blocked during a burst.
        match intent {
            AgentIntent::ReducePosition { .. } | AgentIntent::ClosePosition { .. } => {}
            _ => return GuardrailResult::Approved,
        }

        let cutoff = context.ts_context.as_u64().saturating_sub(self.window_ns);
        let recent_count = context
            .orders
            .iter()
            .filter(|o| o.strategy_id == self.strategy_id && o.ts_init.as_u64() >= cutoff)
            .count();

        if recent_count >= self.max_count {
            return GuardrailResult::Rejected {
                reason: format!(
                    "{recent_count} orders in window exceeds limit of {}",
                    self.max_count,
                ),
            };
        }

        GuardrailResult::Approved
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{enums::OrderSide, identifiers::ClientOrderId};
    use rstest::rstest;

    use super::*;
    use crate::fixtures::{
        test_context, test_instrument_id, test_intent, test_order_snapshot,
        test_run_backtest_intent,
    };

    #[rstest]
    fn test_order_rate_approves_under_limit() {
        let sid = StrategyId::new("EMACross-001");
        let guardrail = OrderRateGuardrail::new(sid, 3, 60_000_000_000);

        let mut ctx = test_context();
        let base_ns = ctx.ts_context.as_u64();
        ctx.orders = vec![
            test_order_snapshot(sid, UnixNanos::from(base_ns - 30_000_000_000)),
            test_order_snapshot(sid, UnixNanos::from(base_ns - 20_000_000_000)),
        ];

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_order_rate_rejects_at_limit() {
        let sid = StrategyId::new("EMACross-001");
        let guardrail = OrderRateGuardrail::new(sid, 2, 60_000_000_000);

        let mut ctx = test_context();
        let base_ns = ctx.ts_context.as_u64();
        ctx.orders = vec![
            test_order_snapshot(sid, UnixNanos::from(base_ns - 30_000_000_000)),
            test_order_snapshot(sid, UnixNanos::from(base_ns - 20_000_000_000)),
        ];

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        match result {
            GuardrailResult::Rejected { reason } => {
                assert!(reason.contains("exceeds limit"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_order_rate_ignores_orders_outside_window() {
        let sid = StrategyId::new("EMACross-001");
        let guardrail = OrderRateGuardrail::new(sid, 1, 60_000_000_000);

        let mut ctx = test_context();
        let base_ns = ctx.ts_context.as_u64();
        ctx.orders = vec![test_order_snapshot(
            sid,
            UnixNanos::from(base_ns - 120_000_000_000),
        )];

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_order_rate_filters_by_strategy() {
        let sid = StrategyId::new("EMACross-001");
        let other_sid = StrategyId::new("Momentum-002");
        let guardrail = OrderRateGuardrail::new(sid, 1, 60_000_000_000);

        let mut ctx = test_context();
        let base_ns = ctx.ts_context.as_u64();
        ctx.orders = vec![test_order_snapshot(
            other_sid,
            UnixNanos::from(base_ns - 10_000_000_000),
        )];

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_order_rate_approves_non_trading_intents() {
        let sid = StrategyId::new("EMACross-001");
        let guardrail = OrderRateGuardrail::new(sid, 0, 60_000_000_000);
        let ctx = test_context();

        let intent = test_run_backtest_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_order_rate_allows_cancel_during_burst() {
        let sid = StrategyId::new("EMACross-001");
        let guardrail = OrderRateGuardrail::new(sid, 1, 60_000_000_000);

        let mut ctx = test_context();
        let base_ns = ctx.ts_context.as_u64();
        ctx.orders = vec![
            test_order_snapshot(sid, UnixNanos::from(base_ns - 10_000_000_000)),
            test_order_snapshot(sid, UnixNanos::from(base_ns - 5_000_000_000)),
        ];

        let cancel = AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-test"),
        };
        assert!(matches!(
            guardrail.evaluate(&cancel, &ctx),
            GuardrailResult::Approved
        ));

        let cancel_all = AgentIntent::CancelAllOrders {
            instrument_id: test_instrument_id(),
            order_side: OrderSide::Buy,
        };
        assert!(matches!(
            guardrail.evaluate(&cancel_all, &ctx),
            GuardrailResult::Approved
        ));
    }
}
