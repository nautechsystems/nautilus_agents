# Nautilus Agents

![license](https://img.shields.io/github/license/nautechsystems/nautilus_agents?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

Open agent protocol for [NautilusTrader](https://nautilustrader.io).

This crate defines the contract between an agent policy and the Nautilus
trading engine:

- Scope what an agent observes with `AgentContext`.
- Express decisions with `PolicyDecision`.
- Represent semantic actions with `AgentIntent`.
- Lower intents into `RuntimeAction`.
- Record each cycle in a `DecisionEnvelope`.

> [!WARNING]
> Early development. The API is not yet stable.

## Platform

[NautilusTrader](https://nautilustrader.io) is an open-source,
high-performance, production-grade algorithmic trading platform. It lets
traders backtest automated strategies on historical data with an
event-driven engine, then deploy those same strategies live with no code
changes.

NautilusTrader's design, architecture, and implementation philosophy put
correctness and safety first. The platform targets backtesting and live
trading workloads where mistakes cost real money.

## What this crate includes

`nautilus-agents` is the open protocol layer. It sits on top of the open
NautilusTrader crates and reuses their real model types.

- `AgentContext`: owned, bounded snapshot of engine state built from
  `QuoteTick`, `Bar`, `AccountState`, `PositionSnapshot`, `OrderSnapshot`,
  and `PositionStatusReport`.
- `AgentPolicy`: the trait a policy implements.
- `PolicyDecision`: `Act(AgentIntent)` or `NoAction`.
- `AgentIntent`: semantic actions with execution constraints.
- `CapabilitySet`: explicit observation and action permissions.
- Intent and action guardrail traits.
- Lowering from `AgentIntent` to `RuntimeAction`.
- `DecisionPipeline`: the policy, capability, guardrail, and lowering loop.
- `DecisionEnvelope`: the canonical record for one decision cycle.
- `DecisionRecorder`: line-delimited JSON recording for envelopes.

## How the pieces fit together

The crate keeps agent reasoning separate from execution details:

```mermaid
flowchart TD
    A[Engine state] --> B[AgentContext]
    B --> C[AgentPolicy::evaluate]
    C --> D{PolicyDecision}
    D -- NoAction --> E[DecisionEnvelope]
    D -- Act --> F[Capability check]
    F -- Denied --> E
    F -- Passed --> G[Intent guardrails]
    G -- Rejected --> E
    G -- Approved --> H{Lowering}
    H -- Failed --> E
    H -- Ok --> I[Action guardrails]
    I -- Rejected --> E
    I -- Approved --> E
    E --> J[Execution or replay]
```

## What this crate does not ship

This crate does not ship:

- An LLM runtime, agent harness, or prompt framework.
- A chat UI or Telegram-style control surface.
- The live MCP or axum server.
- Broker IDs, venue credentials, or hosted execution infrastructure.
- Hosted replay storage, dashboards, or fleet orchestration.
- RBAC, approvals, or team workflow services.

You bring your own runtime. A separate server layer can sit on top of this
protocol for live venue access and product features.

## Current v0 scope

The current implementation is a protocol scaffold with an end-to-end
operations path.

- `DecisionPipeline` runs policy evaluation, capability checks, dual
  guardrails, lowering, and envelope creation.
- Lowering supports these operations intents today:
  `ReducePosition`, `ClosePosition`, `CancelOrder`, `CancelAllOrders`.
- `ExecutionConstraints` accepts a broader constraint block, but v0 only
  enforces `reduce_only` for position-reducing intents.
- Research-mode types are defined but not wired through lowering.
- `DecisionRecorder` writes JSONL and does not provide tamper-evident storage.

## Module map

| Module       | Purpose                                            |
|--------------|----------------------------------------------------|
| `context`    | Owned policy input built from Nautilus snapshots.  |
| `policy`     | `AgentPolicy`, `PolicyDecision`, `PolicyError`.    |
| `intent`     | Semantic action vocabulary and constraints.        |
| `capability` | Observation and action permissions.                |
| `guardrail`  | Intent-level and action-level guardrail traits.    |
| `lowering`   | Intent-to-action translation.                      |
| `action`     | `RuntimeAction`, `TradeAction`, `ResearchCommand`. |
| `pipeline`   | End-to-end decision orchestration.                 |
| `envelope`   | Canonical decision record types.                   |
| `recording`  | JSONL recording for decision envelopes.            |

## Relationship to NautilusTrader

This crate depends on the open NautilusTrader stack:

```text
nautilus-core / nautilus-model / nautilus-common
                      ^
                      |
              nautilus-agents
```

It does not duplicate engine models with protocol-native copies.
`AgentContext` uses the real Nautilus snapshot and report types directly.
`AgentIntent` is the seam between policy reasoning and execution.

## Documentation

See [the docs](https://docs.nautilustrader.io) for API details.

## License

The source code for NautilusTrader is available on GitHub under the
[GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).

---

NautilusTrader is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

Use of this software is subject to the
[Disclaimer](https://nautilustrader.io/legal/disclaimer/).

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="300" height="auto"/>

Copyright 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
