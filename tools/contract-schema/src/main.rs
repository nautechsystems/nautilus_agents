use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use nautilus_agents::{
    assurance::trace::{AgentFailure, AgentFailureKind, AgentTrace, PolicyMetadata, TraceOutcome},
    protocol::{
        canonical,
        error::{DecisionError, ErrorCode, ProtocolError},
        live::LiveProposalRequest,
        observation::Observation,
        receipt::{CorrelationRefs, DecisionReceipt, DecisionStatus, ProposalResponse},
        value::{ContentDigest, TimestampNs},
        version::PROTOCOL_VERSION,
    },
    testing,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const CONTRACT_DIR: &str = "contract/v1";
const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Asset {
    path: String,
    root_type: &'static str,
    bytes: Vec<u8>,
}

#[derive(Serialize)]
struct Manifest {
    protocol_version: String,
    generator_version: &'static str,
    contract_digest: String,
    assets: Vec<ManifestAsset>,
}

#[derive(Serialize)]
struct ManifestAsset {
    kind: &'static str,
    root_type: String,
    path: String,
    byte_length: usize,
    sha256: String,
    expectation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_error: Option<ErrorCode>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Field {
    root_type: String,
    container: String,
    name: String,
    required: bool,
}

struct InvalidFixture {
    file: &'static str,
    root_type: &'static str,
    expected_error: Option<ErrorCode>,
}

const INVALID_FIXTURES: &[InvalidFixture] = &[
    InvalidFixture {
        file: "unknown-field.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "unknown-enum-variant.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "unsupported-protocol-major.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::UnsupportedVersion),
    },
    InvalidFixture {
        file: "unsupported-feature.json",
        root_type: "ProtocolInfo",
        expected_error: Some(ErrorCode::UnsupportedVersion),
    },
    InvalidFixture {
        file: "missing-source-timestamp.json",
        root_type: "Observation",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "missing-digest.json",
        root_type: "Observation",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "digest-mismatch.json",
        root_type: "Observation",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "expiry-before-creation.json",
        root_type: "Observation",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "expired-observation.json",
        root_type: "Observation",
        expected_error: Some(ErrorCode::StaleObservation),
    },
    InvalidFixture {
        file: "missing-position-view.json",
        root_type: "ProposalCase",
        expected_error: Some(ErrorCode::Rejected),
    },
    InvalidFixture {
        file: "missing-instrument-view.json",
        root_type: "ProposalCase",
        expected_error: Some(ErrorCode::Rejected),
    },
    InvalidFixture {
        file: "position-instrument-mismatch.json",
        root_type: "ProposalCase",
        expected_error: Some(ErrorCode::Rejected),
    },
    InvalidFixture {
        file: "zero-quantity.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "negative-quantity.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "exponent-quantity.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "over-precision-quantity.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "increment-misaligned-quantity.json",
        root_type: "ProposalCase",
        expected_error: Some(ErrorCode::Rejected),
    },
    InvalidFixture {
        file: "client-intent-id.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "lowered-command.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "guardrail-result.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "dispatch-claim.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "private-authorization-field.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::Malformed),
    },
    InvalidFixture {
        file: "idempotency-original.json",
        root_type: "LiveProposalRequest",
        expected_error: None,
    },
    InvalidFixture {
        file: "idempotency-conflict.json",
        root_type: "LiveProposalRequest",
        expected_error: Some(ErrorCode::IdempotencyConflict),
    },
];

fn main() -> Result<(), String> {
    let mode = env::args().nth(1).unwrap_or_else(|| "check".to_string());
    if mode != "generate" && mode != "check" {
        return Err(format!(
            "unknown mode {mode:?}; expected `generate` or `check`"
        ));
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tool package is nested two levels below the repository")
        .to_path_buf();
    let schemas = schema_assets()?;
    let fixtures = valid_fixture_assets()?;
    let fields = fields_bytes(&schemas)?;
    let manifest = manifest_bytes(&root, &schemas, &fixtures)?;

    if mode == "generate" {
        write_assets(&root, &schemas, &fixtures, &fields, &manifest)?;
    } else {
        check_assets(&root, &schemas, &fixtures, &fields, &manifest)?;
    }
    Ok(())
}

fn schema_assets() -> Result<Vec<Asset>, String> {
    Ok(vec![
        schema_asset::<AgentTrace>("agent-trace.schema.json", "AgentTrace")?,
        schema_asset::<DecisionReceipt>("decision-receipt.schema.json", "DecisionReceipt")?,
        schema_asset::<LiveProposalRequest>(
            "live-proposal-request.schema.json",
            "LiveProposalRequest",
        )?,
        schema_asset::<Observation>("observation.schema.json", "Observation")?,
        schema_asset::<ProposalResponse>("proposal-response.schema.json", "ProposalResponse")?,
    ])
}

fn schema_asset<T: JsonSchema>(file: &str, root_type: &'static str) -> Result<Asset, String> {
    let schema = schemars::schema_for!(T);
    let mut value = serde_json::to_value(schema).map_err(|error| error.to_string())?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("schema for {root_type} is not an object"))?;
    object.insert(
        "$id".to_string(),
        Value::String(format!(
            "https://schemas.nautilustrader.io/agents/v1/{file}"
        )),
    );
    Ok(Asset {
        path: format!("schema/{file}"),
        root_type,
        bytes: pretty_json(&value)?,
    })
}

fn valid_fixture_assets() -> Result<Vec<Asset>, String> {
    let full = testing::valid_observation();
    let redacted = testing::redacted_observation();
    let request = testing::reduce_position_request();
    let proposal = request.proposal.clone();
    let policy = PolicyMetadata {
        name: "defensive-reducer".to_string(),
        version: "policy-3.7.11".to_string(),
    };
    let trace = |suffix: &str, outcome| -> Result<AgentTrace, String> {
        Ok(AgentTrace {
            version: PROTOCOL_VERSION,
            trace_id: parse(&format!("30000000-0000-4000-8000-{suffix}"))?,
            observation: full.reference(),
            policy: policy.clone(),
            started_at: TimestampNs::new(1_712_400_000_100_000_101),
            completed_at: TimestampNs::new(1_712_400_000_200_000_202),
            outcome,
        })
    };

    let receipt = |status, error, offset| decision_receipt(&full, status, error, offset);
    let rejected = DecisionError {
        code: ErrorCode::Rejected,
        message: "fresh position state no longer permits the proposal".to_string(),
        retryable: false,
    };
    let commit_failed = DecisionError {
        code: ErrorCode::CommitFailed,
        message: "accepted decision was not committed".to_string(),
        retryable: true,
    };
    let dispatch_failed = DecisionError {
        code: ErrorCode::DispatchFailed,
        message: "dispatch outcome is not yet known".to_string(),
        retryable: true,
    };

    let values: Vec<(&str, &str, Value)> = vec![
        (
            "full-live-observation.json",
            "Observation",
            to_value(&full)?,
        ),
        (
            "redacted-observation.json",
            "Observation",
            to_value(redacted)?,
        ),
        (
            "reduce-position-request.json",
            "LiveProposalRequest",
            to_value(request)?,
        ),
        (
            "trace-no-proposal.json",
            "AgentTrace",
            to_value(trace("000000000031", TraceOutcome::NoProposal)?)?,
        ),
        (
            "trace-proposal.json",
            "AgentTrace",
            to_value(trace("000000000032", TraceOutcome::Proposed(proposal))?)?,
        ),
        (
            "trace-timeout.json",
            "AgentTrace",
            to_value(trace(
                "000000000033",
                TraceOutcome::Failed(AgentFailure {
                    kind: AgentFailureKind::Timeout,
                    message: "policy exceeded 250ms timeout".to_string(),
                }),
            )?)?,
        ),
        (
            "trace-policy-failure.json",
            "AgentTrace",
            to_value(trace(
                "000000000034",
                TraceOutcome::Failed(AgentFailure {
                    kind: AgentFailureKind::PolicyError,
                    message: "required position source was unavailable".to_string(),
                }),
            )?)?,
        ),
        (
            "receipt-accepted.json",
            "DecisionReceipt",
            to_value(receipt(DecisionStatus::Accepted, None, 41)?)?,
        ),
        (
            "receipt-rejected.json",
            "DecisionReceipt",
            to_value(receipt(DecisionStatus::Rejected, Some(rejected), 42)?)?,
        ),
        (
            "receipt-authorized.json",
            "DecisionReceipt",
            to_value(receipt(DecisionStatus::Authorized, None, 43)?)?,
        ),
        (
            "receipt-not-dispatched.json",
            "DecisionReceipt",
            to_value(receipt(
                DecisionStatus::NotDispatched,
                Some(commit_failed),
                44,
            )?)?,
        ),
        (
            "receipt-dispatched.json",
            "DecisionReceipt",
            to_value(receipt(DecisionStatus::Dispatched, None, 45)?)?,
        ),
        (
            "receipt-dispatch-unknown.json",
            "DecisionReceipt",
            to_value(receipt(
                DecisionStatus::DispatchUnknown,
                Some(dispatch_failed),
                46,
            )?)?,
        ),
        (
            "receipt-completed.json",
            "DecisionReceipt",
            to_value(receipt(DecisionStatus::Completed, None, 47)?)?,
        ),
        (
            "error-forbidden.json",
            "ProposalResponse",
            to_value(ProposalResponse::Error(ProtocolError {
                request_id: Some(parse("20000000-0000-4000-8000-000000000021")?),
                code: ErrorCode::Forbidden,
                message: "principal cannot submit live proposals".to_string(),
                retryable: false,
            }))?,
        ),
        (
            "error-unsupported-version.json",
            "ProposalResponse",
            to_value(ProposalResponse::Error(ProtocolError {
                request_id: None,
                code: ErrorCode::UnsupportedVersion,
                message: "protocol major 2 is unsupported".to_string(),
                retryable: false,
            }))?,
        ),
    ];

    values
        .into_iter()
        .map(|(file, root_type, value)| {
            Ok(Asset {
                path: format!("fixtures/valid/{file}"),
                root_type,
                bytes: canonical::to_vec(&value).map_err(|error| error.to_string())?,
            })
        })
        .collect()
}

fn decision_receipt(
    observation: &Observation,
    status: DecisionStatus,
    error: Option<DecisionError>,
    offset: u64,
) -> Result<DecisionReceipt, String> {
    let receipt = DecisionReceipt {
        version: PROTOCOL_VERSION,
        request_id: parse("20000000-0000-4000-8000-000000000021")?,
        observation: observation.reference(),
        intent_id: parse(&format!("40000000-0000-4000-8000-{offset:012}"))?,
        status,
        error,
        correlation: CorrelationRefs {
            engine_correlation_id: Some(parse(&format!("50000000-0000-4000-8000-{offset:012}"))?),
            engine_causation_id: Some(parse(&format!("60000000-0000-4000-8000-{offset:012}"))?),
        },
        created_at: TimestampNs::new(1_712_400_001_000_000_707 + offset),
        updated_at: TimestampNs::new(1_712_400_002_000_000_808 + offset),
    };
    receipt.validate().map_err(|error| error.to_string())?;
    Ok(receipt)
}

fn fields_bytes(schemas: &[Asset]) -> Result<Vec<u8>, String> {
    let mut fields = BTreeSet::new();
    for schema in schemas {
        let value: Value =
            serde_json::from_slice(&schema.bytes).map_err(|error| error.to_string())?;
        collect_fields(schema.root_type, schema.root_type, &value, &mut fields);
    }

    let mut output = String::from(
        "# Generated by agent-contract-schema. Change Rust DTOs or generator metadata rules.\n\n",
    );
    for field in fields {
        let owner = field_owner(&field);
        let retention = field_retention(&field);
        let digest_covered = field_digest_covered(&field);
        output.push_str("[[field]]\n");
        output.push_str(&format!("root_type = {:?}\n", field.root_type));
        output.push_str(&format!("container = {:?}\n", field.container));
        output.push_str(&format!("name = {:?}\n", field.name));
        output.push_str(&format!("owner = {owner:?}\n"));
        output.push_str("stability = \"protocol_1_0\"\n");
        output.push_str(&format!("required = {}\n", field.required));
        output.push_str(&format!("retention = {retention:?}\n"));
        output.push_str(&format!("digest_covered = {digest_covered}\n\n"));
    }
    output.pop();
    Ok(output.into_bytes())
}

fn collect_fields(root_type: &str, container: &str, value: &Value, fields: &mut BTreeSet<Field>) {
    match value {
        Value::Object(object) => {
            let container = object
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(container);
            let required: BTreeSet<&str> = object
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for (name, schema) in properties {
                    fields.insert(Field {
                        root_type: root_type.to_string(),
                        container: container.to_string(),
                        name: name.clone(),
                        required: required.contains(name.as_str()),
                    });
                    collect_fields(root_type, container, schema, fields);
                }
            }
            if let Some(definitions) = object.get("$defs").and_then(Value::as_object) {
                for (name, schema) in definitions {
                    collect_fields(root_type, name, schema, fields);
                }
            }
            for (key, nested) in object {
                if key != "$defs" && key != "properties" && key != "required" {
                    collect_fields(root_type, container, nested, fields);
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_fields(root_type, container, nested, fields);
            }
        }
        _ => {}
    }
}

fn field_owner(field: &Field) -> &'static str {
    match field.root_type.as_str() {
        "Observation" | "DecisionReceipt" | "ProposalResponse" => "nautilus_trader",
        "LiveProposalRequest" => "caller",
        "AgentTrace" => "agent",
        _ => "shared",
    }
}

fn field_retention(field: &Field) -> &'static str {
    match field.root_type.as_str() {
        "Observation" => "declared_by_observation",
        "AgentTrace" => "agent_trace",
        _ => "reference_only",
    }
}

fn field_digest_covered(field: &Field) -> bool {
    match field.root_type.as_str() {
        "Observation" => !(field.container == "Observation" && field.name == "digest"),
        "LiveProposalRequest" | "AgentTrace" => true,
        _ => false,
    }
}

fn manifest_bytes(root: &Path, schemas: &[Asset], fixtures: &[Asset]) -> Result<Vec<u8>, String> {
    let mut assets = Vec::new();
    for schema in schemas {
        assets.push(manifest_asset("schema", schema, "schema", None));
    }
    for fixture in fixtures {
        assets.push(manifest_asset("fixture", fixture, "valid", None));
    }
    for fixture in INVALID_FIXTURES {
        let path = format!("fixtures/invalid/{}", fixture.file);
        let bytes = fs::read(root.join(CONTRACT_DIR).join(&path))
            .map_err(|error| format!("failed to read {path}: {error}"))?;
        let expectation = if fixture.expected_error.is_some() {
            "invalid"
        } else {
            "valid"
        };
        assets.push(manifest_asset(
            "fixture",
            &Asset {
                path,
                root_type: fixture.root_type,
                bytes,
            },
            expectation,
            fixture.expected_error,
        ));
    }
    assets.sort_by(|left, right| left.path.cmp(&right.path));
    let contract_digest = canonical::sha256(&assets)
        .map_err(|error| error.to_string())?
        .to_string();
    pretty_json(&Manifest {
        protocol_version: format!("{}.{}", PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor),
        generator_version: GENERATOR_VERSION,
        contract_digest,
        assets,
    })
}

fn manifest_asset(
    kind: &'static str,
    asset: &Asset,
    expectation: &'static str,
    expected_error: Option<ErrorCode>,
) -> ManifestAsset {
    ManifestAsset {
        kind,
        root_type: asset.root_type.to_string(),
        path: asset.path.clone(),
        byte_length: asset.bytes.len(),
        sha256: digest_bytes(&asset.bytes),
        expectation,
        expected_error,
    }
}

fn write_assets(
    root: &Path,
    schemas: &[Asset],
    fixtures: &[Asset],
    fields: &[u8],
    manifest: &[u8],
) -> Result<(), String> {
    for asset in schemas.iter().chain(fixtures) {
        write_file(&root.join(CONTRACT_DIR).join(&asset.path), &asset.bytes)?;
    }
    write_file(&root.join(CONTRACT_DIR).join("fields.toml"), fields)?;
    write_file(&root.join(CONTRACT_DIR).join("manifest.json"), manifest)?;
    check_expected_names(root, schemas, fixtures)
}

fn check_assets(
    root: &Path,
    schemas: &[Asset],
    fixtures: &[Asset],
    fields: &[u8],
    manifest: &[u8],
) -> Result<(), String> {
    for asset in schemas.iter().chain(fixtures) {
        check_file(&root.join(CONTRACT_DIR).join(&asset.path), &asset.bytes)?;
    }
    check_file(&root.join(CONTRACT_DIR).join("fields.toml"), fields)?;
    check_file(&root.join(CONTRACT_DIR).join("manifest.json"), manifest)?;
    check_expected_names(root, schemas, fixtures)
}

fn check_expected_names(root: &Path, schemas: &[Asset], fixtures: &[Asset]) -> Result<(), String> {
    let expected_schema: BTreeSet<_> = schemas
        .iter()
        .map(|asset| Path::new(&asset.path).file_name().unwrap().to_owned())
        .collect();
    let expected_valid: BTreeSet<_> = fixtures
        .iter()
        .map(|asset| Path::new(&asset.path).file_name().unwrap().to_owned())
        .collect();
    let expected_invalid: BTreeSet<_> = INVALID_FIXTURES
        .iter()
        .map(|fixture| fixture.file.into())
        .collect();
    for (directory, expected) in [
        ("schema", expected_schema),
        ("fixtures/valid", expected_valid),
        ("fixtures/invalid", expected_invalid),
    ] {
        let actual = directory_names(&root.join(CONTRACT_DIR).join(directory))?;
        if actual != expected {
            return Err(format!(
                "unexpected files in {directory}: expected {expected:?}, found {actual:?}"
            ));
        }
    }
    Ok(())
}

fn directory_names(path: &Path) -> Result<BTreeSet<std::ffi::OsString>, String> {
    fs::read_dir(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, bytes).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn check_file(path: &Path, expected: &[u8]) -> Result<(), String> {
    let actual =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if actual != expected {
        return Err(format!("generated asset differs: {}", path.display()));
    }
    Ok(())
}

fn pretty_json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn to_value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn parse<T: std::str::FromStr>(value: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    value.parse::<T>().map_err(|error| error.to_string())
}

fn digest_bytes(bytes: &[u8]) -> String {
    ContentDigest::new(Sha256::digest(bytes).into()).to_string()
}
