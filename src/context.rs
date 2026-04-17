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

//! Agent context: the bounded, owned snapshot of engine state that
//! a policy receives as input.
//!
//! All fields are scoped by the [`CapabilitySet`]: only data the agent
//! is allowed to observe appears here. Fields not granted are empty
//! `Vec`s or `None`.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, QuoteTick},
    events::{AccountState, OrderSnapshot, PositionSnapshot},
    identifiers::InstrumentId,
    reports::PositionStatusReport,
};
use serde::{Deserialize, Serialize};

use crate::capability::{CapabilitySet, ObservationCapability};

/// Returned from [`AgentContext::validate`] and [`AgentContext::new`]
/// so callers can catch contract drift at construction time rather
/// than inside the pipeline.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContextError {
    #[error("observation capability {required:?} not granted but field is populated")]
    ObservationDenied { required: ObservationCapability },
    #[error("instrument {instrument_id} populated in context but not in capability scope")]
    InstrumentOutOfScope { instrument_id: InstrumentId },
}

/// Owned snapshots for recording, replay, and async policy paths.
/// The `capabilities` field records what scoped this context, enabling
/// replay fidelity checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentContext {
    pub ts_context: UnixNanos,
    pub capabilities: CapabilitySet,
    pub quotes: Vec<QuoteTick>,
    pub bars: Vec<Bar>,
    pub account_state: Option<AccountState>,
    pub positions: Vec<PositionSnapshot>,
    pub orders: Vec<OrderSnapshot>,
    pub position_reports: Vec<PositionStatusReport>,
}

impl AgentContext {
    /// Construct a context and validate it against its `capabilities`.
    ///
    /// Returns [`ContextError`] if any populated field lacks the
    /// matching [`ObservationCapability`]. Use the struct literal
    /// directly when the caller has already validated inputs.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ts_context: UnixNanos,
        capabilities: CapabilitySet,
        quotes: Vec<QuoteTick>,
        bars: Vec<Bar>,
        account_state: Option<AccountState>,
        positions: Vec<PositionSnapshot>,
        orders: Vec<OrderSnapshot>,
        position_reports: Vec<PositionStatusReport>,
    ) -> Result<Self, ContextError> {
        let ctx = Self {
            ts_context,
            capabilities,
            quotes,
            bars,
            account_state,
            positions,
            orders,
            position_reports,
        };
        ctx.validate()?;
        Ok(ctx)
    }

    /// Confirm every populated field has a matching granted capability
    /// and that every instrument-bearing item falls within the
    /// capability set's `instrument_scope`.
    ///
    /// A missing capability with an empty field is not an error: the
    /// runtime may withhold data it is allowed to produce. Only the
    /// converse (data present without the capability, or data for an
    /// instrument outside scope) is a contract violation.
    pub fn validate(&self) -> Result<(), ContextError> {
        let checks: [(ObservationCapability, bool); 6] = [
            (ObservationCapability::Quotes, !self.quotes.is_empty()),
            (ObservationCapability::Bars, !self.bars.is_empty()),
            (
                ObservationCapability::AccountState,
                self.account_state.is_some(),
            ),
            (ObservationCapability::Positions, !self.positions.is_empty()),
            (ObservationCapability::Orders, !self.orders.is_empty()),
            (
                ObservationCapability::PositionReports,
                !self.position_reports.is_empty(),
            ),
        ];
        for (cap, populated) in checks {
            if populated && !self.capabilities.can_observe(cap) {
                return Err(ContextError::ObservationDenied { required: cap });
            }
        }

        let quote_ids = self.quotes.iter().map(|q| q.instrument_id);
        let bar_ids = self.bars.iter().map(|b| b.instrument_id());
        let position_ids = self.positions.iter().map(|p| p.instrument_id);
        let order_ids = self.orders.iter().map(|o| o.instrument_id);
        let report_ids = self.position_reports.iter().map(|r| r.instrument_id);
        for id in quote_ids
            .chain(bar_ids)
            .chain(position_ids)
            .chain(order_ids)
            .chain(report_ids)
        {
            if !self.capabilities.instrument_allowed(&id) {
                return Err(ContextError::InstrumentOutOfScope { instrument_id: id });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::QuoteTick,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        capability::{CapabilitySet, ObservationCapability},
        fixtures::{test_context, test_instrument_id},
    };

    #[rstest]
    fn test_agent_context_round_trip() {
        let ctx = test_context();
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: AgentContext = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.quotes.len(), 1);
        assert_eq!(restored.quotes[0].instrument_id, test_instrument_id());
    }

    #[rstest]
    fn test_agent_context_validate_approves_scoped_data() {
        let ctx = test_context();
        assert!(ctx.validate().is_ok());
    }

    #[rstest]
    fn test_agent_context_validate_rejects_ungranted_field() {
        let mut ctx = test_context();
        ctx.capabilities
            .observations
            .remove(&ObservationCapability::Quotes);
        match ctx.validate().unwrap_err() {
            ContextError::ObservationDenied { required } => {
                assert_eq!(required, ObservationCapability::Quotes);
            }
            other => panic!("expected ObservationDenied, got {other:?}"),
        }
    }

    #[rstest]
    fn test_agent_context_validate_rejects_out_of_scope_instrument() {
        let mut ctx = test_context();
        let off_scope = InstrumentId::from("ETHUSDT.BINANCE");
        ctx.quotes = vec![QuoteTick::new(
            off_scope,
            Price::from("3000.00"),
            Price::from("3000.50"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        )];
        match ctx.validate().unwrap_err() {
            ContextError::InstrumentOutOfScope { instrument_id } => {
                assert_eq!(instrument_id, off_scope);
            }
            other => panic!("expected InstrumentOutOfScope, got {other:?}"),
        }
    }

    #[rstest]
    fn test_agent_context_new_validates() {
        let err = AgentContext::new(
            UnixNanos::from(1u64),
            CapabilitySet {
                observations: BTreeSet::new(),
                actions: BTreeSet::new(),
                instrument_scope: BTreeSet::new(),
            },
            test_context().quotes,
            vec![],
            None,
            vec![],
            vec![],
            vec![],
        )
        .unwrap_err();
        match err {
            ContextError::ObservationDenied { required } => {
                assert_eq!(required, ObservationCapability::Quotes);
            }
            other => panic!("expected ObservationDenied, got {other:?}"),
        }
    }
}
