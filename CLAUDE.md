# Anvil

The playbook engine. Read `AGENTS.md` for the development workflow. Read `forge/projections/` for current project state.

## Build and test

**The behavioural suites live in a SECOND workspace, `brine-tests/`.** Read
"Two workspaces" below before running tests — the commands changed.

```bash
# Build all crates. Needs NO private siblings: the default workspace contains
# only the product crates.
cargo build

# ── Behavioural suites — run from `brine-tests/`, NOT the repo root ──────────
cd brine-tests

# Run every feature across all three crates
cargo test --workspace

# Run features for a single crate (useful while iterating).
# NOTE the `-steps` suffix: the runner now lives in the step crate, while the
# features it drives still live beside the product crate it tests.
cargo test --test brine_runner -p anvil-core-steps
cargo test --test brine_runner -p anvil-engine-steps
cargo test --test brine_runner -p anvil-mcp-steps

# Run features through the brine CLI (requires brine's built-in rust adapter
# on the PATH brine searches — see one-time setup below)
brine run .
```

The old commands — `cargo test --test brine_runner -p anvil-core` and its
siblings, run from the repo root — **no longer exist**. The product crates have
no `brine_runner` test target any more.

### Two workspaces

`cargo` loads the manifest of every path dependency reachable from a workspace
member — dev-dependencies included, and switched-off optional dependencies
included. While the brine-dependent step crates were members of the default
workspace, `cargo metadata --no-deps` exited **101** for anyone without the
private `brine` sibling: anvil's workspace could not even be enumerated, let
alone built.

So there are **three** workspace roots. Each one that needs a private sibling is
a separate workspace *for that reason and no other*:

| workspace | members | needs private siblings? |
| --- | --- | --- |
| `Cargo.toml` (default) | `anvil-core`, `anvil-core-hearth`, `anvil-engine`, `anvil-mcp` | no |
| `brine-tests/Cargo.toml` | `anvil-test-support`, `anvil-core-steps`, `anvil-engine-steps`, `anvil-mcp-steps` | yes — `brine` |
| `kit-build/Cargo.toml` | `anvil-kit-engine` | yes — `foundry` |

`cargo build`, `cargo check` and `cargo metadata` at the repo root never load
brine *or* foundry. Everything under `brine-tests/` and `kit-build/` does, and
both are excluded from public exports.

**`kit-build/` builds the engine binary the kit ships.** It is `anvil-engine`
plus the Foundry session verifier: it depends on `foundry-kit-broker-client`
(private, path) and compiles `anvil-engine/src/main.rs` as its own bin target.
The verifier cannot live in `anvil-engine` — an optional, switched-off path
dependency is still manifest-loaded — and it must not be inlined, because
`anvil-core/src/ports/session_verifier.rs` records that anvil contains no inline
JWT parsing, claim checking, or signature logic. So `anvil-engine` holds only the
seam (`DynSessionVerifier` / `EngineVerifier::External`) and `kit-build` injects
the implementation.

Consequence worth knowing before you touch auth: **`scripts/build-kit.sh` is the
only thing that compiles the production verifier arm.** A root `cargo build`, and
every Brine suite, build it dead. That is why the build script gates on the
verifier's symbols being present in the assembled binary.

**Adding a fourth workspace root?** Add its `target/` to `.gitignore` in the same
commit. `/target` is anchored to the repo root and covers exactly one workspace;
this has already been missed twice.

Feature files stay with the PRODUCT crate (`anvil-core/features/**`); only the
runners moved.

### Brine adapter setup (one-time)

Anvil uses brine's built-in `brine-adapter-rust` — there is no custom adapter.
Brine discovers adapters in the directory containing the `brine` executable,
so the one-time setup is to put `brine-adapter-rust` next to it:

```bash
# If brine is installed via `cargo install --path ../brine/cli`, brine lives
# at ~/.cargo/bin/brine. Symlink brine's built-in rust adapter alongside it:
ln -sf "$(realpath ../brine/target/debug/brine-adapter-rust)" \
       ~/.cargo/bin/brine-adapter-rust

# Alternatively, cargo-install the rust adapter directly (same effect):
# cargo install --path ../brine/runners/implementations/rust
```

If `brine run .` reports `Adapter binary 'brine-adapter-rust' not found`, one
of those two commands is missing.

## Workspace layout

Anvil requires sibling repositories:

```
workspace/
  anvil/              # this repo
  anvil-hearth/       # development memory (forge/ symlinks here)
  brine/              # first consumer (symlinks AGENTS.md from here)
  brine-hearth/       # brine's development memory
```

If `forge/` is a broken symlink, clone `anvil-hearth` as a sibling directory.

## Repository contents

- `Cargo.toml` — the DEFAULT workspace root: four product crates, no private
  siblings required. Also holds the temporary `[patch.crates-io]` that resolves
  the two Foundry contract crates until they are published.
- `anvil-core/` — library crate: ports, domain types, hearth reader
  - `anvil-core/features/` — behavioral contract for port interfaces
- `anvil-engine/` — binary crate: long-running engine process (gRPC API)
  - `anvil-engine/features/` — behavioral contract for engine API
- `anvil-mcp/` — binary crate: MCP shim (JSON-RPC over stdio)
  - `anvil-mcp/features/` — behavioral contract for MCP protocol
  - `anvil-mcp/features/e2e/` — end-to-end features exercising shim + engine subprocess
- `brine-tests/` — the SECOND workspace: everything that depends on the private
  `brine` sibling. Excluded from the default workspace so `cargo build` never
  loads brine.
  - `brine-tests/anvil-test-support/` — shared step modules and the brine-runner
    harness used by every runner
  - `brine-tests/anvil-{core,engine,mcp}-steps/` — step modules, the
    `tests/brine_runner.rs` test binary, and the `.brine` manifest for the
    matching product crate's features
- `proto/anvil.proto` — gRPC service definition (AnvilService: Catalog + HealthCheck)
- `AGENTS.md` — authoritative development-process definition (brine symlinks to this copy)
- `forge/` — symlink to `../anvil-hearth` (anvil's own development artifacts)
- `.hearth` — config pointing to `../anvil-hearth`

## MCP server configuration

To use anvil as an MCP server in Claude Code, add to `.mcp.json` at the project root:

```json
{
  "mcpServers": {
    "anvil": {
      "command": "/path/to/anvil/target/debug/anvil-mcp"
    }
  }
}
```

The project directory must contain a `.hearth` file pointing to the hearth directory. The shim reads `.hearth` on startup (from cwd), starts the engine automatically on first `catalog` call, and manages the engine lifecycle for the session.

For Claude Desktop, add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "anvil": {
      "command": "/path/to/anvil/target/debug/anvil-mcp"
    }
  }
}
```

Claude Desktop does not pass `cwd`. Instead, the shim reads the workspace root from the MCP `initialize` request's `roots` parameter and looks for `.hearth` there. One server entry works for all projects that have a `.hearth` file. Falls back to cwd if roots are not provided.

Available tools:
- `catalog` — returns active artifacts (id, type, state, summary) and available artifact types (name, description, requires_parent) from the hearth

## Rules

1. **Follow the lifecycle.** Development follows propose → spec → plan → implement → reflect → complete, with review at every transition. See `AGENTS.md`.

2. **Nothing with behavior is built without `.feature` files binding intent to execution.**

3. **All tests are `.feature` files run through brine. No raw unit tests for behavioral testing.**

4. **Tests are written from the user's perspective at each seam.** Each crate's `features/` directory tests from the perspective of that crate's user. The MCP shim's user is Claude Code. The engine's user is the MCP shim. The core library's user is the engine.

5. **Test-driven development. Write the failing test first.**

6. **Prefer declarative over imperative, immutable over mutable.**

## Maintaining this file

This file loads into every agent conversation working on anvil. If it's wrong, every agent is wrong. Update it when the repository structure or rules change.
