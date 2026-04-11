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

//! Runtime actions produced by lowering agent intents.
//!
//! [`RuntimeAction`] branches by operational mode: [`TradeAction`] wraps
//! concrete trading command structs from `nautilus-common`,
//! [`ResearchCommand`] carries backtest configuration, and
//! [`ManagementCommand`] covers strategy lifecycle and escalation.

use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{InstrumentId, StrategyId};
use serde::{Deserialize, Serialize};

use crate::intent::EscalationSeverity;

/// Branches by operational mode after lowering an
/// [`AgentIntent`](crate::intent::AgentIntent).
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", content = "command")]
pub enum RuntimeAction {
    Trade(Box<TradeAction>),
    Research(ResearchCommand),
    Management(ManagementCommand),
}

/// Wraps individual command structs from `nautilus-common` (all
/// `Serialize`/`Deserialize`). Omits `QueryOrder` and `QueryAccount`
/// because queries are observations, not actions.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "payload")]
pub enum TradeAction {
    SubmitOrder(Box<SubmitOrder>),
    SubmitOrderList(SubmitOrderList),
    ModifyOrder(ModifyOrder),
    CancelOrder(CancelOrder),
    CancelAllOrders(CancelAllOrders),
    BatchCancelOrders(BatchCancelOrders),
}

/// Executable research commands. Workflow actions like SaveCandidate
/// and RejectHypothesis stay in the intent layer.
/// AdjustParameters lowers into a new RunBacktest configuration.
///
/// `intent_id` is correlation metadata that participates in
/// `PartialEq`. Replay-level comparison that ignores `intent_id`
/// lives in `replay.rs` because replay is the one caller that needs
/// to treat freshly-minted UUIDs as equivalent.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResearchCommand {
    RunBacktest {
        instrument_id: InstrumentId,
        catalog_path: String,
        data_cls: String,
        bar_spec: Option<String>,
        start_ns: Option<UnixNanos>,
        end_ns: Option<UnixNanos>,
        intent_id: UUID4,
    },
    CancelBacktest {
        run_id: String,
        intent_id: UUID4,
    },
    GetBacktestStatus {
        run_id: String,
        intent_id: UUID4,
    },
    GetBacktestResult {
        run_id: String,
        intent_id: UUID4,
    },
    CompareBacktests {
        run_ids: Vec<String>,
        intent_id: UUID4,
    },
}

/// Strategy lifecycle, risk adjustment, and escalation commands.
/// The server forwards these to the engine or notification system.
///
/// `intent_id` is correlation metadata that participates in
/// `PartialEq`. Replay-level comparison that ignores `intent_id`
/// lives in `replay.rs` because replay is the one caller that needs
/// to treat freshly-minted UUIDs as equivalent.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ManagementCommand {
    PauseStrategy {
        strategy_id: StrategyId,
        intent_id: UUID4,
    },
    ResumeStrategy {
        strategy_id: StrategyId,
        intent_id: UUID4,
    },
    AdjustRiskLimits {
        params: serde_json::Value,
        intent_id: UUID4,
    },
    EscalateToHuman {
        reason: String,
        severity: EscalationSeverity,
        intent_id: UUID4,
    },
}
