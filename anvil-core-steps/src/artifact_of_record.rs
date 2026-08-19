//! Steps for `artifact_of_record.feature` — the shared authority for the
//! artifact of record and its bootstrap bytes.

use anvil_core::domain::begin::artifact_of_record;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{
    check_def,
    step_def,
    StepDef,
};

const PATH_KEY: &str = "aor_path";
const BYTES_KEY: &str = "aor_placeholder";
const KIND_KEY: &str = "aor_kind";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the artifact of record is resolved for kind {string} state {string} display name {string}",
            &[],
            &[(PATH_KEY, "String"), (BYTES_KEY, "String"), (KIND_KEY, "String")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let name = params.get_string(2).ok_or("Expected display name")?.to_string();
                let resolved = artifact_of_record(&kind, &state, &name);
                let mut out = Context::new();
                match resolved {
                    Some((path, bytes)) => {
                        out.set(PATH_KEY, path);
                        out.set(BYTES_KEY, String::from_utf8_lossy(&bytes).to_string());
                    }
                    None => {
                        out.set(PATH_KEY, String::new());
                        out.set(BYTES_KEY, String::new());
                    }
                }
                out.set(KIND_KEY, format!("{kind}|{name}"));
                Ok(out)
            },
        ),
        check_def(
            "the artifact of record path is {string}",
            &[(PATH_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected path")?.to_string();
                let got = ctx.get::<String>(PATH_KEY).ok_or("No resolution")?;
                if *got == want { Ok(()) } else { Err(format!("path was {got:?}, expected {want:?}")) }
            },
        ),
        check_def(
            "the artifact of record placeholder is {string}",
            &[(BYTES_KEY, "String")],
            |ctx, params| {
                // Feature tables cannot carry a literal newline.
                let want = params.get_string(0).ok_or("Expected bytes")?.replace("\\n", "\n");
                let got = ctx.get::<String>(BYTES_KEY).ok_or("No resolution")?;
                if *got == want { Ok(()) } else { Err(format!("placeholder was {got:?}, expected {want:?}")) }
            },
        ),
        check_def(
            "there is no artifact of record",
            &[(PATH_KEY, "String")],
            |ctx, _params| {
                let got = ctx.get::<String>(PATH_KEY).ok_or("No resolution")?;
                if got.is_empty() { Ok(()) } else { Err(format!("expected None, got path {got:?}")) }
            },
        ),
    ]
}
