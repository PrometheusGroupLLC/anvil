# Anvil

> **Note for readers of the public mirror.**
> This file is the working brief used inside the *private* development
> repository, and it is reproduced here unedited so you can see the rules the
> code is actually held to. It therefore refers to things that are **not
> present in this repository**: the `forge/` hearth symlink, the `.hearth` and
> `.mcp.json` config files, the `kit/` packaging tree, the `.claude/commands/`
> prompt files, and the sibling `brine` and `anvil-hearth` repositories.
>
> Those are excluded from the public mirror deliberately — see
> [CONTRIBUTING.md](CONTRIBUTING.md). **The build and test commands below will
> not work here**, because the workspace depends on private sibling
> repositories that have not been published. Treat this file as a statement of
> the project's standards, not as setup instructions you can follow.

The forge playbook engine. Read `AGENTS.md` for the development workflow.

## Build and test

```bash
# Build all crates
cargo build

# Run every feature across all three crates
cargo test --workspace

# Run features for a single crate (useful while iterating)
cargo test --test brine_runner -p anvil-core
cargo test --test brine_runner -p anvil-engine
cargo test --test brine_runner -p anvil-mcp

# Run features through the brine CLI (requires brine's built-in rust adapter
# on the PATH brine searches — see one-time setup below)
brine run .
```

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
  brine/              # first consumer (symlinks AGENTS.md and .claude/commands/forge/ from here)
  brine-hearth/       # brine's development memory
```

If `forge/` is a broken symlink, clone `anvil-hearth` as a sibling directory.

## Repository contents

- `Cargo.toml` — workspace root with four members
- `anvil-core/` — library crate: ports, domain types, hearth reader
  - `anvil-core/features/` — behavioral contract for port interfaces
  - `anvil-core/.brine` — manifest for core features
  - `anvil-core/tests/brine_runner.rs` — per-crate test binary
- `anvil-engine/` — binary crate: long-running engine process (gRPC API)
  - `anvil-engine/features/` — behavioral contract for engine API
  - `anvil-engine/.brine` — manifest for engine features
  - `anvil-engine/tests/brine_runner.rs` — per-crate test binary
- `anvil-mcp/` — binary crate: MCP shim (JSON-RPC over stdio)
  - `anvil-mcp/features/` — behavioral contract for MCP protocol
  - `anvil-mcp/features/e2e/` — end-to-end features exercising shim + engine subprocess
  - `anvil-mcp/.brine` — manifest for MCP features
  - `anvil-mcp/tests/brine_runner.rs` — per-crate test binary
- `anvil-test-support/` — dev-dep-only crate: shared step modules and brine-runner harness used by every crate's `tests/brine_runner.rs`
- `proto/anvil.proto` — gRPC service definition (AnvilService: Catalog + HealthCheck)
- `AGENTS.md` — authoritative development-process definition (brine symlinks to this copy)
- `.claude/commands/forge/` — skill prompt files (brine symlinks to this directory)
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

1. **Follow the forge lifecycle.** Development follows propose → spec → plan → implement → reflect → complete, with review at every transition. See `AGENTS.md`.

2. **Nothing with behavior is built without `.feature` files binding intent to execution.**

3. **All tests are `.feature` files run through brine. No raw unit tests for behavioral testing.**

4. **Tests are written from the user's perspective at each seam.** Each crate's `features/` directory tests from the perspective of that crate's user. The MCP shim's user is Claude Code. The engine's user is the MCP shim. The core library's user is the engine.

5. **Test-driven development. Write the failing test first.**

6. **Prefer declarative over imperative, immutable over mutable.**

## Maintaining this file

This file loads into every agent conversation working on anvil. If it's wrong, every agent is wrong. Update it when the repository structure or rules change.
