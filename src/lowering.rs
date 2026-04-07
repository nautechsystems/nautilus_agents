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
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{OrderSide, OrderType, PositionSide, TimeInForce},
    events::{OrderInitialized, PositionSnapshot},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};

use crate::action::{ResearchCommand, RuntimeAction, TradeAction};
use crate::context::AgentContext;
use crate::intent::AgentIntent;

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
    match intent {
        AgentIntent::ReducePosition {
            instrument_id,
            quantity,
            ..
        } => {
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
            let submit =
                submit_order_cmd(lowering, &order_init, Some(position.position_id), ts_init);
            Ok(RuntimeAction::Trade(Box::new(TradeAction::SubmitOrder(
                Box::new(submit),
            ))))
        }

        AgentIntent::ClosePosition { instrument_id, .. } => {
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
            let submit =
                submit_order_cmd(lowering, &order_init, Some(position.position_id), ts_init);
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
                params: None,
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
                params: None,
            };
            Ok(RuntimeAction::Trade(Box::new(
                TradeAction::CancelAllOrders(cmd),
            )))
        }

        AgentIntent::PauseStrategy { .. }
        | AgentIntent::ResumeStrategy { .. }
        | AgentIntent::AdjustRiskLimits { .. }
        | AgentIntent::EscalateToHuman { .. } => Err(LoweringError::NotLowerable {
            description: intent_variant_name(intent).to_string(),
        }),

        AgentIntent::RunBacktest => Ok(RuntimeAction::Research(ResearchCommand::RunBacktest)),

        AgentIntent::AbortBacktest => Ok(RuntimeAction::Research(ResearchCommand::CancelBacktest)),

        AgentIntent::AdjustParameters => Ok(RuntimeAction::Research(ResearchCommand::RunBacktest)),

        AgentIntent::CompareResults => {
            Ok(RuntimeAction::Research(ResearchCommand::CompareBacktests))
        }

        AgentIntent::SaveCandidate | AgentIntent::RejectHypothesis => {
            Err(LoweringError::NotLowerable {
                description: intent_variant_name(intent).to_string(),
            })
        }
    }
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
        params: None,
        command_id: UUID4::new(),
        ts_init,
    }
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
        AgentIntent::RunBacktest => "RunBacktest",
        AgentIntent::AbortBacktest => "AbortBacktest",
        AgentIntent::AdjustParameters => "AdjustParameters",
        AgentIntent::CompareResults => "CompareResults",
        AgentIntent::SaveCandidate => "SaveCandidate",
        AgentIntent::RejectHypothesis => "RejectHypothesis",
    }
}
