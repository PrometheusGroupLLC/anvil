//! Step module for `amendment_reversal_spec.feature` (AC-5).
//!
//! Calls the pure `reverse(log, op_id)` over the shared op log, storing the
//! result under `am_reversed_log` so the `amendment_apply` shared apply step can
//! fold it.

use crate::amendment_apply::LOG_KEY;
use anvil_core::domain::amendment::op::OpLog;
use anvil_core::domain::amendment::reverse::reverse;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{step_def, StepDef};

const REVERSED_LOG_KEY: &str = "am_reversed_log";

pub fn steps() -> Vec<StepDef> {
    vec![step_def(
        "reverse is called on the log for op {string}",
        &[(LOG_KEY, "OpLog")],
        &[(REVERSED_LOG_KEY, "OpLog"), ("amv_doc", "ArtifactDocument")],
        |ctx, params| {
            let op_id = params.get_string(0).ok_or("op id")?;
            let log = ctx.get::<OpLog>(LOG_KEY).ok_or("no log")?;
            let reversed = reverse(log, op_id);
            // Carry forward the base doc so the subsequent apply step has it.
            let mut out = Context::new().with(REVERSED_LOG_KEY, reversed);
            if let Some(doc) =
                ctx.get::<anvil_core::domain::amendment::document::ArtifactDocument>("amv_doc")
            {
                out.set("amv_doc", doc.clone());
            }
            Ok(out)
        },
    )]
}
