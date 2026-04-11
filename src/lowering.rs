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

//! Intent lowering: translates semantic intents into runtime actions.
//!
//! The [`lower_intent`] function fills execution details that the intent
//! does not carry (trader ID, strategy ID, order type, time in force),
//! producing a concrete [`RuntimeAction`] the engine can execute.
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

/// Translate an [`AgentIntent`] into a [`RuntimeAction`].
///
/// Operations intents lower to trading commands. Research intents
/// lower to `ResearchCommand` variants. `AdjustParameters` lowers
/// to `RunBacktest` configuration. Workflow intents (`SaveCandidate`,
/// `RejectHypothesis`) and strategy management intents are not
/// lowerable.
pub fn lower_intent(
    intent: &AgentIntent,
    context: &AgentContext,
    lowering: &LoweringContext,
    ts_init: UnixNanos,
) -> Result<RuntimeAction, LoweringError> {
    lower_intent_with_id(intent, None, context, lowering, ts_init)
}

/// Translate a [`PlannedIntent`] into a [`RuntimeAction`].
///
/// Preserves `intent_id` as correlation metadata on lowered runtime
/// actions and command params.
pub fn lower_planned_intent(
    planned_intent: &PlannedIntent,
    context: &AgentContext,
    lowering: &LoweringContext,
    ts_init: UnixNanos,
) -> Result<RuntimeAction, LoweringError> {
    lower_intent_with_id(
        &planned_intent.intent,
        Some(planned_intent.intent_id),
        context,
        lowering,
        ts_init,
    )
}

fn lower_intent_with_id(
    intent: &AgentIntent,
    intent_id: Option<UUID4>,
    context: &AgentContext,
    lowering: &LoweringContext,
    ts_init: UnixNanos,
) -> Result<RuntimeAction, LoweringError> {
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
                params: command_params(intent_id),
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
                params: command_params(intent_id),
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
            intent_id,
        })),

        AgentIntent::AbortBacktest { run_id } => {
            Ok(RuntimeAction::Research(ResearchCommand::CancelBacktest {
                run_id: run_id.clone(),
                intent_id,
            }))
        }

        AgentIntent::AdjustParameters {
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
            ..
        } => Ok(RuntimeAction::Research(ResearchCommand::RunBacktest {
            instrument_id: *instrument_id,
            catalog_path: catalog_path.clone(),
            data_cls: data_cls.clone(),
            bar_spec: bar_spec.clone(),
            start_ns: *start_ns,
            end_ns: *end_ns,
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
    intent_id: Option<UUID4>,
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
        params: command_params(intent_id),
    }
}

fn command_params(intent_id: Option<UUID4>) -> Option<Params> {
    let intent_id = intent_id?;

    let mut params = Params::new();
    params.insert(
        INTENT_ID_PARAM_KEY.to_string(),
        json!(intent_id.to_string()),
    );
    Some(params)
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
