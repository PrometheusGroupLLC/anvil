//! Step module for playbook_version.feature.
//!
//! Proves the playbook version is a CONTENT HASH of the loaded PlaybookMachine:
//! a valid machine yields a non-empty version; identical machine content yields
//! an identical version; different machine content yields a different version.
//!
//! Two fixed "slots" (A and B) let one scenario parse two machines and compare
//! their versions. Each compute step loads the doc-string YAML through the real
//! loader and stores the resulting `Option<String>` version under its slot key.

use anvil_core::domain::playbook::loader::load_from_yaml;
use anvil_core::domain::playbook_version::machine_content_version;
use anvil_core_hearth::fs_review_verdict_adapter::FileSystemReviewVerdictAdapter;
use anvil_core_hearth::fs_playbook_measurement_adapter::FileSystemPlaybookMeasurementAdapter;
use anvil_core::ports::review_verdict_port::ReviewVerdictReadPort;
use anvil_core::ports::playbook_measurement_port::PlaybookMeasurementReadPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::sync::{Arc, Mutex};

const SLOT_A_KEY: &str = "pv_version_a";
const SLOT_B_KEY: &str = "pv_version_b";
/// Retained temp dir + parsed read-back result for the adapter round-trip steps.
const RT_DIR_KEY: &str = "pv_rt_dir";
const RT_MEASUREMENT_KEY: &str = "pv_rt_measurement_version";
const RT_VERDICT_KEY: &str = "pv_rt_verdict_version";

/// Load the doc-string machine YAML and compute its content version, returning
/// the `Option<String>` version (None if the machine failed to load).
fn compute_version(yaml: &str) -> Result<Option<String>, String> {
    match load_from_yaml("pv-test-artifact", yaml, &[]) {
        Ok(machine) => Ok(machine_content_version(&machine)),
        Err(e) => Err(format!("machine failed to load: {}", e)),
    }
}

fn slot_version(ctx: &Context, key: &str) -> Result<String, String> {
    ctx.get::<Option<String>>(key)
        .ok_or_else(|| format!("No version stored for slot key '{}'", key))?
        .clone()
        .ok_or_else(|| format!("Slot '{}' version is None (machine unresolvable)", key))
}

/// Write a single raw jsonl line to `<dir>/<filename>` in a fresh retained temp
/// dir, returning the temp-dir handle + path so the caller can read it back.
type RetainedDir = Arc<Mutex<Option<tempfile::TempDir>>>;
fn write_raw_line(filename: &str, line: &str) -> Result<(RetainedDir, std::path::PathBuf), String> {
    let (handle, dir) = anvil_test_support::retained_temp_dir("pv-rt")?;
    std::fs::write(dir.join(filename), format!("{}\n", line))
        .map_err(|e| format!("write {}: {}", filename, e))?;
    Ok((handle, dir))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the playbook version for slot A is computed from machine content:",
            &[],
            &[(SLOT_A_KEY, "Option<String>")],
            |_ctx, params| {
                let yaml = params.doc_string().ok_or("Expected a doc string")?;
                let version = compute_version(yaml)?;
                let mut out = Context::new();
                out.set(SLOT_A_KEY, version);
                Ok(out)
            },
        ),
        step_def(
            "the playbook version for slot B is computed from machine content:",
            &[(SLOT_A_KEY, "Option<String>")],
            &[
                (SLOT_B_KEY, "Option<String>"),
                (SLOT_A_KEY, "Option<String>"),
            ],
            |ctx, params| {
                let yaml = params.doc_string().ok_or("Expected a doc string")?;
                let version = compute_version(yaml)?;
                let mut out = Context::new();
                out.set(SLOT_B_KEY, version);
                // Carry slot A forward (brine retains only `provides`).
                if let Some(a) = ctx.get::<Option<String>>(SLOT_A_KEY) {
                    out.set(SLOT_A_KEY, a.clone());
                }
                Ok(out)
            },
        ),
        check_def(
            "the playbook version for slot A is non-empty",
            &[(SLOT_A_KEY, "Option<String>")],
            |ctx, _params| {
                let version = slot_version(&ctx, SLOT_A_KEY)?;
                if version.is_empty() {
                    Err("Expected a non-empty playbook version but got an empty string".to_string())
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the playbook version for slot A equals slot B",
            &[
                (SLOT_A_KEY, "Option<String>"),
                (SLOT_B_KEY, "Option<String>"),
            ],
            |ctx, _params| {
                let a = slot_version(&ctx, SLOT_A_KEY)?;
                let b = slot_version(&ctx, SLOT_B_KEY)?;
                if a == b {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected identical playbook versions but slot A '{}' != slot B '{}'",
                        a, b
                    ))
                }
            },
        ),
        check_def(
            "the playbook version for slot A differs from slot B",
            &[
                (SLOT_A_KEY, "Option<String>"),
                (SLOT_B_KEY, "Option<String>"),
            ],
            |ctx, _params| {
                let a = slot_version(&ctx, SLOT_A_KEY)?;
                let b = slot_version(&ctx, SLOT_B_KEY)?;
                if a != b {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected different playbook versions but both were '{}'",
                        a
                    ))
                }
            },
        ),
        // ===== Backward-compat: fs adapters round-trip the additive field =====
        step_def(
            "a playbook-measurement.jsonl line without a playbook_version field is read back",
            &[],
            &[
                (RT_MEASUREMENT_KEY, "Option<String>"),
                (RT_DIR_KEY, "RetainedDir"),
            ],
            |_ctx, _params| {
                let line = "{\"kind\":\"playbook_measurement\",\"artifact_kind\":\"track\",\"terminal_state\":\"completed\",\"terminal_reached\":true,\"outcome\":\"terminal_reached\",\"success\":true,\"quality_signal\":\"leading\",\"at\":\"2026-01-01T00:00:00Z\"}";
                let (handle, dir) = write_raw_line("playbook-measurement.jsonl", line)?;
                let records = FileSystemPlaybookMeasurementAdapter::new(&dir)
                    .read_playbook_measurements()
                    .map_err(|e| format!("read playbook measurements: {}", e))?;
                let record = records
                    .into_iter()
                    .next()
                    .ok_or("expected one playbook-measurement record")?;
                let mut out = Context::new();
                out.set(RT_MEASUREMENT_KEY, record.playbook_version);
                out.set(RT_DIR_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a playbook-measurement.jsonl line with playbook_version {string} is read back",
            &[],
            &[
                (RT_MEASUREMENT_KEY, "Option<String>"),
                (RT_DIR_KEY, "RetainedDir"),
            ],
            |_ctx, params| {
                let version = params.get_string(0).ok_or("Expected playbook_version")?;
                let line = format!(
                    "{{\"kind\":\"playbook_measurement\",\"artifact_kind\":\"track\",\"terminal_state\":\"completed\",\"terminal_reached\":true,\"outcome\":\"terminal_reached\",\"success\":true,\"quality_signal\":\"leading\",\"at\":\"2026-01-01T00:00:00Z\",\"playbook_version\":\"{}\"}}",
                    version
                );
                let (handle, dir) = write_raw_line("playbook-measurement.jsonl", &line)?;
                let records = FileSystemPlaybookMeasurementAdapter::new(&dir)
                    .read_playbook_measurements()
                    .map_err(|e| format!("read playbook measurements: {}", e))?;
                let record = records
                    .into_iter()
                    .next()
                    .ok_or("expected one playbook-measurement record")?;
                let mut out = Context::new();
                out.set(RT_MEASUREMENT_KEY, record.playbook_version);
                out.set(RT_DIR_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the read-back playbook-measurement playbook_version is absent",
            &[(RT_MEASUREMENT_KEY, "Option<String>")],
            |ctx, _params| {
                let version = ctx
                    .get::<Option<String>>(RT_MEASUREMENT_KEY)
                    .ok_or("No read-back measurement version")?;
                if version.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected playbook_version None but got {:?}",
                        version
                    ))
                }
            },
        ),
        check_def(
            "the read-back playbook-measurement playbook_version is {string}",
            &[(RT_MEASUREMENT_KEY, "Option<String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook_version")?;
                let version = ctx
                    .get::<Option<String>>(RT_MEASUREMENT_KEY)
                    .ok_or("No read-back measurement version")?;
                match version.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    other => Err(format!(
                        "Expected playbook_version '{}' but got {:?}",
                        expected, other
                    )),
                }
            },
        ),
        step_def(
            "a review-verdict.jsonl line without a playbook_version field is read back",
            &[],
            &[
                (RT_VERDICT_KEY, "Option<String>"),
                (RT_DIR_KEY, "RetainedDir"),
            ],
            |_ctx, _params| {
                let line = "{\"kind\":\"review_verdict\",\"artifact_kind\":\"track\",\"gate_state\":\"review\",\"satisfaction\":\"satisfied\",\"is_final_gate\":true,\"findings\":[],\"intent_confidence\":\"\",\"outcome\":\"ok\",\"at\":\"2026-01-01T00:00:00Z\"}";
                let (handle, dir) = write_raw_line("review-verdict.jsonl", line)?;
                let records = FileSystemReviewVerdictAdapter::new(&dir)
                    .read_review_verdicts()
                    .map_err(|e| format!("read review verdicts: {}", e))?;
                let record = records
                    .into_iter()
                    .next()
                    .ok_or("expected one review-verdict record")?;
                let mut out = Context::new();
                out.set(RT_VERDICT_KEY, record.playbook_version);
                out.set(RT_DIR_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a review-verdict.jsonl line with playbook_version {string} is read back",
            &[],
            &[
                (RT_VERDICT_KEY, "Option<String>"),
                (RT_DIR_KEY, "RetainedDir"),
            ],
            |_ctx, params| {
                let version = params.get_string(0).ok_or("Expected playbook_version")?;
                let line = format!(
                    "{{\"kind\":\"review_verdict\",\"artifact_kind\":\"track\",\"gate_state\":\"review\",\"satisfaction\":\"satisfied\",\"is_final_gate\":true,\"findings\":[],\"intent_confidence\":\"\",\"outcome\":\"ok\",\"at\":\"2026-01-01T00:00:00Z\",\"playbook_version\":\"{}\"}}",
                    version
                );
                let (handle, dir) = write_raw_line("review-verdict.jsonl", &line)?;
                let records = FileSystemReviewVerdictAdapter::new(&dir)
                    .read_review_verdicts()
                    .map_err(|e| format!("read review verdicts: {}", e))?;
                let record = records
                    .into_iter()
                    .next()
                    .ok_or("expected one review-verdict record")?;
                let mut out = Context::new();
                out.set(RT_VERDICT_KEY, record.playbook_version);
                out.set(RT_DIR_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the read-back review-verdict playbook_version is absent",
            &[(RT_VERDICT_KEY, "Option<String>")],
            |ctx, _params| {
                let version = ctx
                    .get::<Option<String>>(RT_VERDICT_KEY)
                    .ok_or("No read-back verdict version")?;
                if version.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected playbook_version None but got {:?}",
                        version
                    ))
                }
            },
        ),
        check_def(
            "the read-back review-verdict playbook_version is {string}",
            &[(RT_VERDICT_KEY, "Option<String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected playbook_version")?;
                let version = ctx
                    .get::<Option<String>>(RT_VERDICT_KEY)
                    .ok_or("No read-back verdict version")?;
                match version.as_deref() {
                    Some(actual) if actual == expected.as_ref() as &str => Ok(()),
                    other => Err(format!(
                        "Expected playbook_version '{}' but got {:?}",
                        expected, other
                    )),
                }
            },
        ),
    ]
}
