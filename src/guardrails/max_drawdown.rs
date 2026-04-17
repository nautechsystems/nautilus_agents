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

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::StrategyId;
    use rstest::rstest;

    use super::*;
    use crate::{
        fixtures::{test_account_state, test_context, test_currency, test_intent},
        intent::EscalationSeverity,
    };

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
            severity: EscalationSeverity::Critical,
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
}
