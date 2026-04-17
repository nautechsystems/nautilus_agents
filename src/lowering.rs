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

//! Intent lowering: translates planned intents into runtime actions.
//!
//! The [`lower_planned_intent`] function fills execution details that
//! the intent does not carry (trader ID, strategy ID, order type, time
//! in force), producing a concrete [`RuntimeAction`] the engine can
//! execute. The planned intent's `intent_id` flows into command
//! params as correlation metadata.
//!
//! Operations intents lower to trading commands. Research intents lower
//! to [`ResearchCommand`](crate::action::ResearchCommand) variants.
//! Workflow intents (`SaveCandidate`, `RejectHypothesis`) and strategy
//! management intents return [`LoweringError::NotLowerable`].

use nautilus_common::messages::execution::{
    CancelAllOrders as CancelAllOrdersCmd, CancelOrder as CancelOrderCmd, SubmitOrder,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    enums::{OrderSide, OrderType, PositionSide, TimeInForce},
    events::{OrderInitialized, PositionSnapshot},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use serde_json::json;

use crate::{
    action::{ManagementCommand, ResearchCommand, RuntimeAction, TradeAction},
    context::AgentContext,
    intent::AgentIntent,
    policy::PlannedIntent,
};

const INTENT_ID_PARAM_KEY: &str = "nautilus_agents.intent_id";

/// Identifiers that intents do not carry. Supplied by the runtime
/// when constructing the pipeline.
#[derive(Clone, Debug)]
pub struct LoweringContext {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum LoweringError {
    #[error("no position found for {instrument_id}")]
    NoPosition { instrument_id: InstrumentId },
    #[error("position is flat for {instrument_id}")]
    FlatPosition { instrument_id: InstrumentId },
    #[error("unsupported constraint: {description}")]
    UnsupportedConstraint { description: String },
    #[error("strategy {target} not managed by this pipeline ({bound})")]
    StrategyMismatch {
        target: StrategyId,
        bound: StrategyId,
    },
    #[error("intent not lowerable: {description}")]
    NotLowerable { description: String },
}

/// Translate a [`PlannedIntent`] into a [`RuntimeAction`].
///
/// Preserves the planned intent's `intent_id` as correlation metadata
/// on lowered runtime actions and command params. Operations intents
/// lower to trading commands. Research intents lower to
/// `ResearchCommand` variants. `AdjustParameters` lowers to
/// `RunBacktest` configuration. Workflow intents (`SaveCandidate`,
/// `RejectHypothesis`) and strategy management intents are not
/// lowerable.
pub fn lower_planned_intent(
    planned_intent: &PlannedIntent,
    context: &AgentContext,
    lowering: &LoweringContext,
    ts_init: UnixNanos,
) -> Result<RuntimeAction, LoweringError> {
    let intent_id = planned_intent.intent_id;
    let intent = &planned_intent.intent;
    match intent {
        AgentIntent::ReducePosition {
            instrument_id,
            quantity,
            constraints,
        } => {
            reject_unsupported_constraints(constraints)?;
            let position = find_position(context, instrument_id, lowering.strategy_id)?;
            let close_side = closing_side(position.side, *instrument_id)?;
            let order_init = market_order_init(
                lowering,
                *instrument_id,
                close_side,
                *quantity,
                true,
                ts_init,
            );
            let submit = submit_order_cmd(
                lowering,
                &order_init,
                Some(position.position_id),
                intent_id,
                ts_init,
            );
            Ok(RuntimeAction::Trade(Box::new(TradeAction::SubmitOrder(
                Box::new(submit),
            ))))
        }

        AgentIntent::ClosePosition {
            instrument_id,
            constraints,
        } => {
            reject_unsupported_constraints(constraints)?;
            let position = find_position(context, instrument_id, lowering.strategy_id)?;
            let close_side = closing_side(position.side, *instrument_id)?;
            let order_init = market_order_init(
                lowering,
                *instrument_id,
                close_side,
                position.quantity,
                true,
                ts_init,
            );
            let submit = submit_order_cmd(
                lowering,
                &order_init,
                Some(position.position_id),
                intent_id,
                ts_init,
            );
            Ok(RuntimeAction::Trade(Box::new(TradeAction::SubmitOrder(
                Box::new(submit),
            ))))
        }

        AgentIntent::CancelOrder {
            instrument_id,
            client_order_id,
        } => {
            let venue_order_id = context
                .orders
                .iter()
                .find(|o| o.client_order_id == *client_order_id)
                .and_then(|o| o.venue_order_id);
            let cmd = CancelOrderCmd {
                trader_id: lowering.trader_id,
                client_id: None,
                strategy_id: lowering.strategy_id,
                instrument_id: *instrument_id,
                client_order_id: *client_order_id,
                venue_order_id,
                command_id: UUID4::new(),
                ts_init,
                params: Some(command_params(intent_id)),
            };
            Ok(RuntimeAction::Trade(Box::new(TradeAction::CancelOrder(
                cmd,
            ))))
        }

        AgentIntent::CancelAllOrders {
            instrument_id,
            order_side,
        } => {
            let cmd = CancelAllOrdersCmd {
                trader_id: lowering.trader_id,
                client_id: None,
                strategy_id: lowering.strategy_id,
                instrument_id: *instrument_id,
                order_side: *order_side,
                command_id: UUID4::new(),
                ts_init,
                params: Some(command_params(intent_id)),
            };
            Ok(RuntimeAction::Trade(Box::new(
                TradeAction::CancelAllOrders(cmd),
            )))
        }

        AgentIntent::PauseStrategy { strategy_id } => {
            check_strategy_scope(*strategy_id, lowering)?;
            Ok(RuntimeAction::Management(
                ManagementCommand::PauseStrategy {
                    strategy_id: *strategy_id,
                    intent_id,
                },
            ))
        }

        AgentIntent::ResumeStrategy { strategy_id } => {
            check_strategy_scope(*strategy_id, lowering)?;
            Ok(RuntimeAction::Management(
                ManagementCommand::ResumeStrategy {
                    strategy_id: *strategy_id,
                    intent_id,
                },
            ))
        }

        AgentIntent::AdjustRiskLimits { params } => Ok(RuntimeAction::Management(
            ManagementCommand::AdjustRiskLimits {
                params: params.clone(),
                intent_id,
            },
        )),

        AgentIntent::EscalateToHuman { reason, severity } => Ok(RuntimeAction::Management(
            ManagementCommand::EscalateToHuman {
                reason: reason.clone(),
                severity: *severity,
                intent_id,
            },
        )),

        AgentIntent::RunBacktest {
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
        } => Ok(RuntimeAction::Research(ResearchCommand::RunBacktest {
            instrument_id: *instrument_id,
            catalog_path: catalog_path.clone(),
            data_cls: data_cls.clone(),
            bar_spec: bar_spec.clone(),
            start_ns: *start_ns,
            end_ns: *end_ns,
            baseline_run_id: None,
            intent_id,
        })),

        AgentIntent::AbortBacktest { run_id } => {
            Ok(RuntimeAction::Research(ResearchCommand::CancelBacktest {
                run_id: run_id.clone(),
                intent_id,
            }))
        }

        AgentIntent::AdjustParameters {
            baseline_run_id,
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
        } => Ok(RuntimeAction::Research(ResearchCommand::RunBacktest {
            instrument_id: *instrument_id,
            catalog_path: catalog_path.clone(),
            data_cls: data_cls.clone(),
            bar_spec: bar_spec.clone(),
            start_ns: *start_ns,
            end_ns: *end_ns,
            baseline_run_id: Some(baseline_run_id.clone()),
            intent_id,
        })),

        AgentIntent::CompareResults { run_ids } => {
            Ok(RuntimeAction::Research(ResearchCommand::CompareBacktests {
                run_ids: run_ids.clone(),
                intent_id,
            }))
        }

        AgentIntent::SaveCandidate { .. } | AgentIntent::RejectHypothesis { .. } => {
            Err(LoweringError::NotLowerable {
                description: intent_variant_name(intent).to_string(),
            })
        }
    }
}

fn check_strategy_scope(
    target: StrategyId,
    lowering: &LoweringContext,
) -> Result<(), LoweringError> {
    if target != lowering.strategy_id {
        return Err(LoweringError::StrategyMismatch {
            target,
            bound: lowering.strategy_id,
        });
    }
    Ok(())
}

/// v0 lowering only produces market IOC orders. Reject constraints
/// that imply limit or algorithmic execution rather than silently
/// ignoring them.
fn reject_unsupported_constraints(
    constraints: &crate::intent::ExecutionConstraints,
) -> Result<(), LoweringError> {
    if constraints.limit_price.is_some() {
        return Err(LoweringError::UnsupportedConstraint {
            description: "limit_price requires limit order support".to_string(),
        });
    }

    if constraints.target_price.is_some() {
        return Err(LoweringError::UnsupportedConstraint {
            description: "target_price requires algorithmic execution".to_string(),
        });
    }

    if constraints.max_slippage_pct.is_some() {
        return Err(LoweringError::UnsupportedConstraint {
            description: "max_slippage_pct requires slippage control".to_string(),
        });
    }
    Ok(())
}

fn find_position<'a>(
    context: &'a AgentContext,
    instrument_id: &InstrumentId,
    strategy_id: StrategyId,
) -> Result<&'a PositionSnapshot, LoweringError> {
    context
        .positions
        .iter()
        .find(|p| p.instrument_id == *instrument_id && p.strategy_id == strategy_id)
        .ok_or(LoweringError::NoPosition {
            instrument_id: *instrument_id,
        })
}

fn closing_side(
    position_side: PositionSide,
    instrument_id: InstrumentId,
) -> Result<OrderSide, LoweringError> {
    match position_side {
        PositionSide::Long => Ok(OrderSide::Sell),
        PositionSide::Short => Ok(OrderSide::Buy),
        _ => Err(LoweringError::FlatPosition { instrument_id }),
    }
}

fn market_order_init(
    lowering: &LoweringContext,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    quantity: Quantity,
    reduce_only: bool,
    ts_init: UnixNanos,
) -> OrderInitialized {
    OrderInitialized {
        trader_id: lowering.trader_id,
        strategy_id: lowering.strategy_id,
        instrument_id,
        client_order_id: ClientOrderId::new(format!("AGENT-{}", UUID4::new())),
        order_side,
        order_type: OrderType::Market,
        quantity,
        time_in_force: TimeInForce::Ioc,
        post_only: false,
        reduce_only,
        quote_quantity: false,
        reconciliation: false,
        event_id: UUID4::new(),
        ts_event: ts_init,
        ts_init,
        price: None,
        trigger_price: None,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: None,
        expire_time: None,
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
    }
}

fn submit_order_cmd(
    lowering: &LoweringContext,
    order_init: &OrderInitialized,
    position_id: Option<nautilus_model::identifiers::PositionId>,
    intent_id: UUID4,
    ts_init: UnixNanos,
) -> SubmitOrder {
    SubmitOrder {
        trader_id: lowering.trader_id,
        client_id: None,
        strategy_id: lowering.strategy_id,
        instrument_id: order_init.instrument_id,
        client_order_id: order_init.client_order_id,
        order_init: order_init.clone(),
        exec_algorithm_id: None,
        position_id,
        command_id: UUID4::new(),
        ts_init,
        params: Some(command_params(intent_id)),
    }
}

fn command_params(intent_id: UUID4) -> Params {
    let mut params = Params::new();
    params.insert(
        INTENT_ID_PARAM_KEY.to_string(),
        json!(intent_id.to_string()),
    );
    params
}

fn intent_variant_name(intent: &AgentIntent) -> &'static str {
    match intent {
        AgentIntent::ReducePosition { .. } => "ReducePosition",
        AgentIntent::ClosePosition { .. } => "ClosePosition",
        AgentIntent::CancelOrder { .. } => "CancelOrder",
        AgentIntent::CancelAllOrders { .. } => "CancelAllOrders",
        AgentIntent::PauseStrategy { .. } => "PauseStrategy",
        AgentIntent::ResumeStrategy { .. } => "ResumeStrategy",
        AgentIntent::AdjustRiskLimits { .. } => "AdjustRiskLimits",
        AgentIntent::EscalateToHuman { .. } => "EscalateToHuman",
        AgentIntent::RunBacktest { .. } => "RunBacktest",
        AgentIntent::AbortBacktest { .. } => "AbortBacktest",
        AgentIntent::AdjustParameters { .. } => "AdjustParameters",
        AgentIntent::CompareResults { .. } => "CompareResults",
        AgentIntent::SaveCandidate { .. } => "SaveCandidate",
        AgentIntent::RejectHypothesis { .. } => "RejectHypothesis",
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, PositionSide},
        identifiers::{ClientOrderId, PositionId, TraderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        action::{ManagementCommand, ResearchCommand, TradeAction},
        fixtures::{
            lower_intent, test_context, test_context_with_position, test_instrument_id,
            test_intent, test_lowering_ctx, test_position_snapshot, test_run_backtest_intent,
        },
        intent::{EscalationSeverity, ExecutionConstraints},
        policy::PlannedIntent,
    };

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
                baseline_run_id,
                ..
            }) => {
                assert_eq!(instrument_id, test_instrument_id());
                assert_eq!(catalog_path, "/data/catalog");
                assert_eq!(data_cls, "Bar");
                assert_eq!(bar_spec, Some("5-MINUTE-MID".to_string()));
                assert_eq!(start_ns, None);
                assert_eq!(end_ns, None);
                assert_eq!(baseline_run_id, Some("run-001".to_string()));
            }
            other => panic!("expected Research(RunBacktest), got {other:?}"),
        }
    }

    #[rstest]
    fn test_lower_run_backtest_has_no_baseline() {
        let ctx = test_context();
        let lowering = test_lowering_ctx();
        let intent = test_run_backtest_intent();
        let action = lower_intent(&intent, &ctx, &lowering, ctx.ts_context).unwrap();
        match action {
            RuntimeAction::Research(ResearchCommand::RunBacktest {
                baseline_run_id, ..
            }) => {
                assert_eq!(baseline_run_id, None);
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
        let ctx = test_context();
        let lowering = test_lowering_ctx();

        let result = lower_intent(&test_intent(), &ctx, &lowering, ctx.ts_context);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_lower_selects_position_by_strategy() {
        let mut ctx = test_context_with_position();
        let mut other_position = test_position_snapshot();
        other_position.strategy_id = StrategyId::new("Other-001");
        other_position.position_id = PositionId::new("P-OTHER");
        other_position.side = PositionSide::Short;
        other_position.quantity = Quantity::from("3.0");
        other_position.signed_qty = -3.0;
        ctx.positions.push(other_position);

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
                    assert_eq!(submit.position_id, Some(PositionId::new("P-001")));
                }
                other => panic!("expected SubmitOrder, got {other:?}"),
            },
            other => panic!("expected Trade, got {other:?}"),
        }
    }
}
