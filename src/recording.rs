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

//! Decision recording: line-delimited JSON persistence for
//! [`DecisionEnvelope`]s.
//!
//! Each envelope is serialized as a single JSON line and appended to
//! a file. The recorder holds the file handle open for the lifetime
//! of the instance and wraps it in a [`BufWriter`] so live loops avoid
//! reopening the file on every cycle. This is the v0 recording format;
//! future versions may use Cap'n Proto or a columnar format.

use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use crate::envelope::DecisionEnvelope;

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("serialization failed: {message}")]
    Serialization { message: String },
    #[error("I/O error: {message}")]
    Io { message: String },
}

/// Appends [`DecisionEnvelope`]s as line-delimited JSON to a file.
///
/// Holds an open, buffered file handle. Each [`record`](Self::record)
/// call writes one line and flushes so readers see the envelope
/// immediately. Dropping the recorder flushes and closes the file.
pub struct DecisionRecorder {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl DecisionRecorder {
    /// Open `path` in append mode (creating it if absent) and wrap it
    /// in a buffered writer.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, RecordingError> {
        let path = path.into();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| RecordingError::Io {
                message: e.to_string(),
            })?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize the envelope to JSON, append it as a single line, and
    /// flush so readers observe it immediately.
    pub fn record(&mut self, envelope: &DecisionEnvelope) -> Result<(), RecordingError> {
        let json = serde_json::to_string(envelope).map_err(|e| RecordingError::Serialization {
            message: e.to_string(),
        })?;

        writeln!(self.writer, "{json}").map_err(|e| RecordingError::Io {
            message: e.to_string(),
        })?;

        self.writer.flush().map_err(|e| RecordingError::Io {
            message: e.to_string(),
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use rstest::rstest;

    use super::*;
    use crate::{
        envelope::{DecisionEnvelope, DecisionTrigger, ENVELOPE_SCHEMA_VERSION},
        fixtures::test_context,
        policy::PolicyDecision,
    };

    #[rstest]
    fn test_decision_recorder_writes_json_lines() {
        let dir = std::env::temp_dir().join(format!("nautilus_test_{}", UUID4::new()));
        let path = dir.join("decisions.jsonl");
        std::fs::create_dir_all(&dir).unwrap();

        let mut recorder = DecisionRecorder::new(&path).unwrap();

        let envelope1 = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Timer {
                interval_ns: 60_000_000_000,
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_000_000_000_000u64),
            ts_reconciled: None,
        };

        let envelope2 = DecisionEnvelope {
            envelope_id: UUID4::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            trigger: DecisionTrigger::Manual {
                reason: "test".to_string(),
            },
            context: test_context(),
            decision: PolicyDecision::NoAction,
            outcome: None,
            reconciliation: None,
            ts_created: UnixNanos::from(1_712_400_001_000_000_000u64),
            ts_reconciled: None,
        };

        recorder.record(&envelope1).unwrap();
        recorder.record(&envelope2).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let restored1: DecisionEnvelope = serde_json::from_str(lines[0]).unwrap();
        let restored2: DecisionEnvelope = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(restored1.envelope_id, envelope1.envelope_id);
        assert_eq!(restored2.envelope_id, envelope2.envelope_id);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
