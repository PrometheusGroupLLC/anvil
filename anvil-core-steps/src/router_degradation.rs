//! Step module for `anvil-core/features/router_degradation_cause.feature`.
//!
//! The classifier's input is a real kiln error envelope. These steps BUILD that
//! envelope from the code + message the scenario names — the same shape
//! `kiln-serve` emits (`{"error":{"message":…,"type":"kiln","code":…}}`) — rather
//! than handing the classifier a pre-decided cause, so what is exercised is the
//! parse and the arm choice, not the step's own opinion.
//!
//! The `body` steps hand over raw bytes with no envelope at all (an HTML error
//! page, an empty body), which is the path where only the status class is left.

use anvil_core::domain::hooks::router_degradation::{classify_kiln_failure, degradation_notice};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const CAUSE_KEY: &str = "rdc_cause";
const NOTICE_KEY: &str = "rdc_notice";

/// The kiln error envelope, verbatim in shape. `-` for a message means the field
/// carries the empty string.
fn envelope(code: &str, message: &str) -> String {
    serde_json::json!({
        "error": { "message": message, "type": "kiln", "code": code }
    })
    .to_string()
}

fn classified(status: i64, body: &str) -> Context {
    let mut out = Context::new();
    out.set(
        CAUSE_KEY,
        classify_kiln_failure(status as u16, body).as_str().to_string(),
    );
    out
}

fn rendered(cause: &str, degraded: bool) -> Context {
    let mut out = Context::new();
    out.set(NOTICE_KEY, degradation_notice(cause, degraded));
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a kiln reply with status {int} code {string} message {string} is classified",
            &[],
            &[(CAUSE_KEY, "String")],
            |_ctx, params| {
                let status = params.get_int(0).ok_or("Expected status")?;
                let code = params.get_string(1).ok_or("Expected code")?.to_string();
                let message = params.get_string(2).ok_or("Expected message")?.to_string();
                Ok(classified(status, &envelope(&code, &message)))
            },
        ),
        step_def(
            "a kiln reply with status {int} and body {string} is classified",
            &[],
            &[(CAUSE_KEY, "String")],
            |_ctx, params| {
                let status = params.get_int(0).ok_or("Expected status")?;
                let body = params.get_string(1).ok_or("Expected body")?.to_string();
                Ok(classified(status, &body))
            },
        ),
        check_def(
            "the classified router cause is {string}",
            &[(CAUSE_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected cause")?.to_string();
                let got = ctx.get::<String>(CAUSE_KEY).ok_or("Nothing was classified")?;
                if *got == want {
                    Ok(())
                } else {
                    Err(format!("classified cause was {got:?}, expected {want:?}"))
                }
            },
        ),
        check_def(
            "the classified router cause is not {string}",
            &[(CAUSE_KEY, "String")],
            |ctx, params| {
                let unwanted = params.get_string(0).ok_or("Expected cause")?.to_string();
                let got = ctx.get::<String>(CAUSE_KEY).ok_or("Nothing was classified")?;
                if *got == unwanted {
                    Err(format!("classified cause was {got:?}, which it must never be"))
                } else {
                    Ok(())
                }
            },
        ),
        step_def(
            "the degradation notice is rendered for cause {string} degrading to lexical",
            &[],
            &[(NOTICE_KEY, "String")],
            |_ctx, params| {
                let cause = params.get_string(0).ok_or("Expected cause")?.to_string();
                Ok(rendered(&cause, true))
            },
        ),
        step_def(
            "the degradation notice is rendered for cause {string} with no playbook suggested",
            &[],
            &[(NOTICE_KEY, "String")],
            |_ctx, params| {
                let cause = params.get_string(0).ok_or("Expected cause")?.to_string();
                Ok(rendered(&cause, false))
            },
        ),
        check_def(
            "the degradation notice is {string}",
            &[(NOTICE_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected notice")?.to_string();
                let got = ctx.get::<String>(NOTICE_KEY).ok_or("No notice rendered")?;
                if *got == want {
                    Ok(())
                } else {
                    Err(format!("notice was {got:?}, expected {want:?}"))
                }
            },
        ),
        check_def(
            "the degradation notice contains {string}",
            &[(NOTICE_KEY, "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let got = ctx.get::<String>(NOTICE_KEY).ok_or("No notice rendered")?;
                if got.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("notice {got:?} does not contain {needle:?}"))
                }
            },
        ),
    ]
}
