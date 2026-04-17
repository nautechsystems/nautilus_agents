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

//! Capability model for the agent protocol.
//!
//! Deny-by-default: every capability must be explicitly granted. Observation
//! capabilities gate what data appears in [`AgentContext`](crate::context::AgentContext).
//! Action capabilities gate which [`AgentIntent`](crate::intent::AgentIntent) variants
//! an agent may emit.

use std::collections::BTreeSet;

use nautilus_model::identifiers::InstrumentId;
use serde::{Deserialize, Serialize};

use crate::intent::AgentIntent;

/// Each variant gates a category of data in
/// [`AgentContext`](crate::context::AgentContext).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ObservationCapability {
    Quotes,
    Bars,
    AccountState,
    Positions,
    Orders,
    PositionReports,
}

/// Each variant gates a category of
/// [`AgentIntent`](crate::intent::AgentIntent) variants.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ActionCapability {
    ManagePositions,
    ManageOrders,
    ManageStrategies,
    AdjustRisk,
    Escalate,
    Research,
}

/// Constructed by the runtime, immutable for the agent's lifetime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub observations: BTreeSet<ObservationCapability>,
    pub actions: BTreeSet<ActionCapability>,
    pub instrument_scope: BTreeSet<InstrumentId>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("action capability {required:?} not granted")]
    ActionDenied { required: ActionCapability },
    #[error("instrument {instrument_id} not in scope")]
    InstrumentDenied { instrument_id: InstrumentId },
}

impl CapabilitySet {
    pub fn can_observe(&self, cap: ObservationCapability) -> bool {
        self.observations.contains(&cap)
    }

    pub fn can_act(&self, cap: ActionCapability) -> bool {
        self.actions.contains(&cap)
    }

    pub fn instrument_allowed(&self, id: &InstrumentId) -> bool {
        self.instrument_scope.contains(id)
    }

    pub fn check_intent(&self, intent: &AgentIntent) -> Result<(), CapabilityError> {
        let (required_cap, instrument_id) = match intent {
            AgentIntent::ReducePosition { instrument_id, .. }
            | AgentIntent::ClosePosition { instrument_id, .. } => {
                (ActionCapability::ManagePositions, Some(*instrument_id))
            }

            AgentIntent::CancelOrder { instrument_id, .. }
            | AgentIntent::CancelAllOrders { instrument_id, .. } => {
                (ActionCapability::ManageOrders, Some(*instrument_id))
            }

            AgentIntent::PauseStrategy { .. } | AgentIntent::ResumeStrategy { .. } => {
                (ActionCapability::ManageStrategies, None)
            }

            AgentIntent::AdjustRiskLimits { .. } => (ActionCapability::AdjustRisk, None),

            AgentIntent::EscalateToHuman { .. } => (ActionCapability::Escalate, None),

            AgentIntent::RunBacktest { instrument_id, .. }
            | AgentIntent::AdjustParameters { instrument_id, .. } => {
                (ActionCapability::Research, Some(*instrument_id))
            }

            // run_id-based intents cannot check instrument scope statically;
            // the executor must verify scope when resolving the run_id.
            AgentIntent::AbortBacktest { .. }
            | AgentIntent::CompareResults { .. }
            | AgentIntent::SaveCandidate { .. }
            | AgentIntent::RejectHypothesis { .. } => (ActionCapability::Research, None),
        };

        if !self.can_act(required_cap) {
            return Err(CapabilityError::ActionDenied {
                required: required_cap,
            });
        }

        if let Some(id) = instrument_id
            && !self.instrument_allowed(&id)
        {
            return Err(CapabilityError::InstrumentDenied { instrument_id: id });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;
    use crate::{
        fixtures::{test_capabilities, test_instrument_id, test_intent},
        intent::AgentIntent,
    };

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
}
