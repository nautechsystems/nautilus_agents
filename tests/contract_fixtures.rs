#![cfg(feature = "conformance")]

use std::collections::{BTreeMap, BTreeSet};

use nautilus_agents::{
    VERSION,
    assurance::trace::AgentTrace,
    conformance::{self, FixtureExpectation},
    protocol::{
        canonical,
        error::ErrorCode,
        identity::{IdempotencyKey, IdentityError},
        live::LiveProposalRequest,
        observation::{Observation, ObservationError},
        receipt::{DecisionReceipt, ProposalResponse},
        value::{ContentDigest, InstrumentId, PositionId, Quantity, TimestampNs, ValueError},
        version::ProtocolInfo,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Deserialize, Serialize)]
struct Manifest {
    protocol_version: String,
    generator_version: String,
    contract_digest: String,
    assets: Vec<ManifestAsset>,
}

#[derive(Deserialize, Serialize)]
struct ManifestAsset {
    kind: String,
    root_type: String,
    path: String,
    byte_length: usize,
    sha256: String,
    expectation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_error: Option<ErrorCode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalCase {
    positions: Vec<ProposalPosition>,
    instruments: Vec<ProposalInstrument>,
    proposal: ProposalValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalPosition {
    position_id: PositionId,
    instrument_id: InstrumentId,
    quantity: Quantity,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalInstrument {
    instrument_id: InstrumentId,
    quantity_increment: Quantity,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalValue {
    position_id: PositionId,
    instrument_id: InstrumentId,
    quantity: Quantity,
}

#[derive(Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

#[test]
fn test_manifest_matches_every_embedded_asset() {
    let manifest_bytes = include_bytes!("../contract/v1/manifest.json");
    let manifest: Manifest = serde_json::from_slice(manifest_bytes).unwrap();
    assert_eq!(manifest.protocol_version, "1.0");
    assert_eq!(manifest.generator_version, VERSION);
    assert_eq!(
        canonical::sha256(&manifest.assets).unwrap().as_str(),
        manifest.contract_digest
    );
    assert_eq!(
        conformance::contract_digest().as_str(),
        manifest.contract_digest
    );
    assert_eq!(conformance::contract_version().major, 1);
    assert_eq!(conformance::contract_version().minor, 0);

    let schemas: BTreeMap<_, _> = conformance::schemas()
        .iter()
        .map(|schema| (schema.name, schema))
        .collect();
    let fixtures: BTreeMap<_, _> = conformance::fixtures()
        .iter()
        .map(|fixture| (fixture.name, fixture))
        .collect();
    assert_eq!(schemas.len(), 5);
    assert_eq!(fixtures.len(), 40);
    assert_eq!(manifest.assets.len(), 45);

    for asset in &manifest.assets {
        let name = asset.path.rsplit('/').next().unwrap();
        let bytes = if asset.kind == "schema" {
            let schema = schemas.get(name).unwrap();
            assert_eq!(schema.root_type, asset.root_type);
            let value: Value = serde_json::from_slice(schema.bytes).unwrap();
            assert_eq!(
                value["$id"],
                Value::String(format!(
                    "https://schemas.nautilustrader.io/agents/v1/{name}"
                ))
            );
            schema.bytes
        } else {
            let fixture = fixtures.get(name).unwrap();
            assert_eq!(fixture.root_type, asset.root_type);
            let expected = match asset.expectation.as_str() {
                "valid" => FixtureExpectation::Valid,
                "invalid" => FixtureExpectation::Invalid,
                other => panic!("unexpected fixture expectation {other}"),
            };
            assert_eq!(fixture.expectation, expected);
            assert_eq!(fixture.expected_error, asset.expected_error);
            fixture.bytes
        };
        assert_eq!(bytes.len(), asset.byte_length, "asset: {}", asset.path);
        assert_eq!(digest(bytes), asset.sha256, "asset: {}", asset.path);
    }
}

#[test]
fn test_valid_fixtures_are_exact_canonical_values() {
    for fixture in conformance::fixtures() {
        if fixture.expectation != FixtureExpectation::Valid {
            continue;
        }
        assert_fixture_valid(fixture.root_type, fixture.bytes);
        if fixture.name != "idempotency-original.json" {
            let value: Value = serde_json::from_slice(fixture.bytes).unwrap();
            assert_eq!(
                canonical::to_vec(&value).unwrap(),
                fixture.bytes,
                "fixture: {}",
                fixture.name
            );
        }
    }
}

#[test]
fn test_invalid_fixtures_fail_at_the_named_boundary() {
    for fixture in conformance::fixtures() {
        if fixture.expectation != FixtureExpectation::Invalid {
            continue;
        }
        match fixture.root_type {
            "LiveProposalRequest" => assert_invalid_request(fixture.name, fixture.bytes),
            "Observation" => assert_invalid_observation(fixture.name, fixture.bytes),
            "ProtocolInfo" => {
                assert!(serde_json::from_slice::<ProtocolInfo>(fixture.bytes).is_err())
            }
            "ProposalCase" => assert_invalid_proposal_case(fixture.name, fixture.bytes),
            other => panic!("unhandled invalid root type {other}"),
        }
    }
}

#[test]
fn test_each_serialized_public_field_is_mutation_sensitive() {
    let mut mutated_paths = BTreeSet::new();
    for fixture in conformance::fixtures() {
        if fixture.expectation != FixtureExpectation::Valid
            || fixture.name == "idempotency-original.json"
        {
            continue;
        }
        let value: Value = serde_json::from_slice(fixture.bytes).unwrap();
        let mut paths = Vec::new();
        collect_paths(&value, &mut Vec::new(), &mut paths);
        for path in paths {
            let mut mutated = value.clone();
            replace_with_invalid(&mut mutated, &path);
            assert!(
                !root_is_valid(fixture.root_type, &mutated),
                "mutation unexpectedly passed for {} at {}",
                fixture.name,
                display_path(&path)
            );
            mutated_paths.insert(format!("{}:{}", fixture.root_type, display_path(&path)));
        }
    }
    assert!(
        mutated_paths.len() >= 70,
        "only {} field paths were covered",
        mutated_paths.len()
    );
}

#[test]
fn test_private_fields_never_enter_request_serialization() {
    let fixture = conformance::fixtures()
        .iter()
        .find(|fixture| fixture.name == "reduce-position-request.json")
        .unwrap();
    let request: LiveProposalRequest = serde_json::from_slice(fixture.bytes).unwrap();
    let baseline = serde_json::to_value(&request).unwrap();
    let serialized = serde_json::to_string(&request).unwrap();
    for field in [
        "intent_id",
        "lowered_command",
        "guardrail_result",
        "dispatch_claim",
        "authorization_record_id",
        "command_digest",
        "gate_state",
    ] {
        assert!(!serialized.contains(field));
        let mut mutation = baseline.clone();
        mutation
            .as_object_mut()
            .unwrap()
            .insert(field.to_string(), Value::String("private".to_string()));
        assert!(serde_json::from_value::<LiveProposalRequest>(mutation).is_err());
    }
}

#[test]
fn test_idempotency_pair_changes_only_one_canonical_payload_field() {
    let fixtures: BTreeMap<_, _> = conformance::fixtures()
        .iter()
        .map(|fixture| (fixture.name, fixture))
        .collect();
    let original: Value =
        serde_json::from_slice(fixtures["idempotency-original.json"].bytes).unwrap();
    let conflict: Value =
        serde_json::from_slice(fixtures["idempotency-conflict.json"].bytes).unwrap();
    let original_request: LiveProposalRequest = serde_json::from_value(original.clone()).unwrap();
    let conflict_request: LiveProposalRequest = serde_json::from_value(conflict.clone()).unwrap();
    assert_eq!(
        original_request.idempotency_key,
        conflict_request.idempotency_key
    );

    let mut normalized = conflict;
    normalized["proposal"]["intent"]["reduce_position"]["quantity"] =
        original["proposal"]["intent"]["reduce_position"]["quantity"].clone();
    assert_eq!(normalized, original);
    assert_eq!(
        fixtures["idempotency-conflict.json"].expected_error,
        Some(ErrorCode::IdempotencyConflict)
    );
}

#[test]
fn test_schema_scalar_patterns_match_deserialization() {
    let request_schema = conformance::schemas()
        .iter()
        .find(|schema| schema.name == "live-proposal-request.schema.json")
        .unwrap();
    let schema: Value = serde_json::from_slice(request_schema.bytes).unwrap();

    assert_eq!(
        schema["$defs"]["IdempotencyKey"]["pattern"],
        r"^[\u0000-\u007f]+$"
    );
    assert_eq!(
        schema["$defs"]["InstrumentId"]["pattern"],
        r"^[^\u0000-\u001f\u007f-\u009f]+$"
    );
    assert_eq!(
        schema["$defs"]["PositionId"]["pattern"],
        r"^[^\u0000-\u001f\u007f-\u009f]+$"
    );
    assert_eq!(
        IdempotencyKey::parse("position-\u{2713}"),
        Err(IdentityError::NonAsciiIdempotencyKey)
    );
    assert_eq!(
        InstrumentId::parse("BTC\u{0085}USDT"),
        Err(ValueError::IdentifierControlCharacter)
    );
    assert_eq!(
        PositionId::parse("P\u{0085}197"),
        Err(ValueError::IdentifierControlCharacter)
    );
}

fn assert_fixture_valid(root_type: &str, bytes: &[u8]) {
    match root_type {
        "AgentTrace" => {
            let trace: AgentTrace = serde_json::from_slice(bytes).unwrap();
            assert!(trace.version.is_supported());
            assert!(trace.completed_at >= trace.started_at);
        }
        "DecisionReceipt" => {
            let receipt: DecisionReceipt = serde_json::from_slice(bytes).unwrap();
            assert_eq!(receipt.validate(), Ok(()));
        }
        "LiveProposalRequest" => {
            let request: LiveProposalRequest = serde_json::from_slice(bytes).unwrap();
            assert_eq!(request.validate(), Ok(()));
        }
        "Observation" => {
            let observation: Observation = serde_json::from_slice(bytes).unwrap();
            assert_eq!(observation.validate(), Ok(()));
        }
        "ProposalResponse" => {
            let response: ProposalResponse = serde_json::from_slice(bytes).unwrap();
            if let ProposalResponse::Receipt(receipt) = response {
                assert_eq!(receipt.validate(), Ok(()));
            }
        }
        other => panic!("unhandled valid root type {other}"),
    }
}

fn assert_invalid_request(name: &str, bytes: &[u8]) {
    match name {
        "idempotency-conflict.json" => {
            let request: LiveProposalRequest = serde_json::from_slice(bytes).unwrap();
            assert_eq!(request.validate(), Ok(()));
        }
        "unsupported-protocol-major.json" => {
            let request: LiveProposalRequest = serde_json::from_slice(bytes).unwrap();
            assert!(request.validate().is_err());
        }
        _ => assert!(
            serde_json::from_slice::<LiveProposalRequest>(bytes).is_err(),
            "fixture unexpectedly decoded: {name}"
        ),
    }
}

fn assert_invalid_observation(name: &str, bytes: &[u8]) {
    match name {
        "digest-mismatch.json" => {
            let observation: Observation = serde_json::from_slice(bytes).unwrap();
            assert!(matches!(
                observation.validate(),
                Err(ObservationError::DigestMismatch { .. })
            ));
        }
        "expiry-before-creation.json" => {
            let observation: Observation = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                observation.validate(),
                Err(ObservationError::ExpiryBeforeCreation)
            );
        }
        "expired-observation.json" => {
            let observation: Observation = serde_json::from_slice(bytes).unwrap();
            assert_eq!(observation.validate(), Ok(()));
            assert_eq!(
                observation.validate_at(TimestampNs::new(201)),
                Err(ObservationError::Expired {
                    expires_at: TimestampNs::new(200),
                })
            );
        }
        _ => assert!(
            serde_json::from_slice::<Observation>(bytes).is_err(),
            "fixture unexpectedly decoded: {name}"
        ),
    }
}

fn assert_invalid_proposal_case(name: &str, bytes: &[u8]) {
    let case: ProposalCase = serde_json::from_slice(bytes).unwrap();
    let position = case
        .positions
        .iter()
        .find(|position| position.position_id == case.proposal.position_id);
    let instrument = case
        .instruments
        .iter()
        .find(|instrument| instrument.instrument_id == case.proposal.instrument_id);
    match name {
        "missing-position-view.json" => assert!(position.is_none()),
        "missing-instrument-view.json" => assert!(instrument.is_none()),
        "position-instrument-mismatch.json" => {
            assert_ne!(position.unwrap().instrument_id, case.proposal.instrument_id)
        }
        "increment-misaligned-quantity.json" => {
            assert_eq!(instrument.unwrap().quantity_increment.as_str(), "0.25");
            assert_eq!(case.proposal.quantity.as_str(), "1.2");
            assert_eq!(position.unwrap().quantity.as_str(), "2.75");
        }
        other => panic!("unhandled proposal case {other}"),
    }
}

fn root_is_valid(root_type: &str, value: &Value) -> bool {
    match root_type {
        "AgentTrace" => serde_json::from_value::<AgentTrace>(value.clone()).is_ok(),
        "DecisionReceipt" => serde_json::from_value::<DecisionReceipt>(value.clone())
            .is_ok_and(|receipt| receipt.validate().is_ok()),
        "LiveProposalRequest" => serde_json::from_value::<LiveProposalRequest>(value.clone())
            .is_ok_and(|request| request.validate().is_ok()),
        "Observation" => serde_json::from_value::<Observation>(value.clone())
            .is_ok_and(|observation| observation.validate().is_ok()),
        "ProposalResponse" => serde_json::from_value::<ProposalResponse>(value.clone()).is_ok(),
        other => panic!("unhandled mutation root type {other}"),
    }
}

fn collect_paths(
    value: &Value,
    current: &mut Vec<PathSegment>,
    output: &mut Vec<Vec<PathSegment>>,
) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                current.push(PathSegment::Key(key.clone()));
                output.push(current.clone());
                collect_paths(nested, current, output);
                current.pop();
            }
        }
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                current.push(PathSegment::Index(index));
                collect_paths(nested, current, output);
                current.pop();
            }
        }
        _ => {}
    }
}

fn replace_with_invalid(value: &mut Value, path: &[PathSegment]) {
    let (last, parents) = path.split_last().unwrap();
    let mut target = value;
    for segment in parents {
        target = match segment {
            PathSegment::Key(key) => &mut target[key],
            PathSegment::Index(index) => &mut target[*index],
        };
    }
    let value = match last {
        PathSegment::Key(key) => &mut target[key],
        PathSegment::Index(index) => &mut target[*index],
    };
    *value = if value.is_object() || value.is_array() {
        Value::String("__invalid_contract_value__".to_string())
    } else {
        serde_json::json!({"__invalid_contract_value__": true})
    };
}

fn display_path(path: &[PathSegment]) -> String {
    let mut value = String::new();
    for segment in path {
        value.push('/');
        match segment {
            PathSegment::Key(key) => value.push_str(key),
            PathSegment::Index(index) => value.push_str(&index.to_string()),
        }
    }
    value
}

fn digest(bytes: &[u8]) -> String {
    ContentDigest::new(Sha256::digest(bytes).into()).to_string()
}
