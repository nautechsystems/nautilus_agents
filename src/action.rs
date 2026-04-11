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
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResearchCommand {
    RunBacktest {
        instrument_id: InstrumentId,
        catalog_path: String,
        data_cls: String,
        bar_spec: Option<String>,
        start_ns: Option<UnixNanos>,
        end_ns: Option<UnixNanos>,
        intent_id: Option<UUID4>,
    },
    CancelBacktest {
        run_id: String,
        intent_id: Option<UUID4>,
    },
    GetBacktestStatus {
        run_id: String,
        intent_id: Option<UUID4>,
    },
    GetBacktestResult {
        run_id: String,
        intent_id: Option<UUID4>,
    },
    CompareBacktests {
        run_ids: Vec<String>,
        intent_id: Option<UUID4>,
    },
}

/// Semantic equality ignores `intent_id`.
impl PartialEq for ResearchCommand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::RunBacktest {
                    instrument_id: instrument_id_a,
                    catalog_path: catalog_path_a,
                    data_cls: data_cls_a,
                    bar_spec: bar_spec_a,
                    start_ns: start_ns_a,
                    end_ns: end_ns_a,
                    ..
                },
                Self::RunBacktest {
                    instrument_id: instrument_id_b,
                    catalog_path: catalog_path_b,
                    data_cls: data_cls_b,
                    bar_spec: bar_spec_b,
                    start_ns: start_ns_b,
                    end_ns: end_ns_b,
                    ..
                },
            ) => {
                instrument_id_a == instrument_id_b
                    && catalog_path_a == catalog_path_b
                    && data_cls_a == data_cls_b
                    && bar_spec_a == bar_spec_b
                    && start_ns_a == start_ns_b
                    && end_ns_a == end_ns_b
            }
            (
                Self::CancelBacktest {
                    run_id: run_id_a, ..
                },
                Self::CancelBacktest {
                    run_id: run_id_b, ..
                },
            ) => run_id_a == run_id_b,
            (
                Self::GetBacktestStatus {
                    run_id: run_id_a, ..
                },
                Self::GetBacktestStatus {
                    run_id: run_id_b, ..
                },
            ) => run_id_a == run_id_b,
            (
                Self::GetBacktestResult {
                    run_id: run_id_a, ..
                },
                Self::GetBacktestResult {
                    run_id: run_id_b, ..
                },
            ) => run_id_a == run_id_b,
            (
                Self::CompareBacktests {
                    run_ids: run_ids_a, ..
                },
                Self::CompareBacktests {
                    run_ids: run_ids_b, ..
                },
            ) => run_ids_a == run_ids_b,
            (Self::RunBacktest { .. }, _)
            | (Self::CancelBacktest { .. }, _)
            | (Self::GetBacktestStatus { .. }, _)
            | (Self::GetBacktestResult { .. }, _)
            | (Self::CompareBacktests { .. }, _) => false,
        }
    }
}

/// Strategy lifecycle, risk adjustment, and escalation commands.
/// The server forwards these to the engine or notification system.
#[non_exhaustive]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ManagementCommand {
    PauseStrategy {
        strategy_id: StrategyId,
        intent_id: Option<UUID4>,
    },
    ResumeStrategy {
        strategy_id: StrategyId,
        intent_id: Option<UUID4>,
    },
    AdjustRiskLimits {
        params: serde_json::Value,
        intent_id: Option<UUID4>,
    },
    EscalateToHuman {
        reason: String,
        severity: EscalationSeverity,
        intent_id: Option<UUID4>,
    },
}

/// Semantic equality ignores `intent_id`.
impl PartialEq for ManagementCommand {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::PauseStrategy {
                    strategy_id: strategy_id_a,
                    ..
                },
                Self::PauseStrategy {
                    strategy_id: strategy_id_b,
                    ..
                },
            ) => strategy_id_a == strategy_id_b,
            (
                Self::ResumeStrategy {
                    strategy_id: strategy_id_a,
                    ..
                },
                Self::ResumeStrategy {
                    strategy_id: strategy_id_b,
                    ..
                },
            ) => strategy_id_a == strategy_id_b,
            (
                Self::AdjustRiskLimits {
                    params: params_a, ..
                },
                Self::AdjustRiskLimits {
                    params: params_b, ..
                },
            ) => params_a == params_b,
            (
                Self::EscalateToHuman {
                    reason: reason_a,
                    severity: severity_a,
                    ..
                },
                Self::EscalateToHuman {
                    reason: reason_b,
                    severity: severity_b,
                    ..
                },
            ) => reason_a == reason_b && severity_a == severity_b,
            (Self::PauseStrategy { .. }, _)
            | (Self::ResumeStrategy { .. }, _)
            | (Self::AdjustRiskLimits { .. }, _)
            | (Self::EscalateToHuman { .. }, _) => false,
        }
    }
}
