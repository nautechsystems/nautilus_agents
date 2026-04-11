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

//! Research workflow: run a backtest iteration through the decision pipeline.
//!
//! ```bash
//! cargo run --example research_workflow
//! ```
//!
//! Demonstrates how a research-focused agent policy emits `RunBacktest`
//! intents that lower to `ResearchCommand` variants, with every decision
//! recorded in an envelope for replay.

use std::collections::BTreeSet;

use nautilus_agents::prelude::*;
use pollster::block_on;

struct BacktestIterationPolicy;

impl AgentPolicy for BacktestIterationPolicy {
    fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> PolicyFuture<'a> {
        Box::pin(async move {
            Ok(PolicyDecision::Execute(PlannedIntent::new(
                AgentIntent::RunBacktest {
                    instrument_id: InstrumentId::from("BTCUSDT.BINANCE"),
                    catalog_path: "/data/catalog".to_string(),
                    data_cls: "Bar".to_string(),
                    bar_spec: Some("1-MINUTE-LAST".to_string()),
                    start_ns: None,
                    end_ns: None,
                },
            )))
        })
    }
}

fn main() {
    let capabilities = CapabilitySet {
        observations: BTreeSet::from([ObservationCapability::Quotes]),
        actions: BTreeSet::from([ActionCapability::Research]),
        instrument_scope: BTreeSet::from([InstrumentId::from("BTCUSDT.BINANCE")]),
    };

    let context = AgentContext {
        ts_context: UnixNanos::from(1_712_400_000_000_000_000u64),
        capabilities,
        quotes: vec![QuoteTick::new(
            InstrumentId::from("BTCUSDT.BINANCE"),
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
    };

    let lowering = LoweringContext {
        trader_id: "RESEARCHER-001".into(),
        strategy_id: StrategyId::new("ParamSweep-001"),
    };

    let pipeline = DecisionPipeline::new(Box::new(BacktestIterationPolicy), lowering);

    let trigger = DecisionTrigger::Timer {
        interval_ns: 60_000_000_000,
    };

    let envelope = block_on(pipeline.run(trigger, context)).unwrap();

    println!("Decision: {:?}", envelope.decision);

    if let Some(outcome) = &envelope.outcome {
        println!("Intent ID: {}", outcome.intent_id);
        println!("Lowering:  {:?}", outcome.lowering_result);
        println!("Action:    {:?}", outcome.lowered_action);

        match &outcome.lowered_action {
            Some(RuntimeAction::Research(cmd)) => {
                println!("Research command: {cmd:?}");
            }
            other => {
                println!("Unexpected action: {other:?}");
            }
        }
    }
}
