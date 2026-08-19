//! Step module for `step_measurement_sink.feature`.
//!
//! Exercises the real `FileSystemStepMeasurementAdapter` against a temp hearth,
//! proving the append-only + redacted (boolean-only, no prose) contract of the
//! durable per-step measurement sink.

use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core::ports::step_measurement_port::{
    StepMeasurementReadPort, StepMeasurementRecord, StepMeasurementWritePort, STEP_MEASUREMENT_KIND,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const SINK_HEARTH_KEY: &str = "sm_sink_hearth";
const SINK_HANDLE_KEY: &str = "sm_sink_handle";
const SINK_READ_KEY: &str = "sm_sink_read";

fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("Expected true|false, got '{}'", other)),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a step measurement hearth",
            &[],
            &[
                (SINK_HEARTH_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-step-measurement-")?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, tmp);
                out.set::<RetainedTempDir>(SINK_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a step measurement record is appended with from {string}, to {string}, role {string}, intent_present {string}, expected_output_present {string}, at {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[
                (SINK_HEARTH_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let from_state = params.get_string(0).ok_or("Expected from")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let intent_present = parse_bool(params.get_string(3).ok_or("Expected intent_present")?)?;
                let expected_output_present =
                    parse_bool(params.get_string(4).ok_or("Expected expected_output_present")?)?;
                let at = params.get_string(5).ok_or("Expected at")?.to_string();
                let adapter = FileSystemStepMeasurementAdapter::new(&hearth);
                adapter
                    .append_step_measurement(&StepMeasurementRecord {
                        kind: STEP_MEASUREMENT_KIND.to_string(),
                        from_state,
                        to_state,
                        role,
                        intent_present,
                        expected_output_present,
                        at,
                        artifact_kind: String::new(),
                        actor_hash: None,
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        evidence: None,
                    })
                    .map_err(|e| format!("append failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>(SINK_HANDLE_KEY) {
                    out.set::<RetainedTempDir>(SINK_HANDLE_KEY, std::sync::Arc::clone(h));
                }
                Ok(out)
            },
        ),
        step_def(
            "a step measurement record is appended with from {string}, to {string}, role {string}, intent_present {string}, expected_output_present {string}, at {string}, conversation_hash {string}, project_label {string}, playbook_run_id {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[
                (SINK_HEARTH_KEY, "PathBuf"),
                (SINK_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let from_state = params.get_string(0).ok_or("Expected from")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let intent_present = parse_bool(params.get_string(3).ok_or("Expected intent_present")?)?;
                let expected_output_present =
                    parse_bool(params.get_string(4).ok_or("Expected expected_output_present")?)?;
                let at = params.get_string(5).ok_or("Expected at")?.to_string();
                let conversation_hash = params
                    .get_string(6)
                    .ok_or("Expected conversation_hash")?
                    .to_string();
                let project_label = params.get_string(7).ok_or("Expected project_label")?.to_string();
                let playbook_run_id = params
                    .get_string(8)
                    .ok_or("Expected playbook_run_id")?
                    .to_string();
                let adapter = FileSystemStepMeasurementAdapter::new(&hearth);
                adapter
                    .append_step_measurement(&StepMeasurementRecord {
                        kind: STEP_MEASUREMENT_KIND.to_string(),
                        from_state,
                        to_state,
                        role,
                        intent_present,
                        expected_output_present,
                        at,
                        artifact_kind: String::new(),
                        actor_hash: None,
                        conversation_hash: Some(conversation_hash),
                        project_label: Some(project_label),
                        playbook_run_id: Some(playbook_run_id),
                        evidence: None,
                    })
                    .map_err(|e| format!("append failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>(SINK_HANDLE_KEY) {
                    out.set::<RetainedTempDir>(SINK_HANDLE_KEY, std::sync::Arc::clone(h));
                }
                Ok(out)
            },
        ),
        step_def(
            "the step measurement sink is read without any append",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            &[
                (SINK_HEARTH_KEY, "PathBuf"),
                (SINK_READ_KEY, "Vec<StepMeasurementRecord>"),
            ],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let adapter = FileSystemStepMeasurementAdapter::new(&hearth);
                let records = adapter
                    .read_step_measurements()
                    .map_err(|e| format!("read failed: {}", e))?;
                let mut out = Context::new();
                out.set(SINK_HEARTH_KEY, hearth);
                out.set(SINK_READ_KEY, records);
                Ok(out)
            },
        ),
        check_def(
            "the step measurement sink file contains {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let path = hearth.join("step-measurement.jsonl");
                let contents =
                    std::fs::read_to_string(&path).map_err(|e| format!("read sink file: {}", e))?;
                if contents.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Sink file does not contain '{}'. Contents:\n{}",
                        needle, contents
                    ))
                }
            },
        ),
        check_def(
            "the step measurement sink file does not contain {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let path = hearth.join("step-measurement.jsonl");
                let contents =
                    std::fs::read_to_string(&path).map_err(|e| format!("read sink file: {}", e))?;
                if contents.contains(&needle) {
                    Err(format!(
                        "Sink file unexpectedly contains '{}'. Contents:\n{}",
                        needle, contents
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "reading the step measurement sink returns {int} records",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let records = if let Some(r) = ctx.get::<Vec<StepMeasurementRecord>>(SINK_READ_KEY) {
                    r.clone()
                } else {
                    let hearth = ctx
                        .get::<PathBuf>(SINK_HEARTH_KEY)
                        .ok_or("No sink hearth")?
                        .clone();
                    FileSystemStepMeasurementAdapter::new(&hearth)
                        .read_step_measurements()
                        .map_err(|e| format!("read failed: {}", e))?
                };
                if records.len() == expected {
                    Ok(())
                } else {
                    Err(format!("Expected {} records, got {}", expected, records.len()))
                }
            },
        ),
        check_def(
            "the step measurement records contain a record for to_state {string}",
            &[(SINK_HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(SINK_HEARTH_KEY)
                    .ok_or("No sink hearth")?
                    .clone();
                let records = FileSystemStepMeasurementAdapter::new(&hearth)
                    .read_step_measurements()
                    .map_err(|e| format!("read failed: {}", e))?;
                if records.iter().any(|r| r.to_state == to_state) {
                    Ok(())
                } else {
                    Err(format!("No record for to_state '{}'", to_state))
                }
            },
        ),
    ]
}
