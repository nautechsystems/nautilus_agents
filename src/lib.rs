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

//! Agent protocol for [NautilusTrader](https://nautilustrader.io).
//!
//! This crate provides the types, traits, and contracts for building
//! autonomous trading agents on top of the Nautilus trading engine.
//!
//! **Status**: early development. The API is not yet stable.

pub mod action;
pub mod capability;
pub mod context;
pub mod envelope;
pub mod guardrail;
pub mod guardrails;
pub mod intent;
pub mod lowering;
pub mod pipeline;
pub mod policy;
pub mod recording;
pub mod replay;

#[cfg(test)]
pub(crate) mod fixtures;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod prelude {
    pub use nautilus_core::{UUID4, UnixNanos};
    pub use nautilus_model::{
        data::{Bar, QuoteTick},
        enums::{OrderSide, OrderType, PositionSide},
        events::{AccountState, OrderSnapshot, PositionSnapshot},
        identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId},
        reports::PositionStatusReport,
        types::{Currency, Money, Price, Quantity},
    };

    pub use crate::{
        action::{ManagementCommand, ResearchCommand, RuntimeAction, TradeAction},
        capability::{ActionCapability, CapabilitySet, ObservationCapability},
        context::{AgentContext, ContextError},
        envelope::{
            DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION, GuardrailResult,
            LoweringOutcome, PlannedIntentOutcome, ReconciliationOutcome,
        },
        guardrail::{ActionGuardrail, IntentGuardrail},
        guardrails::{
            max_drawdown::MaxDrawdownGuardrail, order_rate::OrderRateGuardrail,
            position_limit::PositionLimitGuardrail,
        },
        intent::{AgentIntent, EscalationSeverity, ExecutionConstraints},
        lowering::{LoweringContext, LoweringError, lower_planned_intent},
        pipeline::DecisionPipeline,
        policy::{AgentPolicy, PlannedIntent, PolicyDecision, PolicyError, PolicyFuture},
        recording::{DecisionRecorder, RecordingError},
        replay::{ReplayConfig, ReplayError, ReplayResult, ReplayRunner, read_envelopes},
    };
}
