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
pub mod guardrail;
pub mod guardrails;
pub mod intent;
pub mod lowering;
pub mod pipeline;
pub mod policy;
pub mod recording;
pub mod replay;

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
        action::{ManagementCommand, ResearchCommand, RuntimeAction, TradeAction},
        capability::{ActionCapability, CapabilitySet, ObservationCapability},
        context::AgentContext,
        envelope::{
            DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
            LoweringOutcome, PlannedIntentOutcome, ReconciliationOutcome,
        },
        guardrail::{ActionGuardrail, IntentGuardrail},
        guardrails::{
            max_drawdown::MaxDrawdownGuardrail, order_rate::OrderRateGuardrail,
            position_limit::PositionLimitGuardrail,
        },
        intent::{AgentIntent, EscalationSeverity, ExecutionConstraints},
        lowering::{LoweringContext, LoweringError, lower_planned_intent},
        pipeline::DecisionPipeline,
        policy::{AgentPolicy, PlannedIntent, PolicyDecision, PolicyError, PolicyFuture},
        recording::{DecisionRecorder, RecordingError},
        replay::{ReplayConfig, ReplayError, ReplayResult, ReplayRunner, read_envelopes},
    };
}

#[cfg(test)]
mod tests {
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
    use rstest::rstest;

    use crate::{
        action::{ManagementCommand, ResearchCommand, RuntimeAction, TradeAction},
        capability::{ActionCapability, CapabilitySet, ObservationCapability},
        context::AgentContext,
        envelope::{
            DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
            LoweringOutcome, PlannedIntentOutcome, ReconciliationOutcome,
        },
        guardrail::{ActionGuardrail, IntentGuardrail},
        guardrails::{
            max_drawdown::MaxDrawdownGuardrail, order_rate::OrderRateGuardrail,
            position_limit::PositionLimitGuardrail,
        },
        intent::{AgentIntent, ExecutionConstraints},
        lowering::{LoweringContext, lower_planned_intent},
        pipeline::DecisionPipeline,
        policy::{AgentPolicy, PlannedIntent, PolicyDecision, PolicyError},
        recording::DecisionRecorder,
        replay::{ReplayConfig, ReplayError, ReplayResult, ReplayRunner, read_envelopes},
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

    fn test_run_backtest_intent() -> AgentIntent {
        AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("1-HOUR-BID".to_string()),
            start_ns: None,
            end_ns: None,
        }
    }

    fn test_intent() -> AgentIntent {
        AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints {
                reduce_only: true,
                ..Default::default()
            },
        }
    }

    fn execute(intent: AgentIntent) -> PolicyDecision {
        PolicyDecision::Execute(PlannedIntent::new(intent))
    }

    fn planned_intent(decision: &PolicyDecision) -> &PlannedIntent {
        match decision {
            PolicyDecision::Execute(planned_intent) => planned_intent,
            other => panic!("expected Execute, got {other:?}"),
        }
    }

    fn lower_intent(
        intent: &AgentIntent,
        context: &AgentContext,
        lowering: &LoweringContext,
        ts_init: UnixNanos,
    ) -> Result<RuntimeAction, crate::lowering::LoweringError> {
        lower_planned_intent(
            &PlannedIntent::new(intent.clone()),
            context,
            lowering,
            ts_init,
        )
    }

    fn run_pipeline(
        pipeline: &DecisionPipeline,
        trigger: DecisionTrigger,
        context: AgentContext,
    ) -> DecisionEnvelope {
        block_on(pipeline.run(trigger, context))
    }

    fn run_replay(
        runner: &ReplayRunner,
        envelopes: Vec<DecisionEnvelope>,
    ) -> Result<Vec<ReplayResult>, ReplayError> {
        block_on(runner.run(envelopes))
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
    fn test_capability_research_instrument_denied() {
        let caps = CapabilitySet {
            observations: BTreeSet::new(),
            actions: BTreeSet::from([ActionCapability::Research]),
            instrument_scope: BTreeSet::from([InstrumentId::from("ETHUSDT.BINANCE")]),
        };
        let intent = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: None,
            start_ns: None,
            end_ns: None,
        };
        assert!(caps.check_intent(&intent).is_err());
    }

    #[rstest]
    fn test_capability_abort_backtest_skips_instrument_scope() {
        let caps = CapabilitySet {
            observations: BTreeSet::new(),
            actions: BTreeSet::from([ActionCapability::Research]),
            instrument_scope: BTreeSet::new(),
        };
        let intent = AgentIntent::AbortBacktest {
            run_id: "run-001".to_string(),
        };
        assert!(caps.check_intent(&intent).is_ok());
    }

    #[rstest]
    fn test_policy_decision_round_trip() {
        let decision = execute(test_intent());
        let original_intent_id = planned_intent(&decision).intent_id;
        let json = serde_json::to_string(&decision).unwrap();
        let restored: PolicyDecision = serde_json::from_str(&json).unwrap();
        let restored_intent = planned_intent(&restored);

        assert_eq!(restored_intent.intent_id, original_intent_id);

        match &restored_intent.intent {
            AgentIntent::ReducePosition {
                instrument_id,
                quantity,
                constraints,
            } => {
                assert_eq!(*instrument_id, test_instrument_id());
                assert_eq!(*quantity, Quantity::from("0.5"));
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
    fn test_policy_decision_execute_constructs_execute_variant() {
        let intent = test_intent();
        match PolicyDecision::execute(intent.clone()) {
            PolicyDecision::Execute(planned) => assert_eq!(planned.intent, intent),
            other => panic!("expected Execute, got {other:?}"),
        }
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
    fn test_research_round_trip() {
        let intent = test_run_backtest_intent();
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(restored, AgentIntent::RunBacktest { .. }));
        assert_eq!(restored, intent);
    }

    fn test_currency() -> Currency {
        Currency::new("USDT", 8, 0, "Tether", CurrencyType::Crypto)
    }

    fn test_position_snapshot() -> PositionSnapshot {
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

    fn test_context_with_position() -> AgentContext {
        let mut ctx = test_context();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);
        ctx.positions = vec![test_position_snapshot()];
        ctx
    }

    fn test_lowering_ctx() -> LoweringContext {
        LoweringContext {
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("EMACross-001"),
        }
    }

    struct FixedPolicy(PolicyDecision);

    impl AgentPolicy for FixedPolicy {
        fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> crate::policy::PolicyFuture<'a> {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    struct FreshPlanPolicy(AgentIntent);

    impl AgentPolicy for FreshPlanPolicy {
        fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> crate::policy::PolicyFuture<'a> {
            let intent = self.0.clone();
            Box::pin(async move { Ok(execute(intent)) })
        }
    }

    struct ApproveAllIntents;

    impl IntentGuardrail for ApproveAllIntents {
        fn evaluate(&self, _intent: &AgentIntent, _context: &AgentContext) -> GuardrailResult {
            GuardrailResult::Approved
        }
    }

    struct RejectAllIntents(String);

    impl IntentGuardrail for RejectAllIntents {
        fn evaluate(&self, _intent: &AgentIntent, _context: &AgentContext) -> GuardrailResult {
            GuardrailResult::Rejected {
                reason: self.0.clone(),
            }
        }
    }

    struct ApproveAllActions;

    impl ActionGuardrail for ApproveAllActions {
        fn evaluate(&self, _action: &RuntimeAction, _context: &AgentContext) -> GuardrailResult {
            GuardrailResult::Approved
        }
    }

    struct RejectAllActions(String);

    impl ActionGuardrail for RejectAllActions {
        fn evaluate(&self, _action: &RuntimeAction, _context: &AgentContext) -> GuardrailResult {
            GuardrailResult::Rejected {
                reason: self.0.clone(),
            }
        }
    }

    struct FailingPolicy(PolicyError);

    impl AgentPolicy for FailingPolicy {
        fn evaluate<'a>(&'a self, _context: &'a AgentContext) -> crate::policy::PolicyFuture<'a> {
            let e = self.0.clone();
            Box::pin(async move { Err(e) })
        }
    }

    #[rstest]
    fn test_intent_guardrail_approve() {
        let guardrail = ApproveAllIntents;
        let result = guardrail.evaluate(&test_intent(), &test_context());
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_intent_guardrail_reject() {
        let guardrail = RejectAllIntents("position limit exceeded".to_string());
        let result = guardrail.evaluate(&test_intent(), &test_context());
        match result {
            GuardrailResult::Rejected { reason } => {
                assert_eq!(reason, "position limit exceeded");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_cancel_order() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-123"),
        };

        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::CancelOrder(cmd) => {
                    assert_eq!(cmd.instrument_id, test_instrument_id());
                    assert_eq!(cmd.client_order_id, ClientOrderId::new("O-123"));
                    assert_eq!(cmd.trader_id, TraderId::new("TESTER-001"));
                }
                other => panic!("expected CancelOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_planned_intent_preserves_intent_id_in_command_params() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let planned_intent = PlannedIntent::new(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-789"),
        });

        let action =
            lower_planned_intent(&planned_intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::CancelOrder(cmd) => {
                    let params = cmd.params.expect("missing command params");
                    let intent_id = planned_intent.intent_id.to_string();
                    assert_eq!(
                        params.get_str("nautilus_agents.intent_id"),
                        Some(intent_id.as_str())
                    );
                }
                other => panic!("expected CancelOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_planned_run_backtest_preserves_intent_id() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let planned_intent = PlannedIntent::new(test_run_backtest_intent());
        let expected = planned_intent.intent_id;

        let action =
            lower_planned_intent(&planned_intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::RunBacktest { intent_id, .. }) => {
                assert_eq!(intent_id, expected);
            }
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_planned_pause_strategy_preserves_intent_id() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let planned_intent = PlannedIntent::new(AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        });
        let expected = planned_intent.intent_id;

        let action =
            lower_planned_intent(&planned_intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Management(ManagementCommand::PauseStrategy { intent_id, .. }) => {
                assert_eq!(intent_id, expected);
            }
            other => panic!("expected Management(PauseStrategy), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_cancel_all_orders() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::CancelAllOrders {
            instrument_id: test_instrument_id(),
            order_side: OrderSide::Buy,
        };

        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::CancelAllOrders(cmd) => {
                    assert_eq!(cmd.instrument_id, test_instrument_id());
                    assert_eq!(cmd.order_side, OrderSide::Buy);
                }
                other => panic!("expected CancelAllOrders, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_reduce_position_long_produces_sell() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();

        let action = lower_intent(&test_intent(), &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::SubmitOrder(submit) => {
                    assert_eq!(submit.order_init.order_side, OrderSide::Sell);
                    assert_eq!(submit.order_init.quantity, Quantity::from("0.5"));
                    assert!(submit.order_init.reduce_only);
                    assert_eq!(submit.position_id, Some(PositionId::new("P-001")));
                }
                other => panic!("expected SubmitOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_close_position_uses_full_quantity() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };

        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::SubmitOrder(submit) => {
                    assert_eq!(submit.order_init.order_side, OrderSide::Sell);
                    assert_eq!(submit.order_init.quantity, Quantity::from("1.5"));
                    assert!(submit.order_init.reduce_only);
                }
                other => panic!("expected SubmitOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_rejects_limit_price_constraint() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints {
                limit_price: Some(Price::from("68000.00")),
                ..Default::default()
            },
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("limit_price"));
    }

    #[rstest]
    fn test_lower_rejects_target_price_constraint() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints {
                target_price: Some(Price::from("70000.00")),
                ..Default::default()
            },
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("target_price"));
    }

    #[rstest]
    fn test_lower_rejects_max_slippage_constraint() {
        let ctx = test_context_with_position();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::ReducePosition {
            instrument_id: test_instrument_id(),
            quantity: Quantity::from("0.5"),
            constraints: ExecutionConstraints {
                max_slippage_pct: Some(0.001),
                ..Default::default()
            },
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("max_slippage_pct"));
    }

    #[rstest]
    fn test_lower_run_backtest() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = test_run_backtest_intent();
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::RunBacktest {
                instrument_id,
                catalog_path,
                data_cls,
                bar_spec,
                ..
            }) => {
                assert_eq!(instrument_id, test_instrument_id());
                assert_eq!(catalog_path, "/data/catalog");
                assert_eq!(data_cls, "Bar");
                assert_eq!(bar_spec, Some("1-HOUR-BID".to_string()));
            }
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_abort_backtest() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::AbortBacktest {
            run_id: "run-001".to_string(),
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::CancelBacktest { run_id, .. }) => {
                assert_eq!(run_id, "run-001");
            }
            other => panic!("expected Research(CancelBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_adjust_parameters_produces_run_backtest() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::AdjustParameters {
            baseline_run_id: "run-001".to_string(),
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("5-MINUTE-MID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::RunBacktest {
                instrument_id,
                catalog_path,
                data_cls,
                bar_spec,
                start_ns,
                end_ns,
                ..
            }) => {
                assert_eq!(instrument_id, test_instrument_id());
                assert_eq!(catalog_path, "/data/catalog");
                assert_eq!(data_cls, "Bar");
                assert_eq!(bar_spec, Some("5-MINUTE-MID".to_string()));
                assert_eq!(start_ns, None);
                assert_eq!(end_ns, None);
            }
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_compare_results() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::CompareResults {
            run_ids: vec!["run-001".to_string(), "run-002".to_string()],
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::CompareBacktests { run_ids, .. }) => {
                assert_eq!(run_ids, vec!["run-001", "run-002"]);
            }
            other => panic!("expected Research(CompareBacktests), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_run_backtest_with_time_range() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let start = UnixNanos::from(1_700_000_000_000_000_000u64);
        let end = UnixNanos::from(1_712_000_000_000_000_000u64);
        let intent = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "QuoteTick".to_string(),
            bar_spec: None,
            start_ns: Some(start),
            end_ns: Some(end),
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::RunBacktest {
                data_cls,
                bar_spec,
                start_ns,
                end_ns,
                ..
            }) => {
                assert_eq!(data_cls, "QuoteTick");
                assert_eq!(bar_spec, None);
                assert_eq!(start_ns, Some(start));
                assert_eq!(end_ns, Some(end));
            }
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
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

    #[rstest]
    fn test_lower_pause_strategy() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Management(ManagementCommand::PauseStrategy { strategy_id, .. }) => {
                assert_eq!(strategy_id, StrategyId::new("EMACross-001"));
            }
            other => panic!("expected Management(PauseStrategy), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_resume_strategy() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::ResumeStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Management(ManagementCommand::ResumeStrategy {
                strategy_id, ..
            }) => {
                assert_eq!(strategy_id, StrategyId::new("EMACross-001"));
            }
            other => panic!("expected Management(ResumeStrategy), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_adjust_risk_limits() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let params = serde_json::json!({"max_drawdown_pct": 0.05});
        let intent = AgentIntent::AdjustRiskLimits {
            params: params.clone(),
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Management(ManagementCommand::AdjustRiskLimits {
                params: lowered,
                ..
            }) => {
                assert_eq!(lowered, params);
            }
            other => panic!("expected Management(AdjustRiskLimits), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_escalate_to_human() {
        use crate::intent::EscalationSeverity;

        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::EscalateToHuman {
            reason: "drawdown limit breached".to_string(),
            severity: EscalationSeverity::Critical,
        };
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Management(ManagementCommand::EscalateToHuman {
                reason,
                severity,
                ..
            }) => {
                assert_eq!(reason, "drawdown limit breached");
                assert_eq!(severity, EscalationSeverity::Critical);
            }
            other => panic!("expected Management(EscalateToHuman), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_pause_strategy_rejects_cross_strategy() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("Other-999"),
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("not managed by this pipeline"));
    }

    #[rstest]
    fn test_lower_save_candidate_not_lowerable() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::SaveCandidate {
            run_id: "run-001".to_string(),
            label: "best so far".to_string(),
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("not lowerable"));
    }

    #[rstest]
    fn test_lower_reject_hypothesis_not_lowerable() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = AgentIntent::RejectHypothesis {
            run_id: "run-001".to_string(),
            reason: "underperforms baseline".to_string(),
        };
        let err = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap_err();
        assert!(err.to_string().contains("not lowerable"));
    }

    #[rstest]
    fn test_lower_no_position_returns_error() {
        let ctx = test_context(); // no positions
        let lowering = test_lowering_ctx();

        let result = lower_intent(&test_intent(), &ctx, &lowering, ctx.ts_context);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_lower_selects_position_by_strategy() {
        let mut ctx = test_context_with_position();
        // Add a second position for the same instrument under a different strategy.
        let mut other_position = test_position_snapshot();
        other_position.strategy_id = StrategyId::new("Other-001");
        other_position.position_id = PositionId::new("P-OTHER");
        other_position.side = PositionSide::Short;
        other_position.quantity = Quantity::from("3.0");
        other_position.signed_qty = -3.0;
        ctx.positions.push(other_position);

        let lowering = test_lowering_ctx(); // strategy_id = EMACross-001
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };

        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Trade(trade) => match *trade {
                TradeAction::SubmitOrder(submit) => {
                    // Should close the EMACross-001 Long position (qty 1.5),
                    // not the Other-001 Short position (qty 3.0).
                    assert_eq!(submit.order_init.order_side, OrderSide::Sell);
                    assert_eq!(submit.order_init.quantity, Quantity::from("1.5"));
                    assert_eq!(submit.position_id, Some(PositionId::new("P-001")));
                }
                other => panic!("expected SubmitOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }

    #[rstest]
    fn test_pipeline_no_action() {
        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context());
        assert!(matches!(envelope.decision, PolicyDecision::NoAction));
        assert!(envelope.outcome.is_none());
    }

    #[rstest]
    fn test_pipeline_act_approved() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-123"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        assert!(outcome.lowered_action.is_some());
        assert!(matches!(
            outcome.action_guardrail,
            Some(GuardrailResult::Approved)
        ));
    }

    #[rstest]
    fn test_pipeline_intent_guardrail_rejected() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(RejectAllIntents(
                "exceeds position limit".to_string(),
            )));

        let trigger = DecisionTrigger::Timer {
            interval_ns: 30_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context_with_position());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        match &outcome.intent_guardrail {
            Some(GuardrailResult::Rejected { reason }) => {
                assert_eq!(reason, "exceeds position limit");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
    }

    #[rstest]
    fn test_pipeline_capability_denied_records_rejection() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let ctx = AgentContext {
            capabilities: CapabilitySet {
                observations: BTreeSet::new(),
                actions: BTreeSet::new(),
                instrument_scope: BTreeSet::new(),
            },
            ..test_context()
        };
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));
        assert!(outcome.lowered_action.is_none());
    }

    #[rstest]
    fn test_pipeline_lowering_failure_records_lowering_result() {
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents));

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        // Context has ManagePositions capability but no positions,
        // so lowering will fail with NoPosition.
        let envelope = run_pipeline(&pipeline, trigger, test_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        match &outcome.lowering_result {
            Some(LoweringOutcome::Failed { reason }) => {
                assert!(reason.contains("no position found"));
            }
            other => panic!("expected LoweringOutcome::Failed, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
    }

    #[rstest]
    fn test_pipeline_action_guardrail_rejection_is_recorded() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-777"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(RejectAllActions(
                "venue circuit breaker open".to_string(),
            )));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        assert!(outcome.lowered_action.is_some());
        match &outcome.action_guardrail {
            Some(GuardrailResult::Rejected { reason }) => {
                assert_eq!(reason, "venue circuit breaker open");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[rstest]
    fn test_pipeline_policy_error_is_recorded_as_failed_decision() {
        let policy = FailingPolicy(PolicyError::Timeout { timeout_ms: 250 });
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };

        let envelope = run_pipeline(&pipeline, trigger, test_context());
        match &envelope.decision {
            PolicyDecision::Failed(PolicyError::Timeout { timeout_ms }) => {
                assert_eq!(*timeout_ms, 250);
            }
            other => panic!("expected Failed(Timeout), got {other:?}"),
        }
        assert!(envelope.outcome.is_none());
    }

    #[rstest]
    fn test_pipeline_round_trip_serialization() {
        let policy = FixedPolicy(execute(AgentIntent::CancelOrder {
            instrument_id: test_instrument_id(),
            client_order_id: ClientOrderId::new("O-456"),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let mut ctx = test_context_with_position();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageOrders);

        let envelope = run_pipeline(&pipeline, trigger, ctx);
        let json = serde_json::to_string(&envelope).unwrap();
        let restored: DecisionEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.schema_version, ENVELOPE_SCHEMA_VERSION);
        assert!(matches!(restored.decision, PolicyDecision::Execute(_)));
        let outcome = restored.outcome.as_ref().expect("expected outcome");
        assert!(outcome.lowered_action.is_some());
    }

    #[rstest]
    fn test_decision_recorder_writes_json_lines() {
        let dir = std::env::temp_dir().join(format!("nautilus_test_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let recorder = DecisionRecorder::new(&path);

        let envelope1 = DecisionEnvelope {
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

        let envelope2 = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Manual {
                reason: "test".to_string(),
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_001_000_000_000u64),
            ts_reconciled: None,
        };

        recorder.record(&envelope1).unwrap();
        recorder.record(&envelope2).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let restored1: DecisionEnvelope = serde_json::from_str(lines[0]).unwrap();
        let restored2: DecisionEnvelope = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(restored1.envelope_id, envelope1.envelope_id);
        assert_eq!(restored2.envelope_id, envelope2.envelope_id);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_record_then_read_fidelity() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let recorder = DecisionRecorder::new(&path);

        let envelope1 = DecisionEnvelope {
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

        let decision = execute(test_intent());
        let intent_id = planned_intent(&decision).intent_id;
        let envelope2 = DecisionEnvelope {
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
                lowering_result: None,
                lowered_action: None,
                action_guardrail: None,
            }),
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_001_000_000_000u64),
            ts_reconciled: None,
        };

        recorder.record(&envelope1).unwrap();
        recorder.record(&envelope2).unwrap();

        let restored = read_envelopes(&path).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].envelope_id, envelope1.envelope_id);
        assert_eq!(restored[1].envelope_id, envelope2.envelope_id);
        assert!(matches!(restored[0].decision, PolicyDecision::NoAction));
        assert_eq!(planned_intent(&restored[1].decision).intent, test_intent());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_record_then_read_failed_envelope() {
        let dir = std::env::temp_dir().join(format!("nautilus_failed_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let envelope = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::Failed(PolicyError::Internal {
                message: "downstream LLM rejected request".to_string(),
            }),
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&envelope).unwrap();

        let restored = read_envelopes(&path).unwrap();
        assert_eq!(restored.len(), 1);
        match &restored[0].decision {
            PolicyDecision::Failed(PolicyError::Internal { message }) => {
                assert_eq!(message, "downstream LLM rejected request");
            }
            other => panic!("expected Failed(Internal), got {other:?}"),
        }
        assert!(restored[0].outcome.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_read_envelopes_malformed_line() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_bad_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(&path, "{\"valid\":false}\n{not json\n").unwrap();

        let err = read_envelopes(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("line 1"), "expected line 1 error, got: {msg}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_different_policy_changes_decision() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_diff_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        // Record an Execute envelope using a policy that submits one planned intent.
        let act_policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(act_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let original = run_pipeline(&pipeline, trigger, test_context_with_position());
        assert!(matches!(original.decision, PolicyDecision::Execute(_)));

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        // Replay with a NoAction policy.
        let no_action_policy = FixedPolicy(PolicyDecision::NoAction);
        let replay_pipeline =
            DecisionPipeline::new(Box::new(no_action_policy), test_lowering_ctx());
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());
        assert!(results[0].summary().contains("changed"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_ignores_fresh_intent_ids_when_intent_is_unchanged() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_intent_ids_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };

        let original_policy = FreshPlanPolicy(test_intent());
        let original_pipeline =
            DecisionPipeline::new(Box::new(original_policy), test_lowering_ctx())
                .with_intent_guardrail(Box::new(ApproveAllIntents))
                .with_action_guardrail(Box::new(ApproveAllActions));
        let original = run_pipeline(&original_pipeline, trigger, test_context_with_position());

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        let replay_policy = FreshPlanPolicy(test_intent());
        let replay_pipeline = DecisionPipeline::new(Box::new(replay_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_stricter_guardrail_rejects() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_guard_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        // Record an envelope approved by a permissive guardrail.
        let policy = FixedPolicy(execute(test_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(ApproveAllIntents))
            .with_action_guardrail(Box::new(ApproveAllActions));

        let trigger = DecisionTrigger::MarketData {
            instrument_id: test_instrument_id(),
        };
        let original = run_pipeline(&pipeline, trigger, test_context_with_position());
        let original_outcome_approved = original.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            original_outcome_approved.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        // Replay with a stricter guardrail that rejects everything.
        let replay_policy = FixedPolicy(execute(test_intent()));
        let replay_pipeline = DecisionPipeline::new(Box::new(replay_policy), test_lowering_ctx())
            .with_intent_guardrail(Box::new(RejectAllIntents("stricter limit".to_string())));
        let runner = ReplayRunner::new(replay_pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        // Original was approved, replayed is rejected: outcome changed.
        assert!(results[0].decision_changed());
        let original_outcome = results[0]
            .original
            .outcome
            .as_ref()
            .expect("expected original outcome");
        let replayed_outcome = results[0]
            .replayed
            .outcome
            .as_ref()
            .expect("expected replayed outcome");
        assert!(matches!(
            original_outcome.intent_guardrail,
            Some(GuardrailResult::Approved)
        ));
        assert!(matches!(
            replayed_outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_skip_no_action() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_skip_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let recorder = DecisionRecorder::new(&path);

        let no_action_envelope = DecisionEnvelope {
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
        recorder.record(&no_action_envelope).unwrap();

        let envelopes = read_envelopes(&path).unwrap();
        assert_eq!(envelopes.len(), 1);

        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let config = ReplayConfig {
            skip_no_action: true,
        };
        let runner = ReplayRunner::new(pipeline, config);
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_noaction_replayed_as_noaction_is_unchanged() {
        let dir =
            std::env::temp_dir().join(format!("nautilus_replay_noaction_eq_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let no_action_envelope = DecisionEnvelope {
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
        let recorder = DecisionRecorder::new(&path);
        recorder.record(&no_action_envelope).unwrap();

        // Replay with a NoAction policy and no skip; comparison must match.
        let policy = FixedPolicy(PolicyDecision::NoAction);
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_failed_error_change() {
        let build = |error: PolicyError| DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::Failed(error),
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let same = ReplayResult {
            original: build(PolicyError::Timeout { timeout_ms: 100 }),
            replayed: build(PolicyError::Timeout { timeout_ms: 100 }),
        };
        assert!(!same.decision_changed());

        let different = ReplayResult {
            original: build(PolicyError::Timeout { timeout_ms: 100 }),
            replayed: build(PolicyError::Timeout { timeout_ms: 200 }),
        };
        assert!(different.decision_changed());
    }

    #[rstest]
    fn test_replay_detects_research_payload_change() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_research_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        // Record with a policy that emits RunBacktest with 1-HOUR-BID bar spec.
        let intent_a = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("1-HOUR-BID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let original = run_pipeline(&pipeline_a, trigger, test_research_context());

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        // Replay with a policy that emits RunBacktest with a different bar spec.
        let intent_b = AgentIntent::RunBacktest {
            instrument_id: test_instrument_id(),
            catalog_path: "/data/catalog".to_string(),
            data_cls: "Bar".to_string(),
            bar_spec: Some("5-MINUTE-MID".to_string()),
            start_ns: None,
            end_ns: None,
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_management_pause_strategy_change() {
        let dir = std::env::temp_dir().join(format!("nautilus_replay_mgmt_pause_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut ctx = test_context();
        ctx.capabilities
            .actions
            .insert(ActionCapability::ManageStrategies);

        // Record: PauseStrategy for EMACross-001. Lowering binds the command's
        // strategy_id to the lowering context's strategy_id, so we match both.
        let lowering_a = LoweringContext {
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let intent_a = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), lowering_a);
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let original = run_pipeline(&pipeline_a, trigger, ctx);

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        // Replay: PauseStrategy for EMACross-002, lowered under a matching
        // LoweringContext. The lowered commands now carry different
        // strategy_ids, so management_commands_match must report the diff.
        let lowering_b = LoweringContext {
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("EMACross-002"),
        };
        let intent_b = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-002"),
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), lowering_b);
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        let original_outcome = results[0]
            .original
            .outcome
            .as_ref()
            .expect("expected original outcome");
        let replayed_outcome = results[0]
            .replayed
            .outcome
            .as_ref()
            .expect("expected replayed outcome");
        assert_ne!(
            action_strategy_id(&original_outcome.lowered_action),
            action_strategy_id(&replayed_outcome.lowered_action),
            "fixture must produce different strategy_ids for the assertion to be meaningful",
        );
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[rstest]
    fn test_replay_detects_management_escalation_severity_change() {
        use crate::intent::EscalationSeverity;

        let dir =
            std::env::temp_dir().join(format!("nautilus_replay_mgmt_escalate_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut ctx = test_context();
        ctx.capabilities.actions.insert(ActionCapability::Escalate);

        // Record: EscalateToHuman at Warning severity.
        let intent_a = AgentIntent::EscalateToHuman {
            reason: "drawdown approaching limit".to_string(),
            severity: EscalationSeverity::Warning,
        };
        let policy_a = FixedPolicy(execute(intent_a));
        let pipeline_a = DecisionPipeline::new(Box::new(policy_a), test_lowering_ctx());
        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let original = run_pipeline(&pipeline_a, trigger, ctx.clone());

        let recorder = DecisionRecorder::new(&path);
        recorder.record(&original).unwrap();

        // Replay: same reason, Critical severity. management_commands_match must
        // compare severity, so decision_changed should report the diff.
        let intent_b = AgentIntent::EscalateToHuman {
            reason: "drawdown approaching limit".to_string(),
            severity: EscalationSeverity::Critical,
        };
        let policy_b = FixedPolicy(execute(intent_b));
        let pipeline_b = DecisionPipeline::new(Box::new(policy_b), test_lowering_ctx());
        let runner = ReplayRunner::new(pipeline_b, ReplayConfig::default());

        let envelopes = read_envelopes(&path).unwrap();
        let results = run_replay(&runner, envelopes).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].decision_changed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn action_strategy_id(action: &Option<RuntimeAction>) -> Option<StrategyId> {
        match action {
            Some(RuntimeAction::Management(ManagementCommand::PauseStrategy {
                strategy_id,
                ..
            })) => Some(*strategy_id),
            _ => None,
        }
    }

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
        // Context with position qty 1.5 exceeds limit of 1.0.
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
        // Context with position qty 1.5 is under limit of 2.0.
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
        // Add a large position under a different strategy for the same instrument.
        let mut other_position = test_position_snapshot();
        other_position.strategy_id = StrategyId::new("Other-001");
        other_position.position_id = PositionId::new("P-OTHER");
        other_position.quantity = Quantity::from("5.0");
        other_position.signed_qty = 5.0;
        ctx.positions.push(other_position);

        // Guardrail for EMACross-001 with limit 2.0.
        // EMACross-001 position is 1.5 (under limit), Other-001 is 5.0 (over limit).
        let guardrail =
            PositionLimitGuardrail::new(StrategyId::new("EMACross-001"), Quantity::from("2.0"));
        let intent = AgentIntent::ClosePosition {
            instrument_id: test_instrument_id(),
            constraints: ExecutionConstraints::default(),
        };
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    fn test_account_state(total_f64: f64) -> AccountState {
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

    #[rstest]
    fn test_max_drawdown_approves_within_limit() {
        let currency = test_currency();
        let guardrail = MaxDrawdownGuardrail::new(Money::new(10000.0, currency), 0.10);
        let mut ctx = test_context();
        ctx.account_state = Some(test_account_state(9500.0));

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    #[rstest]
    fn test_max_drawdown_rejects_beyond_limit() {
        let currency = test_currency();
        let guardrail = MaxDrawdownGuardrail::new(Money::new(10000.0, currency), 0.10);
        let mut ctx = test_context();
        ctx.account_state = Some(test_account_state(8500.0));

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
    fn test_max_drawdown_allows_management_during_drawdown() {
        let currency = test_currency();
        let guardrail = MaxDrawdownGuardrail::new(Money::new(10000.0, currency), 0.10);
        let mut ctx = test_context();
        ctx.account_state = Some(test_account_state(5000.0));

        let pause = AgentIntent::PauseStrategy {
            strategy_id: StrategyId::new("EMACross-001"),
        };
        assert!(matches!(
            guardrail.evaluate(&pause, &ctx),
            GuardrailResult::Approved
        ));

        let escalate = AgentIntent::EscalateToHuman {
            reason: "drawdown".to_string(),
            severity: crate::intent::EscalationSeverity::Critical,
        };
        assert!(matches!(
            guardrail.evaluate(&escalate, &ctx),
            GuardrailResult::Approved
        ));
    }

    #[rstest]
    fn test_max_drawdown_approves_without_account_state() {
        let currency = test_currency();
        let guardrail = MaxDrawdownGuardrail::new(Money::new(10000.0, currency), 0.10);
        let ctx = test_context();

        let intent = test_intent();
        let result = guardrail.evaluate(&intent, &ctx);
        assert!(matches!(result, GuardrailResult::Approved));
    }

    fn test_order_snapshot(strategy_id: StrategyId, ts_init: UnixNanos) -> OrderSnapshot {
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
        // Order from 2 minutes ago, outside the 60s window
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
        // Order belongs to a different strategy
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

    #[rstest]
    fn test_pipeline_research_denied_without_capability() {
        let policy = FixedPolicy(execute(test_run_backtest_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        // test_context() has no Research capability.
        let envelope = run_pipeline(&pipeline, trigger, test_context());
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.intent_guardrail,
            Some(GuardrailResult::Rejected { .. })
        ));
        assert!(outcome.lowered_action.is_none());
    }

    fn test_research_context() -> AgentContext {
        let mut ctx = test_context();
        ctx.capabilities.actions.insert(ActionCapability::Research);
        ctx
    }

    #[rstest]
    fn test_pipeline_research_intent_lowers_successfully() {
        let policy = FixedPolicy(execute(test_run_backtest_intent()));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_research_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        assert!(matches!(
            outcome.lowering_result,
            Some(LoweringOutcome::Success)
        ));
        match &outcome.lowered_action {
            Some(RuntimeAction::Research(ResearchCommand::RunBacktest { .. })) => {}
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_pipeline_workflow_intent_records_lowering_failure() {
        let policy = FixedPolicy(execute(AgentIntent::SaveCandidate {
            run_id: "run-001".to_string(),
            label: "candidate".to_string(),
        }));
        let pipeline = DecisionPipeline::new(Box::new(policy), test_lowering_ctx());

        let trigger = DecisionTrigger::Timer {
            interval_ns: 60_000_000_000,
        };
        let envelope = run_pipeline(&pipeline, trigger, test_research_context());
        assert!(matches!(envelope.decision, PolicyDecision::Execute(_)));
        let outcome = envelope.outcome.as_ref().expect("expected outcome");
        match &outcome.lowering_result {
            Some(LoweringOutcome::Failed { reason }) => {
                assert!(reason.contains("not lowerable"));
            }
            other => panic!("expected LoweringOutcome::Failed, got {other:?}"),
        }
        assert!(outcome.lowered_action.is_none());
        assert!(outcome.action_guardrail.is_none());
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

    #[rstest]
    fn test_read_envelopes_missing_file() {
        let path = std::env::temp_dir().join(format!("nonexistent_{}.jsonl", UUID4::new()));
        let err = read_envelopes(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("I/O error"), "expected I/O error, got: {msg}");
    }
}
