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

//! Property-based serde round-trip tests for research types.

use nautilus_agents::{action::ResearchCommand, intent::AgentIntent};
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::InstrumentId;
use proptest::prelude::*;

fn instrument_id_strategy() -> impl Strategy<Value = InstrumentId> {
    prop_oneof![
        Just(InstrumentId::from("BTCUSDT.BINANCE")),
        Just(InstrumentId::from("ETHUSDT.BINANCE")),
        Just(InstrumentId::from("AAPL.NASDAQ")),
        Just(InstrumentId::from("ES.CME")),
    ]
}

fn optional_nanos_strategy() -> impl Strategy<Value = Option<UnixNanos>> {
    prop_oneof![
        Just(None),
        (0u64..=u64::MAX).prop_map(|n| Some(UnixNanos::from(n))),
    ]
}

fn optional_string_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), "[a-zA-Z0-9_/-]{1,50}".prop_map(Some),]
}

fn run_id_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,30}"
}

fn data_cls_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Bar".to_string()),
        Just("QuoteTick".to_string()),
        Just("TradeTick".to_string()),
    ]
}

proptest! {
    #[test]
    fn prop_run_backtest_intent_serde_round_trip(
        instrument_id in instrument_id_strategy(),
        catalog_path in "[a-zA-Z0-9_/-]{1,100}",
        data_cls in data_cls_strategy(),
        bar_spec in optional_string_strategy(),
        start_ns in optional_nanos_strategy(),
        end_ns in optional_nanos_strategy(),
    ) {
        let intent = AgentIntent::RunBacktest {
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, intent);
    }

    #[test]
    fn prop_abort_backtest_intent_serde_round_trip(
        run_id in run_id_strategy(),
    ) {
        let intent = AgentIntent::AbortBacktest { run_id };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, intent);
    }

    #[test]
    fn prop_adjust_parameters_intent_serde_round_trip(
        baseline_run_id in run_id_strategy(),
        instrument_id in instrument_id_strategy(),
        catalog_path in "[a-zA-Z0-9_/-]{1,100}",
        data_cls in data_cls_strategy(),
        bar_spec in optional_string_strategy(),
        start_ns in optional_nanos_strategy(),
        end_ns in optional_nanos_strategy(),
    ) {
        let intent = AgentIntent::AdjustParameters {
            baseline_run_id,
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, intent);
    }

    #[test]
    fn prop_compare_results_intent_serde_round_trip(
        run_ids in prop::collection::vec(run_id_strategy(), 1..5),
    ) {
        let intent = AgentIntent::CompareResults { run_ids };
        let json = serde_json::to_string(&intent).unwrap();
        let restored: AgentIntent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, intent);
    }

    #[test]
    fn prop_research_command_run_backtest_serde_round_trip(
        instrument_id in instrument_id_strategy(),
        catalog_path in "[a-zA-Z0-9_/-]{1,100}",
        data_cls in data_cls_strategy(),
        bar_spec in optional_string_strategy(),
        start_ns in optional_nanos_strategy(),
        end_ns in optional_nanos_strategy(),
    ) {
        let cmd = ResearchCommand::RunBacktest {
            instrument_id,
            catalog_path,
            data_cls,
            bar_spec,
            start_ns,
            end_ns,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: ResearchCommand = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, cmd);
    }

    #[test]
    fn prop_research_command_cancel_backtest_serde_round_trip(
        run_id in run_id_strategy(),
    ) {
        let cmd = ResearchCommand::CancelBacktest { run_id };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: ResearchCommand = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, cmd);
    }

    #[test]
    fn prop_research_command_compare_backtests_serde_round_trip(
        run_ids in prop::collection::vec(run_id_strategy(), 1..5),
    ) {
        let cmd = ResearchCommand::CompareBacktests { run_ids };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: ResearchCommand = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored, cmd);
    }
}
