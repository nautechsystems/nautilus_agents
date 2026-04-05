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
    reports::PositionStatusReport,
};
use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;

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
