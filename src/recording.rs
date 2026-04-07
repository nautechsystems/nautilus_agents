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
//! a file. This is the v0 recording format; future versions may use
//! Cap'n Proto or a columnar format.

use std::{
    fs::OpenOptions,
    io::Write,
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
pub struct DecisionRecorder {
    path: PathBuf,
}

impl DecisionRecorder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serialize the envelope to JSON and append it as a single line.
    pub fn record(&self, envelope: &DecisionEnvelope) -> Result<(), RecordingError> {
        let json = serde_json::to_string(envelope).map_err(|e| RecordingError::Serialization {
            message: e.to_string(),
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| RecordingError::Io {
                message: e.to_string(),
            })?;

        writeln!(file, "{json}").map_err(|e| RecordingError::Io {
            message: e.to_string(),
        })?;

        Ok(())
    }
}
