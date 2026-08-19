//! Step module for `amendment_conflict_spec.feature` (AC-4).
//!
//! Calls the pure `detect_conflicts` over the shared op log (built by the
//! `amendment_apply` shared steps) and asserts flagged target IDs / count.

use crate::amendment_apply::LOG_KEY;
use anvil_core::domain::amendment::conflict::{detect_conflicts, Conflict};
use anvil_core::domain::amendment::op::OpLog;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const CONFLICTS_KEY: &str = "am_conflicts";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "detect_conflicts is called on the log",
            &[(LOG_KEY, "OpLog")],
            &[(CONFLICTS_KEY, "Vec<Conflict>")],
            |ctx, _p| {
                let log = ctx.get::<OpLog>(LOG_KEY).ok_or("no log")?;
                let conflicts = detect_conflicts(log);
                Ok(Context::new().with(CONFLICTS_KEY, conflicts))
            },
        ),
        check_def(
            "the conflicts contain target {string}",
            &[(CONFLICTS_KEY, "Vec<Conflict>")],
            |ctx, params| {
                let target = params.get_string(0).ok_or("target")?;
                let conflicts = ctx
                    .get::<Vec<Conflict>>(CONFLICTS_KEY)
                    .ok_or("no conflicts")?;
                if conflicts.iter().any(|c| c.target_id == target) {
                    Ok(())
                } else {
                    Err(format!(
                        "no conflict for target '{}': {:?}",
                        target, conflicts
                    ))
                }
            },
        ),
        check_def(
            "the conflicts count is {int}",
            &[(CONFLICTS_KEY, "Vec<Conflict>")],
            |ctx, params| {
                let n: usize = params
                    .get_int(0)
                    .ok_or("count")?
                    .try_into()
                    .map_err(|_| "neg")?;
                let conflicts = ctx
                    .get::<Vec<Conflict>>(CONFLICTS_KEY)
                    .ok_or("no conflicts")?;
                if conflicts.len() == n {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} conflicts, got {}: {:?}",
                        n,
                        conflicts.len(),
                        conflicts
                    ))
                }
            },
        ),
    ]
}
