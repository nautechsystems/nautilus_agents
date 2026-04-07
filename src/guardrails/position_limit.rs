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
            AgentIntent::ReducePosition { quantity, .. } => {
                if *quantity > self.max_order_quantity {
                    return GuardrailResult::Rejected {
                        reason: format!(
                            "order quantity {} exceeds max_order_quantity {}",
                            quantity, self.max_order_quantity
                        ),
                    };
                }
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
