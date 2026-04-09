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

//! Drawdown circuit breaker.
//!
//! Rejects all intents except management commands when the account
//! balance has drawn down beyond a configured percentage from a
//! reference balance. Management intents ([`PauseStrategy`],
//! [`EscalateToHuman`], etc.) pass through so the agent can still
//! respond to the drawdown.

use nautilus_model::types::Money;

use crate::{
    context::AgentContext, envelope::GuardrailResult, guardrail::IntentGuardrail,
    intent::AgentIntent,
};

/// Rejects non-management intents when drawdown exceeds
/// `max_drawdown_pct` relative to `reference_balance`.
///
/// If the context has no account state or no matching currency
/// balance, the guardrail approves (no data to evaluate against).
pub struct MaxDrawdownGuardrail {
    pub reference_balance: Money,
    pub max_drawdown_pct: f64,
}

impl MaxDrawdownGuardrail {
    pub fn new(reference_balance: Money, max_drawdown_pct: f64) -> Self {
        Self {
            reference_balance,
            max_drawdown_pct,
        }
    }
}

impl IntentGuardrail for MaxDrawdownGuardrail {
    fn evaluate(&self, intent: &AgentIntent, context: &AgentContext) -> GuardrailResult {
        // Management intents always pass so the agent can respond
        // to the drawdown (pause strategies, escalate to human).
        match intent {
            AgentIntent::PauseStrategy { .. }
            | AgentIntent::ResumeStrategy { .. }
            | AgentIntent::AdjustRiskLimits { .. }
            | AgentIntent::EscalateToHuman { .. } => return GuardrailResult::Approved,
            _ => {}
        }

        let Some(account_state) = &context.account_state else {
            return GuardrailResult::Approved;
        };

        let current_total = account_state
            .balances
            .iter()
            .find(|b| b.currency == self.reference_balance.currency)
            .map(|b| b.total.as_f64());

        let Some(current) = current_total else {
            return GuardrailResult::Approved;
        };

        let reference = self.reference_balance.as_f64();
        if reference <= 0.0 {
            return GuardrailResult::Approved;
        }

        let drawdown_pct = (reference - current) / reference;
        if drawdown_pct > self.max_drawdown_pct {
            return GuardrailResult::Rejected {
                reason: format!(
                    "drawdown {:.2}% exceeds limit {:.2}%",
                    drawdown_pct * 100.0,
                    self.max_drawdown_pct * 100.0,
                ),
            };
        }

        GuardrailResult::Approved
    }
}
