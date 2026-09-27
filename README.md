# Nautilus Agents SDK

[![License](https://img.shields.io/badge/license-LGPL--3.0--only-blue.svg)](LICENSE)
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
- Runtime-neutral proposal policies with a cooperative timeout and unwinding-panic capture while
  creating and polling the policy future.
- Agent-side traces and retention-aware JSONL recording.
- Advisory-only local checks and side-by-side policy evaluation.
- A transport-neutral client trait, with no bundled transport.
- Versioned schemas, fixtures, field metadata, and embedded conformance assets.
- Deterministic test values behind the `testkit` feature.

## Design intent

The SDK targets agent processes that reason about a narrow view of live state and propose a
semantic, risk-reducing action. Policy code works against an exact, versioned `Observation`,
produces either no proposal or one `ReducePosition` proposal, and can retain agent-side evidence
under an explicit capture mode.

The public boundary contains the observation, proposal, and assurance data needed to author and
test those policies. Protocol-native DTOs keep the crate independent of NautilusTrader packages.

## Authority boundary

NautilusTrader owns observation construction and every production decision and execution step.
This crate evaluates policies and carries proposals, local evidence, and public outcomes.

An `AdvisoryReport` is local evidence only. NautilusTrader revalidates every input and may reject a
proposal even when every local check is clear. An `AgentTrace` records the agent-side evaluation,
and a `DecisionReceipt` reports the public outcome. Neither grants production authority.

## Protocol 1.0

Protocol versioning is independent of crate SemVer. `ProtocolVersion { major, minor }` travels with
public observations, requests, traces, and receipts. A major change is incompatible, and a minor
change may add backward-compatible detail. Validation accepts a version with the crate's major and
an equal or lower minor, so a crate implementing protocol 1.0 rejects both `1.1` and `2.0`.

Protocol 1.0 supports only:

```rust
LiveIntent::ReducePosition(ReducePosition {
    position_id,
    instrument_id,
    quantity,
})
```

`ReducePosition` carries only a position, instrument, and strictly positive quantity.
NautilusTrader determines how to handle an accepted proposal.

## Availability

This README documents the unreleased `0.2.0` source tree, which is not published on crates.io. To
evaluate it from source, run:

```bash
git clone https://github.com/nautechsystems/nautilus_agents.git
cd nautilus_agents
```

[docs.rs](https://docs.rs/nautilus-agents) documents the latest published release, not this source
tree.

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

`ProposalRunner::new` takes the policy and a `RunnerConfig`, which sets the `PolicyMetadata`
recorded in every trace and the evaluation timeout. `ProposalRunner::run` evaluates the policy
against one observation:

- Each completed run returns exactly one `AgentTrace`. The runner does not call a client or produce
  a receipt.
- The runner races the policy future against a runtime-neutral timer. The timeout takes effect only
  when the policy future yields control, so it cannot preempt blocking work inside a future poll.
- A returned error, a timeout, or an unwinding panic raised while creating or polling the policy
  future becomes a `TraceOutcome::Failed` trace with `AgentFailureKind::PolicyError`, `Timeout`, or
  `Panic`, respectively.
- A panic while dropping the future may escape, and an aborting panic terminates the process.

See [the defensive policy example](examples/defensive_policy.rs) for a complete typed observation,
policy evaluation, trace, and advisory report:

```bash
cargo run --example defensive_policy
```

## Local assurance

### Advisory checks

`AdvisoryValidator::evaluate` checks one proposal against one observation at a caller-supplied
evaluation time. It checks protocol version, expiry, digest, required omissions, position and
instrument identity, observed quantity, and quantity increment. Reports use `finding` and `clear`
terms because the checks do not make a production decision.

### Recording

`TraceRecorder` writes traces and optional observations as separate JSONL record kinds. Trace
records use the `trace` kind. Observation records follow the configured `ObservationCapture` mode,
which defaults to `ReferenceOnly`:

| Mode            | Record kind             | Stores                                    | Rejects                                                                  |
| --------------- | ----------------------- | ----------------------------------------- | ------------------------------------------------------------------------ |
| `ReferenceOnly` | `observation_reference` | `ObservationRef` identity and digest only | None                                                                     |
| `Redacted`      | `observation`           | Source reference and redacted observation | Missing redactor, failed redaction or validation, or `Restricted` output |
| `Full`          | `observation`           | Source reference and complete observation | A `Restricted` observation                                               |

Supply the `ObservationRedactor` for `Redacted` capture through `TraceRecorder::with_redactor`. The
redacted observation must pass protocol validation.

> [!IMPORTANT]
> The recorder enforces only `RetentionClass::Restricted`. Match the capture mode to the
> observation's retention class yourself: use `ReferenceOnly` capture for
> `RetentionClass::ReferenceOnly` data, and retain `RetentionClass::Derived` data only under your
> own retention policy.

Each append replaces the JSONL target atomically, so a failed write does not leave a partial
record. Use one recorder per path; concurrent recorders are not coordinated and may overwrite each
other's latest append.

### Shadow evaluation

`ShadowEvaluator` runs a baseline and a candidate `ProposalPolicy`, each with its own
`RunnerConfig`, over the same recorded or synthetic `Observation`. It compares proposal decisions
and local failure outcomes field by field, and `ShadowResult` lists each differing field path. It
does not simulate NautilusTrader or venue outcomes.

See [the shadow policy example](examples/shadow_policy.rs):

```bash
cargo run --example shadow_policy
```

## Client boundary

`AgentClient` separates policy authoring from transport. The crate defines the trait and ships no
transport implementation:

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

A successful call returns a `ProposalResponse`: a `DecisionReceipt` once NautilusTrader accepts the
request into the decision path, or a `ProtocolError` when it rejects the request before acceptance.
A request-level error or a decision-path rejection therefore arrives as a response, not a
`ClientError`. `ClientError` covers transport, decoding, and unsupported-version failures.

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

The contract generator derives these versioned assets under [`contract/v1`](contract/v1) from the
Rust DTOs:

- Draft 2020-12 JSON Schemas under `contract/v1/schema`.
- Canonical RFC 8785 fixtures under `contract/v1/fixtures/valid`.
- `fields.toml` ownership, stability, required, retention, and digest metadata.
- `manifest.json` byte lengths, SHA-256 hashes, root types, expectations, and aggregate digest.

The consumer cases under `contract/v1/fixtures/invalid` are reviewed by hand, not generated. They
include a structurally valid idempotency control and invalid fixtures with expected public errors.
The generator records their hashes and expectations in the manifest but never rewrites them.

Regenerate and verify the assets with:

```bash
make contract-generate
make contract-check
```

`make contract-check` fails when a generated asset differs from the generator's output or when the
schema or fixture directories hold an unexpected or missing file.

Enable `conformance` to embed the same schema and fixture bytes, with each fixture's expectation and
the aggregate contract digest, in consumer tests. Enable `testkit` for `ObservationBuilder` and
deterministic observation, request, trace, receipt, redaction, and expiry constructors.

## Compatibility

| Surface                         | Current support     |
| ------------------------------- | ------------------- |
| Crate version                   | `0.2.0` early alpha |
| Minimum Rust version            | `1.98.0`            |
| Protocol version                | `1.0`               |
| Semantic live proposals         | `ReducePosition`    |
| NautilusTrader package coupling | None                |

## Engineering standards

[`nautilus_engineering`](https://github.com/nautechsystems/nautilus_engineering) is the source of
truth for shared engineering standards, tool pins, managed pre-commit definitions, and vendored
scripts. [`.nautilus-engineering.lock`](.nautilus-engineering.lock) records the adopted revision,
selected profiles, target paths, and file hashes.

Do not edit a managed file or its lock hash directly. A root `tools.toml`, if needed, is reserved
for tools unique to this repository; shared tool pins remain in
`.nautilus-engineering/tools.toml`. Local policy stays in `Cargo.toml`, `security-audit.toml`,
`deny.toml`, the `supply-chain` audit store, the `Makefile`, and pre-commit entries outside the
managed markers.

To adopt a reviewed upstream revision, run the update from that exact `nautilus_engineering`
checkout, then render and verify the consumer files:

```bash
consumer_repo=/path/to/nautilus_agents
sync/sync.bash update --consumer "$consumer_repo"
cd "$consumer_repo"
python3 scripts/manage-nautilus-engineering-pre-commit.py render
make check-shared
```

See the upstream
[consumer adoption guide](https://github.com/nautechsystems/nautilus_engineering/blob/main/docs/consumer-adoption.md)
for selection changes and target overrides.

## Security

See [SECURITY.md](SECURITY.md) to report a vulnerability privately.

## License

This project is licensed under the GNU Lesser General Public License v3.0 only. See [LICENSE](LICENSE).
