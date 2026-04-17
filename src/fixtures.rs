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

//! Shared test fixtures and helper policies used across per-module test
//! suites. Gated on `cfg(test)` so nothing leaks into the public surface.

#![allow(dead_code)]

use std::collections::BTreeSet;

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::QuoteTick,
    enums::{
        AccountType, CurrencyType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
    },
    events::{AccountState, OrderSnapshot, PositionSnapshot},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use pollster::block_on;

use crate::{
    action::RuntimeAction,
    capability::{ActionCapability, CapabilitySet, ObservationCapability},
    context::AgentContext,
    envelope::{DecisionEnvelope, DecisionTrigger, GuardrailResult},
    guardrail::{ActionGuardrail, IntentGuardrail},
    intent::{AgentIntent, ExecutionConstraints},
    lowering::{LoweringContext, LoweringError, lower_planned_intent},
    pipeline::DecisionPipeline,
    policy::{AgentPolicy, PlannedIntent, PolicyDecision, PolicyError, PolicyFuture},
    replay::{ReplayError, ReplayResult, ReplayRunner},
};

pub(crate) fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("BTCUSDT.BINANCE")
}

pub(crate) fn test_capabilities() -> CapabilitySet {
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

pub(crate) fn test_context() -> AgentContext {
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

pub(crate) fn test_currency() -> Currency {
    Currency::new("USDT", 8, 0, "Tether", CurrencyType::Crypto)
}

pub(crate) fn test_position_snapshot() -> PositionSnapshot {
    PositionSnapshot {
        trader_id: TraderId::new("TESTER-001"),
        strategy_id: StrategyId::new("EMACross-001"),
        instrument_id: test_instrument_id(),
        position_id: PositionId::new("P-001"),
        account_id: AccountId::new("SIM-001"),
        opening_order_id: ClientOrderId::new("O-001"),
        closing_order_id: None,
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.5,
        quantity: Quantity::from("1.5"),
        peak_qty: Quantity::from("1.5"),
        quote_currency: test_currency(),
        base_currency: None,
        settlement_currency: test_currency(),
        avg_px_open: 68450.0,
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
    }
}

pub(crate) fn test_context_with_position() -> AgentContext {
    let mut ctx = test_context();
    ctx.capabilities
        .actions
        .insert(ActionCapability::ManageOrders);
    ctx.positions = vec![test_position_snapshot()];
    ctx
}

pub(crate) fn test_research_context() -> AgentContext {
    let mut ctx = test_context();
    ctx.capabilities.actions.insert(ActionCapability::Research);
    ctx
}

pub(crate) fn test_lowering_ctx() -> LoweringContext {
    LoweringContext {
        trader_id: TraderId::new("TESTER-001"),
        strategy_id: StrategyId::new("EMACross-001"),
    }
}

pub(crate) fn test_intent() -> AgentIntent {
    AgentIntent::ReducePosition {
        instrument_id: test_instrument_id(),
        quantity: Quantity::from("0.5"),
        constraints: ExecutionConstraints {
            reduce_only: true,
            ..Default::default()
        },
    }
}

pub(crate) fn test_run_backtest_intent() -> AgentIntent {
    AgentIntent::RunBacktest {
        instrument_id: test_instrument_id(),
        catalog_path: "/data/catalog".to_string(),
        data_cls: "Bar".to_string(),
        bar_spec: Some("1-HOUR-BID".to_string()),
        start_ns: None,
        end_ns: None,
    }
}

pub(crate) fn test_account_state(total_f64: f64) -> AccountState {
    let currency = test_currency();
    AccountState::new(
        AccountId::new("SIM-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::new(total_f64, currency),
            Money::new(0.0, currency),
            Money::new(total_f64, currency),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(currency),
    )
}

pub(crate) fn test_order_snapshot(strategy_id: StrategyId, ts_init: UnixNanos) -> OrderSnapshot {
    OrderSnapshot {
        trader_id: TraderId::from("TESTER-001"),
        strategy_id,
        instrument_id: test_instrument_id(),
        client_order_id: ClientOrderId::new(format!("O-{}", UUID4::new())),
        venue_order_id: None,
        position_id: None,
        account_id: None,
        last_trade_id: None,
        order_type: OrderType::Market,
        order_side: OrderSide::Buy,
        quantity: Quantity::from("1.0"),
        price: None,
        trigger_price: None,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: None,
        time_in_force: TimeInForce::Ioc,
        expire_time: None,
        filled_qty: Quantity::from("0"),
        liquidity_side: None,
        avg_px: None,
        slippage: None,
        commissions: vec![],
        status: OrderStatus::Accepted,
        is_post_only: false,
        is_reduce_only: false,
        is_quote_quantity: false,
        display_qty: None,
        emulation_trigger: None,
        trigger_instrument_id: None,
        contingency_type: None,
        order_list_id: None,
        linked_order_ids: None,
        parent_order_id: None,
        exec_algorithm_id: None,
        exec_algorithm_params: None,
        exec_spawn_id: None,
        tags: None,
        init_id: UUID4::new(),
        ts_init,
        ts_last: ts_init,
    }
}

pub(crate) fn execute(intent: AgentIntent) -> PolicyDecision {
    PolicyDecision::Execute(PlannedIntent::new(intent))
}

pub(crate) fn planned_intent(decision: &PolicyDecision) -> &PlannedIntent {
    match decision {
        PolicyDecision::Execute(planned_intent) => planned_intent,
        other => panic!("expected Execute, got {other:?}"),
    }
}

pub(crate) fn lower_intent(
    intent: &AgentIntent,
    context: &AgentContext,
    lowering: &LoweringContext,
    ts_init: UnixNanos,
) -> Result<RuntimeAction, LoweringError> {
    lower_planned_intent(
        &PlannedIntent::new(intent.clone()),
        context,
        lowering,
        ts_init,
    )
}

pub(crate) fn run_pipeline(
    pipeline: &DecisionPipeline,
    trigger: DecisionTrigger,
    context: AgentContext,
) -> DecisionEnvelope {
    block_on(pipeline.run(trigger, context))
}

pub(crate) fn run_replay(
    runner: &ReplayRunner,
    envelopes: Vec<DecisionEnvelope>,
) -> Result<Vec<ReplayResult>, ReplayError> {
    block_on(runner.run(envelopes))
}

pub(crate) struct FixedPolicy(pub PolicyDecision);

impl AgentPolicy for FixedPolicy {
    fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> PolicyFuture<'a> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

pub(crate) struct FreshPlanPolicy(pub AgentIntent);

impl AgentPolicy for FreshPlanPolicy {
    fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> PolicyFuture<'a> {
        let intent = self.0.clone();
        Box::pin(async move { Ok(execute(intent)) })
    }
}

pub(crate) struct FailingPolicy(pub PolicyError);

impl AgentPolicy for FailingPolicy {
    fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> PolicyFuture<'a> {
        let e = self.0.clone();
        Box::pin(async move { Err(e) })
    }
}

pub(crate) struct ApproveAllIntents;

impl IntentGuardrail for ApproveAllIntents {
    fn evaluate(&self, _intent: &AgentIntent, _context: &AgentContext) -> GuardrailResult {
        GuardrailResult::Approved
    }
}

pub(crate) struct RejectAllIntents(pub String);

impl IntentGuardrail for RejectAllIntents {
    fn evaluate(&self, _intent: &AgentIntent, _context: &AgentContext) -> GuardrailResult {
        GuardrailResult::Rejected {
            reason: self.0.clone(),
        }
    }
}

pub(crate) struct ApproveAllActions;

impl ActionGuardrail for ApproveAllActions {
    fn evaluate(&self, _action: &RuntimeAction, _context: &AgentContext) -> GuardrailResult {
        GuardrailResult::Approved
    }
}

pub(crate) struct RejectAllActions(pub String);

impl ActionGuardrail for RejectAllActions {
    fn evaluate(&self, _action: &RuntimeAction, _context: &AgentContext) -> GuardrailResult {
        GuardrailResult::Rejected {
            reason: self.0.clone(),
        }
    }
}
