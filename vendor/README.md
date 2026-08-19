# `vendor/`

Two small support crates that the anvil engine links, vendored here in source
form so that this repository builds without reaching for any private
repository.

| crate | what it does |
| --- | --- |
| `foundry-engine-addressing` | Writes the engine's rendezvous record (`~/.anvil/engine.json`) and resolves a live engine address from an env var, that record, or a health probe. |
| `foundry-kit-telemetry` | Content-free local telemetry: safe-token validation, salted-hash identifiers, on-device aggregation, and the versioned rollup envelope. |

Both are the copyright of Prometheus Group LLC and are released under the same
Apache-2.0 license as the rest of this repository (see `../LICENSE`).

They are vendored rather than depended on because upstream they live in a
private monorepo. Vendoring keeps this tree self-contained; the trade-off is
that fixes made upstream must be copied in.
