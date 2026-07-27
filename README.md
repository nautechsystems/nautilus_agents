# Nautilus Agents SDK

[![License](https://img.shields.io/badge/license-LGPL--3.0--or--later-blue.svg)](LICENSE)
![Status](https://img.shields.io/badge/status-early%20alpha-orange)

Nautilus Agents is the public SDK for authoring and assuring agent policies that propose narrow,
semantic actions to NautilusTrader. It gives policy authors scoped observations, strict protocol
types, local evidence, and a transport-neutral client boundary without granting engine or venue
authority to the agent process.

> [!WARNING]
> **Early alpha:** the API and protocol 1.0 may change. The crate is not ready for production use.

## Scope

The SDK provides:

- Protocol-native identity, quantity, timestamp, digest, capability, observation, proposal, error, and receipt types.
- One semantic live proposal: `ReducePosition`.
- Runtime-neutral proposal policies with timeout and panic capture.
- Agent-side traces and retention-aware JSONL recording.
- Advisory-only local checks and side-by-side policy evaluation.
- A transport-neutral client trait.
- Generated schemas, fixtures, field metadata, and embedded conformance assets.
- Deterministic test values behind the `testkit` feature.

## Design intent

The SDK is designed for agent processes that should be able to reason about a narrow view of live
state and propose a semantic risk-reducing action. Policy code works against an exact, versioned
`Observation`, produces no proposal or one `ReducePosition` proposal, and can retain agent-side
evidence under an explicit recording policy.

The public boundary contains the observation, proposal, and assurance data needed to author and
test those policies. Protocol-native DTOs keep the crate independent of NautilusTrader packages.

## Authority boundary

NautilusTrader owns observation construction and every production decision and execution step.
This crate evaluates policies and carries proposals, local evidence, and public outcomes.

An `AdvisoryReport` is local evidence only. NautilusTrader may reject a proposal even when every
local check is clear. An `AgentTrace` records the agent-side evaluation, while a `DecisionReceipt`
reports the public outcome. Neither grants production authority.

## Protocol 1.0

Protocol versioning is independent of crate SemVer. `ProtocolVersion { major, minor }` travels with
public observations, requests, traces, and receipts. A major change is incompatible. A supported
minor version may add backward-compatible detail.

Protocol 1.0 supports only:

```rust
LiveIntent::ReducePosition(ReducePosition {
    position_id,
    instrument_id,
    quantity,
})
```

`ReducePosition` carries only a position, instrument, and quantity. NautilusTrader determines how
to handle an accepted proposal.

## Authoring a policy

Implement `ProposalPolicy` with explicit SDK imports:

```rust
use nautilus_agents::{
    authoring::policy::{ProposalDecision, ProposalFuture, ProposalPolicy},
    protocol::observation::Observation,
};

struct ObserveOnly;

impl ProposalPolicy for ObserveOnly {
    fn propose<'a>(&'a self, _observation: &'a Observation) -> ProposalFuture<'a> {
        Box::pin(async { Ok(ProposalDecision::NoProposal) })
    }
}
```

`ProposalRunner` applies a runtime-neutral timeout, captures returned errors and panics, and emits
exactly one `AgentTrace`. It does not call a client or produce a receipt.

See [the defensive policy example](examples/defensive_policy.rs) for a complete typed observation,
policy evaluation, trace, and advisory report:

```bash
cargo run --example defensive_policy
```

## Local assurance

### Advisory checks

`AdvisoryValidator` checks protocol version, expiry, digest, required omissions, position and
instrument identity, observed quantity, and quantity increment. Reports use `finding` and `clear`
terms because the checks do not make a production decision.

### Recording

`TraceRecorder` writes traces and optional observations as separate JSONL record kinds. The default
`ObservationCapture::ReferenceOnly` mode stores only `ObservationRef` identity and digest data.

- `ReferenceOnly` stores no observation payload.
- `Redacted` requires an `ObservationRedactor`, and the returned observation must pass protocol validation.
- `Full` records the complete observation.

`Redacted` and `Full` both reject `RetentionClass::Restricted` observation data.

Each record update replaces the JSONL target atomically, so a failed write does not leave a
partial record. Use one recorder per path; concurrent recorders are not coordinated and may
overwrite each other's latest append.

### Shadow evaluation

`ShadowEvaluator` runs two `ProposalPolicy` values over the same recorded or synthetic
`Observation`. It compares proposal decisions and local failure outcomes field by field. It does
not simulate NautilusTrader or venue outcomes.

See [the shadow policy example](examples/shadow_policy.rs):

```bash
cargo run --example shadow_policy
```

## Client boundary

`AgentClient` separates policy authoring from transport:

```rust
pub trait AgentClient: Send + Sync {
    fn submit<'a>(
        &'a self,
        request: &'a LiveProposalRequest,
    ) -> ClientFuture<'a, ProposalResponse>;

    fn receipt<'a>(
        &'a self,
        request_id: &'a RequestId,
    ) -> ClientFuture<'a, ProposalResponse>;
}
```

`ClientError` covers transport, decoding, and unsupported-version failures. A request-level error
or decision-path rejection is a successful `ProposalResponse`, not a client transport failure.

## Modules

| Module        | Purpose                                                                     |
| ------------- | --------------------------------------------------------------------------- |
| `protocol`    | Strict versioned DTOs, identities, values, digests, requests, and receipts. |
| `authoring`   | `ProposalPolicy`, local decisions, errors, configuration, and runner.       |
| `assurance`   | Traces, recording, advisory reports, and shadow policy comparison.          |
| `client`      | Transport-neutral request submission and receipt retrieval.                 |
| `conformance` | Embedded public contract assets behind the `conformance` feature.           |
| `testing`     | Deterministic builders and values behind the `testkit` feature.             |

The crate has no broad prelude. Import the public values each policy uses.

## Contract assets

Rust DTOs are the source for the versioned assets under [`contract/v1`](contract/v1):

- Draft 2020-12 JSON Schemas.
- Canonical RFC 8785 valid fixtures.
- Reviewed invalid fixtures with expected public errors.
- `fields.toml` ownership, stability, required, retention, and digest metadata.
- `manifest.json` byte lengths, SHA-256 hashes, root types, expectations, and aggregate digest.

Regenerate and verify them with:

```bash
make contract-generate
make contract-check
```

Enable `conformance` to embed the same bytes in consumer tests. Enable `testkit` for `ObservationBuilder`
and deterministic observation, request, trace, receipt, redaction, and expiry constructors.

## Compatibility

| Surface                         | Current support     |
| ------------------------------- | ------------------- |
| Crate version                   | `0.2.0` early alpha |
| Minimum Rust version            | `1.97.1`            |
| Protocol version                | `1.0`               |
| Semantic live proposals         | `ReducePosition`    |
| NautilusTrader package coupling | None                |

## Security

See [SECURITY.md](SECURITY.md) to report a vulnerability privately.

## License

This project is licensed under the GNU Lesser General Public License v3.0 or later. See [LICENSE](LICENSE).
