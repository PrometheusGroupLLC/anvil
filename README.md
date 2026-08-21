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
> **This repository builds.** Clone it, install `protoc`, and `cargo test
> --workspace --locked` goes green — see [Building](#building) for the exact
> commands and what was measured. An earlier version of this file said the
> opposite; that was true at the initial release and was fixed on 2026-08-20.
>
> Two things are still worth knowing up front. This repository is a **one-way
> curated export** of a private development repository — see [Relationship to
> the private repository](#relationship-to-the-private-repository). And while
> the 504 `.feature` files that constitute anvil's specification all ship here,
> **they cannot be executed here**, because the Gherkin runner they bind to is
> not ours to publish. The specification is readable; that particular suite is
> not runnable. Details under [The specification is the `.feature`
> files](#the-specification-is-the-feature-files).

## What is in this repository

The Rust workspace has exactly four members:

| Crate | What it is |
| --- | --- |
| `anvil-core` | Library: the domain and the ports. Playbook loader and registries, and the lifecycle domain — begin, complete, snapshot, amend, route, checkin, catalog, describe, the transition log |
| `anvil-core-hearth` | The filesystem adapters that implement those ports against a hearth, plus three maintenance binaries (`anvil-hearth-probe`, `anvil-status-header-reconcile`, `anvil-change-record-import`) |
| `anvil-engine` | The long-running engine process, exposing a gRPC API. Also builds `anvil-hooks`, the per-harness hook installer and runtime gate |
| `anvil-mcp` | An MCP shim (JSON-RPC over stdio) that fronts the engine for agents |

Alongside them:

- `proto/anvil.proto` — the gRPC service definition.
- `playbooks/` — the state machines the engine actually runs. Not documentation;
  the engine loads these, and the integrity tests read them.
- `docs/authoring-playbooks.md`, `docs/vocabulary.md` — how to write a playbook,
  and what the terms mean.
- `PHILOSOPHY.md`, `AGENTS.md`, `CLAUDE.md` — the design doctrine and the
  development process the project holds itself to.

## Building

You need a Rust toolchain and **`protoc`**. `anvil-engine/build.rs` compiles
`proto/anvil.proto` through `tonic-build`, which shells out to `protoc`. Without
it the build fails inside a build script — "Could not find protoc" — and cargo
exits 101. On macOS, `brew install protobuf`; on Debian/Ubuntu, `apt install
protobuf-compiler`.

Then:

```sh
git clone https://github.com/PrometheusGroupLLC/anvil.git
cd anvil
cargo test --workspace --locked
```

### What was actually measured

Run on the exported tree extracted somewhere neither its parent nor its
grandparent contains any private sibling, with a throwaway `CARGO_HOME` and no
`.cargo/config.toml` in any ancestor — so nothing off the build machine could be
silently supplied:

```
cargo metadata --no-deps --locked                exit=0
cargo check --workspace --all-targets --locked   exit=0
cargo test --workspace --locked                  exit=0
```

`cargo test` ran 18 test binaries: **104 tests, 0 failures, 0 ignored**. The
same three commands were also run against a fresh unauthenticated clone of this
repository with an empty `CARGO_HOME`, forcing a cold download of every
dependency from crates.io — same result.

Be precise about what those 104 tests are. They are the workspace's Rust unit
tests, doc-tests, and its structural guards — the checks that no crate reaches
outside the repository, that the shipped playbooks parse, that parallel copies
have not drifted. They are **not** the 504 `.feature` scenarios, which do not
run here. "The workspace builds and its 104 Rust tests pass from a bare clone"
is the claim; "the test suite passes" would be a larger one than the evidence
supports.

Verified with `cargo 1.97.1` / `rustc 1.97.1` and `libprotoc 29.3` on macOS
arm64. Other platforms and toolchains are not claimed — they are untested here,
not known-broken. The build is not warning-free; three dead-code warnings in
`anvil-engine` are known.

## The specification is the `.feature` files

This is the most useful thing to read here, and it needs no toolchain.

Anvil's central claim is that **nothing with behaviour is built without a
`.feature` file binding intent to execution**. Behavioural guarantees are
Gherkin scenarios rather than hand-written unit tests, and each crate's
`features/` directory tests that crate from the perspective of its own consumer:

| Directory | Written from the perspective of | Scenarios |
| --- | --- | --- |
| `anvil-mcp/features/` | the coding agent | 78 files |
| `anvil-engine/features/` | the MCP shim | 166 files |
| `anvil-core/features/` | the engine | 260 files |

If you want to know what anvil actually promises, read those files rather than
this README. Reviewing them for gaps or wrong behaviour is a genuinely useful
contribution and requires nothing installed.

**They do not execute in this repository.** The step definitions that bind them
to running code, and the `.brine` manifests that drive them, live in crates that
depend on Brine, a Gherkin runner that is a third party's work and not ours to
publish. Those crates are therefore not exported,
so `cargo test` here does not touch the `.feature` files at all. When the runner
is published, the suites ship with the repository. Until then, treat this
directory as specification you can read and argue with, not as a suite you can
run. The Rust tests described under [Building](#building) are a different and
much smaller thing.

## Relationship to the private repository

This repository is a **one-way curated export** of a private development
repository. It is produced by an export script working from an explicit
include-list: anything not named is excluded by construction. Development memory,
internal contracts, agent prompts, vendored assets and local configuration do
not cross over, and the export is gated on a tree-wide sweep plus an isolated
build before anything is published.

Two consequences worth stating plainly:

- **There is no CI here.** No `.github/` directory and no workflow of any kind —
  the export refuses to run if one appears. So no automated checks run on your
  pull request; everything, the DCO sign-off included, is checked by hand at
  review time.
- **History is export history.** Commits arrive in curated batches rather than
  as the private repository's own commit stream.

## MCP tools

Once running, the engine exposes 23 tools to an agent through the MCP shim.
The lifecycle surface is:

- `anvil_orchestrate` — the general surface-to-anvil handoff; usually the entry
  point
- `catalog` — active artifacts and the artifact types available in the hearth
- `checkin` — orient in the current work and get the next lifecycle step
- `describe` — available actions and lifecycle context for the current state
- `begin` — start a new artifact, such as a track, or re-enter an existing one
- `snapshot` — drive a state transition
- `complete` — close out the current pass through its review gate
- `amend` — record a structured amendment against a frozen document
- `persist_playbook`, `candidate_playbook_intake` — register playbooks
- `begin_adoption_status` — adoption reporting

The remaining twelve are the `backlog_*` family (shaping, ranking, reshuffle
proposal and commit, execution binding, outcome sign-off, queue and cross-organ
views).

## Contributing

Contributions are welcome, but the workflow here is not the usual one — your
pull request will be **closed rather than merged**, with your patch reapplied
upstream preserving your authorship, and it will reappear here in the next
export. All commits must carry a `Signed-off-by` line under the
[Developer Certificate of Origin](dco.txt).

Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request —
with one warning. Its section 2, "The hard part: you probably cannot build this
yet", is **out of date**: it describes a manifest-resolution failure that no
longer occurs, names crates that are no longer in the tree, and links a
`vendor/` directory that does not exist. [Building](#building) above supersedes
it. The rest of that document — the contribution workflow, the DCO, the review
cadence — is current.

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
