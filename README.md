# Anvil

**The forge playbook engine** — an event-sourced development lifecycle system
for agent-assisted software projects.

Anvil governs how coding agents and humans collaborate on software. Instead of
letting an agent improvise its way through a change, anvil serves it a
**playbook**: an explicit state machine for a unit of work, with defined phases,
review gates, and the guidance appropriate to whichever phase the work is
currently in. The engine records what actually happened as an append-only event
log, so the process is measurable rather than merely asserted.

It is project-agnostic. Any project can adopt the forge lifecycle by pointing at
anvil and maintaining its own **hearth** — a git repository holding that
project's development memory (proposals, tracks, decisions, learnings).

> ### Read this first
>
> This repository is a **public mirror** of a private development repository,
> and **it does not currently build for outside contributors** — the workspace
> depends on private sibling repositories that are not yet published. Both
> points are explained honestly in [CONTRIBUTING.md](CONTRIBUTING.md), and the
> build situation is summarised under [Build status](#build-status) below.
>
> You can read the source and the specification today. You cannot compile it
> today. We would rather tell you that up front.

## What is in this repository

| Crate | What it is |
| --- | --- |
| `anvil-core` | Library: ports, domain types, playbook loader, hearth reader |
| `anvil-core-hearth` | Hearth storage adapters and maintenance binaries |
| `anvil-engine` | The long-running engine process, exposing a gRPC API |
| `anvil-mcp` | An MCP shim (JSON-RPC over stdio) that fronts the engine for agents |
| `anvil-test-support` | Shared test harness and fixtures |
| `anvil-core-steps`, `anvil-engine-steps`, `anvil-mcp-steps` | Gherkin step definitions binding the `.feature` files to real execution |

`proto/anvil.proto` defines the gRPC service.

## The specification is the `.feature` files

This is the most useful thing to read here, and it needs no toolchain.

Anvil's central claim is that **nothing with behaviour is built without a
`.feature` file binding intent to execution**. There are no raw unit tests for
behavioural coverage — every behavioural guarantee is a Gherkin scenario, and
each crate's `features/` directory tests that crate from the perspective of its
own consumer:

- `anvil-mcp/features/` — written from the perspective of the coding agent.
- `anvil-engine/features/` — written from the perspective of the MCP shim.
- `anvil-core/features/` — written from the perspective of the engine.

If you want to know what anvil actually promises, read those files rather than
this README. Reviewing them for gaps or wrong behaviour is a genuinely useful
contribution and requires nothing installed.

## Build status

**Honest summary: outside contributors cannot build this repository yet.**

The workspace declares non-optional path dependencies on private sibling
repositories that have not been published — most importantly the Gherkin runner
that executes the `.feature` suite. Because the crate holding those
dependencies is a workspace member, Cargo fails at *manifest resolution*, before
compiling anything. `cargo build`, `cargo check`, and `cargo metadata` all fail
identically.

This is a known defect of the public release, not a problem with your
environment. [CONTRIBUTING.md](CONTRIBUTING.md#2-the-hard-part-you-probably-cannot-build-this-yet)
sets out the exact failure, the three fixes planned to remove it, and what is
worth doing in the meantime.

There is deliberately **no CI in this repository** — no GitHub Actions
workflows at all. That is a security decision: with no workflows, a pull request
from a fork has nothing to execute. It also means no automated checks will run
on your PR.

## MCP tools

Once running, the engine exposes these tools to an agent through the MCP shim:

- `catalog` — active artifacts and the artifact types available in the hearth
- `describe` — available actions and lifecycle context for the current state
- `begin` — start a new artifact, such as a track
- `checkin` — record progress on in-flight work
- `snapshot` — capture the non-deterministic remainder of a piece of work
- `complete` — close out an artifact through its review gate

## Contributing

Contributions are welcome, but the workflow here is not the usual one — your
pull request will be **closed rather than merged**, with your patch reapplied
upstream preserving your authorship, and it will reappear here in the next
export. All commits must carry a `Signed-off-by` line under the
[Developer Certificate of Origin](dco.txt).

Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

For security vulnerabilities, do not open a public issue — see
[SECURITY.md](SECURITY.md).

## License

Anvil is released under the **Apache License, Version 2.0** (SPDX:
`Apache-2.0`). The full text is in [LICENSE](LICENSE).

You are free to use, modify, and distribute Anvil for any purpose, including
commercially, subject to the terms of that license. The software is provided
**as is**, without warranties or conditions of any kind. Copyright is retained
by Prometheus Group LLC.

This summary is for orientation only; [LICENSE](LICENSE) is the binding text.

Copyright © 2026 Prometheus Group LLC. Authored by Nicholas Rinard.
