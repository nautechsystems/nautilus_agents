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
//! concrete trading command structs from `nautilus-common` (which implement
//! `Serialize`), and [`ResearchCommand`] provides stubs for research mode.

use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder, SubmitOrderList,
};
use serde::{Deserialize, Serialize};

/// Branches by operational mode after lowering an
/// [`AgentIntent`](crate::intent::AgentIntent).
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", content = "command")]
pub enum RuntimeAction {
    Trade(Box<TradeAction>),
    Research(ResearchCommand),
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

/// Executable research commands (stubs for v0). Workflow actions like
/// SaveCandidate and RejectHypothesis stay in the intent layer.
/// AdjustParameters lowers into a new RunBacktest configuration.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResearchCommand {
    RunBacktest,
    CancelBacktest,
    GetBacktestStatus,
    GetBacktestResult,
    CompareBacktests,
}
