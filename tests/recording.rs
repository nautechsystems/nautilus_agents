use std::{fs, io, path::PathBuf};

use nautilus_agents::{
    assurance::{
        recording::{
            ObservationCapture, ObservationRedactor, RecorderConfig, RecordingError, TraceRecorder,
        },
        trace::AgentTrace,
    },
    protocol::{
        observation::{
            FieldOmission, Observation, ObservationPayload, OmissionReason, RetentionClass,
        },
        value::FieldPath,
    },
};
use serde_json::{Value, json};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nautilus-agents-recording-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct SafeRedactor;

impl ObservationRedactor for SafeRedactor {
    fn redact(&self, observation: &Observation) -> Result<Observation, String> {
        let mut redacted = observation.clone();
        let ObservationPayload::Live(live) = &mut redacted.payload;
        live.positions.clear();
        redacted.omissions.push(FieldOmission {
            field: FieldPath::parse("/payload/live/positions").unwrap(),
            reason: OmissionReason::Redacted,
        });
        redacted.retention = RetentionClass::Derived;
        redacted.refresh_digest().unwrap();
        Ok(redacted)
    }
}

struct FailingRedactor;

impl ObservationRedactor for FailingRedactor {
    fn redact(&self, _observation: &Observation) -> Result<Observation, String> {
        Err("redaction policy failed".to_owned())
    }
}

struct InvalidRedactor;

impl ObservationRedactor for InvalidRedactor {
    fn redact(&self, observation: &Observation) -> Result<Observation, String> {
        let mut invalid = observation.clone();
        invalid.retention = RetentionClass::ReferenceOnly;
        Ok(invalid)
    }
}

struct RestrictedRedactor;

impl ObservationRedactor for RestrictedRedactor {
    fn redact(&self, observation: &Observation) -> Result<Observation, String> {
        let mut restricted = observation.clone();
        restricted.retention = RetentionClass::Restricted;
        restricted.refresh_digest().unwrap();
        Ok(restricted)
    }
}

#[test]
fn test_default_records_reference_without_payload() {
    let directory = TestDirectory::new();
    let path = directory.path("reference.jsonl");
    let observation = observation();
    let recorder = TraceRecorder::new(&path, RecorderConfig::default());

    recorder.record_observation(&observation).unwrap();

    assert_eq!(
        records(&path),
        vec![json!({
            "kind": "observation_reference",
            "reference": observation.reference(),
        })]
    );
}

#[test]
fn test_trace_and_full_observation_use_separate_record_kinds() {
    let directory = TestDirectory::new();
    let path = directory.path("full.jsonl");
    let observation = observation();
    let trace = trace();
    let recorder = TraceRecorder::new(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Full,
        },
    );

    recorder.record_trace(&trace).unwrap();
    recorder.record_observation(&observation).unwrap();

    assert_eq!(
        records(&path),
        vec![
            json!({"kind": "trace", "trace": trace}),
            json!({
                "kind": "observation",
                "reference": observation.reference(),
                "observation": observation,
            }),
        ]
    );
}

#[test]
fn test_redacted_capture_records_only_validated_redactor_output() {
    let directory = TestDirectory::new();
    let path = directory.path("redacted.jsonl");
    let observation = observation();
    let redacted = SafeRedactor.redact(&observation).unwrap();
    let recorder = TraceRecorder::with_redactor(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Redacted,
        },
        SafeRedactor,
    );

    recorder.record_observation(&observation).unwrap();

    assert_eq!(
        records(&path),
        vec![json!({
            "kind": "observation",
            "reference": observation.reference(),
            "observation": redacted,
        })]
    );
}

#[test]
fn test_redaction_failures_leave_existing_record_unchanged() {
    let directory = TestDirectory::new();
    let path = directory.path("redaction-failure.jsonl");
    let observation = observation();
    let trace = trace();
    let initial = TraceRecorder::new(&path, RecorderConfig::default());
    initial.record_trace(&trace).unwrap();
    let before = fs::read(&path).unwrap();

    let missing = TraceRecorder::new(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Redacted,
        },
    );
    assert_eq!(
        missing.record_observation(&observation).unwrap_err(),
        RecordingError::RedactorRequired,
    );
    assert_eq!(fs::read(&path).unwrap(), before);

    let failing = TraceRecorder::with_redactor(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Redacted,
        },
        FailingRedactor,
    );
    assert_eq!(
        failing.record_observation(&observation).unwrap_err(),
        RecordingError::Redaction {
            message: "redaction policy failed".to_owned(),
        }
    );
    assert_eq!(fs::read(&path).unwrap(), before);

    let invalid = TraceRecorder::with_redactor(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Redacted,
        },
        InvalidRedactor,
    );
    assert_eq!(
        invalid.record_observation(&observation).unwrap_err(),
        RecordingError::InvalidRedaction {
            message: "observation digest mismatch".to_owned(),
        }
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn test_restricted_full_capture_leaves_existing_record_unchanged() {
    let directory = TestDirectory::new();
    let path = directory.path("restricted.jsonl");
    let mut observation = observation();
    observation.retention = RetentionClass::Restricted;
    observation.refresh_digest().unwrap();
    let trace = trace();
    let recorder = TraceRecorder::new(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Full,
        },
    );
    recorder.record_trace(&trace).unwrap();
    let before = fs::read(&path).unwrap();

    let error = recorder.record_observation(&observation).unwrap_err();

    assert_eq!(
        error,
        RecordingError::ForbiddenRetention {
            capture: ObservationCapture::Full,
            retention: RetentionClass::Restricted,
        }
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn test_restricted_redactor_output_leaves_existing_record_unchanged() {
    let directory = TestDirectory::new();
    let path = directory.path("restricted-redacted.jsonl");
    let observation = observation();
    let trace = trace();
    let initial = TraceRecorder::new(&path, RecorderConfig::default());
    initial.record_trace(&trace).unwrap();
    let before = fs::read(&path).unwrap();
    let recorder = TraceRecorder::with_redactor(
        &path,
        RecorderConfig {
            observation_capture: ObservationCapture::Redacted,
        },
        RestrictedRedactor,
    );

    let error = recorder.record_observation(&observation).unwrap_err();

    assert_eq!(
        error,
        RecordingError::ForbiddenRetention {
            capture: ObservationCapture::Redacted,
            retention: RetentionClass::Restricted,
        }
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn test_io_failure_leaves_no_partial_record() {
    let directory = TestDirectory::new();
    let recorder = TraceRecorder::new(&directory.0, RecorderConfig::default());

    let error = recorder.record_trace(&trace()).unwrap_err();

    let RecordingError::Io { kind, message } = error else {
        panic!("expected an I/O recording error");
    };
    assert_eq!(kind, io::ErrorKind::IsADirectory);
    assert!(!message.is_empty());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

fn records(path: &PathBuf) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn observation() -> Observation {
    serde_json::from_slice(include_bytes!(
        "../contract/v1/fixtures/valid/full-live-observation.json"
    ))
    .unwrap()
}

fn trace() -> AgentTrace {
    serde_json::from_slice(include_bytes!(
        "../contract/v1/fixtures/valid/trace-proposal.json"
    ))
    .unwrap()
}
