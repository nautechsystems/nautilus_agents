//! Retention-aware JSONL recording for agent-side evidence.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    assurance::trace::AgentTrace,
    protocol::observation::{Observation, ObservationRef, RetentionClass},
};

/// Selects how observation data is captured by a trace recorder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObservationCapture {
    /// Records only the observation identity and digest.
    #[default]
    ReferenceOnly,
    /// Records a separately validated transformation supplied by the caller.
    Redacted,
    /// Records the full observation unless its retention class forbids it.
    Full,
}

/// Configures observation capture for a trace recorder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecorderConfig {
    /// The explicit observation capture mode.
    pub observation_capture: ObservationCapture,
}

/// Transforms an observation into a separately validated recordable observation.
pub trait ObservationRedactor: Send + Sync {
    /// Produces a redacted observation or a human-readable local error.
    fn redact(&self, observation: &Observation) -> Result<Observation, String>;
}

/// Reports an agent-side recording failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecordingError {
    /// A filesystem operation failed before the target could be replaced.
    #[error("recording I/O failed ({kind:?}): {message}")]
    Io {
        /// The stable standard-library I/O category.
        kind: io::ErrorKind,
        /// A human-readable local explanation.
        message: String,
    },
    /// A record could not be encoded as JSON.
    #[error("record serialization failed: {message}")]
    Serialization {
        /// A human-readable serialization explanation.
        message: String,
    },
    /// Redacted capture was selected without a redactor.
    #[error("redacted observation capture requires an ObservationRedactor")]
    RedactorRequired,
    /// The caller-supplied redactor failed.
    #[error("observation redaction failed: {message}")]
    Redaction {
        /// The caller-supplied redaction explanation.
        message: String,
    },
    /// The caller-supplied redacted observation failed protocol validation.
    #[error("redacted observation is invalid: {message}")]
    InvalidRedaction {
        /// The protocol validation explanation.
        message: String,
    },
    /// The selected capture mode would retain restricted observation data.
    #[error("{capture:?} capture cannot record {retention:?} observation data")]
    ForbiddenRetention {
        /// The selected capture mode.
        capture: ObservationCapture,
        /// The observation retention class.
        retention: RetentionClass,
    },
}

/// Records agent traces and explicitly selected observation data as JSONL.
///
/// Use one recorder per path. Concurrent recorders are not coordinated and may overwrite each
/// other's latest append.
pub struct TraceRecorder {
    path: PathBuf,
    config: RecorderConfig,
    redactor: Option<Box<dyn ObservationRedactor>>,
}

impl TraceRecorder {
    /// Creates a recorder without an observation redactor.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, config: RecorderConfig) -> Self {
        Self {
            path: path.into(),
            config,
            redactor: None,
        }
    }

    /// Creates a recorder with a caller-supplied observation redactor.
    #[must_use]
    pub fn with_redactor<R>(path: impl Into<PathBuf>, config: RecorderConfig, redactor: R) -> Self
    where
        R: ObservationRedactor + 'static,
    {
        Self {
            path: path.into(),
            config,
            redactor: Some(Box::new(redactor)),
        }
    }

    /// Appends one complete agent trace record.
    pub fn record_trace(&self, trace: &AgentTrace) -> Result<(), RecordingError> {
        self.record(&JsonlRecord::Trace { trace })
    }

    /// Appends one observation record using the configured capture mode.
    pub fn record_observation(&self, observation: &Observation) -> Result<(), RecordingError> {
        match self.config.observation_capture {
            ObservationCapture::ReferenceOnly => self.record(&JsonlRecord::ObservationReference {
                reference: observation.reference(),
            }),
            ObservationCapture::Redacted => {
                let redactor = self
                    .redactor
                    .as_ref()
                    .ok_or(RecordingError::RedactorRequired)?;
                let redacted = redactor
                    .redact(observation)
                    .map_err(|message| RecordingError::Redaction { message })?;
                redacted
                    .validate()
                    .map_err(|e| RecordingError::InvalidRedaction {
                        message: e.to_string(),
                    })?;

                if redacted.retention == RetentionClass::Restricted {
                    return Err(RecordingError::ForbiddenRetention {
                        capture: ObservationCapture::Redacted,
                        retention: redacted.retention,
                    });
                }
                self.record(&JsonlRecord::Observation {
                    reference: observation.reference(),
                    observation: &redacted,
                })
            }
            ObservationCapture::Full => {
                if observation.retention == RetentionClass::Restricted {
                    return Err(RecordingError::ForbiddenRetention {
                        capture: ObservationCapture::Full,
                        retention: observation.retention,
                    });
                }
                self.record(&JsonlRecord::Observation {
                    reference: observation.reference(),
                    observation,
                })
            }
        }
    }

    fn record<T: Serialize>(&self, record: &T) -> Result<(), RecordingError> {
        let mut line = serde_json::to_vec(record).map_err(|e| RecordingError::Serialization {
            message: e.to_string(),
        })?;
        line.push(b'\n');
        replace_with_appended_record(&self.path, &line)
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JsonlRecord<'a> {
    Trace {
        trace: &'a AgentTrace,
    },
    ObservationReference {
        reference: ObservationRef,
    },
    Observation {
        reference: ObservationRef,
        observation: &'a Observation,
    },
}

fn replace_with_appended_record(path: &Path, line: &[u8]) -> Result<(), RecordingError> {
    let existing = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(io_error(&e)),
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| RecordingError::Io {
            kind: io::ErrorKind::InvalidInput,
            message: "recording path has no UTF-8 file name".to_owned(),
        })?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|e| io_error(&e))?;
        file.write_all(&existing).map_err(|e| io_error(&e))?;
        file.write_all(line).map_err(|e| io_error(&e))?;
        file.sync_all().map_err(|e| io_error(&e))?;
        fs::rename(&temporary, path).map_err(|e| io_error(&e))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn io_error(e: &io::Error) -> RecordingError {
    RecordingError::Io {
        kind: e.kind(),
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::Serializer;

    use super::*;

    struct FailingRecord;

    impl Serialize for FailingRecord {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(serde::ser::Error::custom(
                "deliberate serialization failure",
            ))
        }
    }

    #[rstest]
    fn test_serialization_failure_does_not_create_target() {
        let path = std::env::temp_dir().join(format!(
            "nautilus-agents-serialization-{}.jsonl",
            uuid::Uuid::new_v4()
        ));
        let recorder = TraceRecorder::new(&path, RecorderConfig::default());

        let error = recorder.record(&FailingRecord).unwrap_err();

        assert_eq!(
            error,
            RecordingError::Serialization {
                message: "deliberate serialization failure".to_owned(),
            }
        );
        assert!(!path.exists());
    }
}
