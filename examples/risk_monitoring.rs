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

//! Risk monitoring: detect a position anomaly and reduce exposure.
//!
//! ```bash
//! cargo run --example risk_monitoring
//! ```
//!
//! Demonstrates a defensive risk agent that monitors position size and
//! emits `ReducePosition` when exposure exceeds a threshold. The
//! `PositionLimitGuardrail` enforces per-order quantity limits as a
//! second safety layer.

use std::collections::BTreeSet;

use nautilus_agents::prelude::*;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{CurrencyType, OrderSide, PositionSide},
    events::PositionSnapshot,
    identifiers::{AccountId, ClientOrderId, PositionId, StrategyId, TraderId},
    types::{Currency, Quantity},
};
use pollster::block_on;

const MAX_POSITION_QTY: &str = "2.0";
const REDUCE_TO_QTY: &str = "1.0";

/// Reduces exposure when position quantity exceeds a threshold.
struct ExposureMonitorPolicy {
    instrument_id: InstrumentId,
    strategy_id: StrategyId,
    threshold: Quantity,
    target: Quantity,
}

impl AgentPolicy for ExposureMonitorPolicy {
    fn evaluate<'a>(&'a self, context: &'a AgentContext) -> PolicyFuture<'a> {
        Box::pin(async move {
            let position = context.positions.iter().find(|p| {
                p.instrument_id == self.instrument_id && p.strategy_id == self.strategy_id
            });

            let Some(pos) = position else {
                return Ok(PolicyDecision::NoAction);
            };

            if pos.quantity <= self.threshold {
                return Ok(PolicyDecision::NoAction);
            }

            let reduce_by = Quantity::from_raw(
                pos.quantity.raw.saturating_sub(self.target.raw),
                pos.quantity.precision,
            );

            Ok(PolicyDecision::execute(AgentIntent::ReducePosition {
                instrument_id: self.instrument_id,
                quantity: reduce_by,
                constraints: ExecutionConstraints {
                    reduce_only: true,
                    ..Default::default()
                },
            }))
        })
    }
}

fn main() {
    let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
    let strategy_id = StrategyId::new("RiskMon-001");
    let trader_id = TraderId::new("TESTER-001");

    let usdt = Currency::new("USDT", 8, 0, "Tether", CurrencyType::Crypto);

    let position = PositionSnapshot {
        trader_id,
        strategy_id,
        instrument_id,
        position_id: PositionId::new("P-ETH-001"),
        account_id: AccountId::new("SIM-001"),
        opening_order_id: ClientOrderId::new("O-001"),
        closing_order_id: None,
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 3.0,
        quantity: Quantity::from("3.0"),
        peak_qty: Quantity::from("3.0"),
        quote_currency: usdt,
        base_currency: None,
        settlement_currency: usdt,
        avg_px_open: 3500.0,
        avg_px_close: None,
        realized_return: None,
        realized_pnl: None,
        unrealized_pnl: None,
        commissions: vec![],
        duration_ns: None,
        ts_opened: UnixNanos::from(1_712_399_000_000_000_000u64),
        ts_closed: None,
        ts_init: UnixNanos::from(1_712_399_000_000_000_000u64),
        ts_last: UnixNanos::from(1_712_399_999_000_000_000u64),
    };

    let capabilities = CapabilitySet {
        observations: BTreeSet::from([
            ObservationCapability::Positions,
            ObservationCapability::AccountState,
        ]),
        actions: BTreeSet::from([ActionCapability::ManagePositions]),
        instrument_scope: BTreeSet::from([instrument_id]),
    };

    let context = AgentContext {
        ts_context: UnixNanos::from(1_712_400_000_000_000_000u64),
        capabilities,
        quotes: vec![],
        bars: vec![],
        account_state: None,
        positions: vec![position],
        orders: vec![],
        position_reports: vec![],
    };

    let policy = ExposureMonitorPolicy {
        instrument_id,
        strategy_id,
        threshold: Quantity::from(MAX_POSITION_QTY),
        target: Quantity::from(REDUCE_TO_QTY),
    };

    let lowering = LoweringContext {
        trader_id,
        strategy_id,
    };

    let guardrail = PositionLimitGuardrail::new(strategy_id, Quantity::from("5.0"));

    let pipeline = DecisionPipeline::new(Box::new(policy), lowering)
        .with_intent_guardrail(Box::new(guardrail));

    let trigger = DecisionTrigger::StateChange {
        description: "position quantity exceeded threshold".to_string(),
    };

    let envelope = block_on(pipeline.run(trigger, context));

    println!("Decision:        {:?}", envelope.decision);

    if let Some(outcome) = &envelope.outcome {
        println!("Intent ID:       {}", outcome.intent_id);
        println!("Intent guardrail:{:?}", outcome.intent_guardrail);
        println!("Lowering:        {:?}", outcome.lowering_result);
        match &outcome.lowered_action {
            Some(RuntimeAction::Trade(action)) => {
                println!("Trade action:    {action:?}");
            }
            other => {
                println!("Action:          {other:?}");
            }
        }
    }
}
