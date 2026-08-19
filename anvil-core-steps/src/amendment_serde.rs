//! Step module for `amendment_oplog_roundtrip.feature` (KD-2).
//!
//! Round-trips the shared `OpLog` through serde (YAML), asserting the
//! deserialized log is identical and that `ordered()` is stable across the
//! round-trip.

use crate::amendment_apply::LOG_KEY;
use anvil_core::domain::amendment::op::OpLog;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const ORIGINAL_KEY: &str = "am_serde_original";
const ROUNDTRIP_KEY: &str = "am_serde_roundtrip";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the op log is serialized and deserialized",
            &[(LOG_KEY, "OpLog")],
            &[(ORIGINAL_KEY, "OpLog"), (ROUNDTRIP_KEY, "OpLog")],
            |ctx, _p| {
                let original = ctx.get::<OpLog>(LOG_KEY).ok_or("no log")?.clone();
                let yaml = serde_yaml::to_string(&original)
                    .map_err(|e| format!("serialize failed: {}", e))?;
                let roundtrip: OpLog = serde_yaml::from_str(&yaml)
                    .map_err(|e| format!("deserialize failed: {}", e))?;
                Ok(Context::new()
                    .with(ORIGINAL_KEY, original)
                    .with(ROUNDTRIP_KEY, roundtrip))
            },
        ),
        check_def(
            "the deserialized log equals the original log",
            &[(ORIGINAL_KEY, "OpLog"), (ROUNDTRIP_KEY, "OpLog")],
            |ctx, _p| {
                let original = ctx.get::<OpLog>(ORIGINAL_KEY).ok_or("no original")?;
                let roundtrip = ctx.get::<OpLog>(ROUNDTRIP_KEY).ok_or("no roundtrip")?;
                if original == roundtrip {
                    Ok(())
                } else {
                    Err("deserialized log differs from original".to_string())
                }
            },
        ),
        check_def(
            "the deserialized log ordered() sequence equals the original ordered() sequence",
            &[(ORIGINAL_KEY, "OpLog"), (ROUNDTRIP_KEY, "OpLog")],
            |ctx, _p| {
                let original = ctx.get::<OpLog>(ORIGINAL_KEY).ok_or("no original")?;
                let roundtrip = ctx.get::<OpLog>(ROUNDTRIP_KEY).ok_or("no roundtrip")?;
                if original.ordered() == roundtrip.ordered() {
                    Ok(())
                } else {
                    Err("ordered() sequence differs across round-trip".to_string())
                }
            },
        ),
        check_def(
            "the ordered() op_id sequence is {string}",
            &[(ROUNDTRIP_KEY, "OpLog")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("expected sequence")?;
                let roundtrip = ctx.get::<OpLog>(ROUNDTRIP_KEY).ok_or("no roundtrip")?;
                let actual: Vec<String> =
                    roundtrip.ordered().into_iter().map(|e| e.op_id).collect();
                let actual_str = actual.join(",");
                if actual_str == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected ordered op_ids '{}' got '{}'",
                        expected, actual_str
                    ))
                }
            },
        ),
    ]
}
