//! Step definitions for the change record at the anvil-engine seam — the MCP
//! shim is the engine's user, so these drive the real engine binary over gRPC
//! against a real temporary hearth that is a real git repository.

use brine_runner_rust::registry::StepDef;

pub fn steps() -> Vec<StepDef> {
    vec![]
}
