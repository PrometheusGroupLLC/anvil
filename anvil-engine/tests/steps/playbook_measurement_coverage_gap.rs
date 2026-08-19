use anvil_test_support::{check_def, retained_temp_dir, step_def, Context, StepDef};

const HEARTH_KEY: &str = "hearth_path";
/// The run whose machine cannot be resolved. Its directory is real and its
/// history reads — only the MACHINE is missing.
const GAP_ARTIFACT_ID: &str = "20260728T0000_unresolved";
const HANDLE_KEY: &str = "hearth_path_handle";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a terminal playbook event whose machine cannot be resolved",
            &[],
            &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |_, _| {
                let (handle, hearth) = retained_temp_dir("anvil-playbook-gap-")?;
                // A REAL artifact directory with a READABLE history that
                // contains a correction. The run's machine is still
                // unresolvable — that is what this event is — but nothing about
                // its history is. Without this the grade came back absent
                // because the history could not be read, and the scenario could
                // not tell that apart from the resolution gap it is named for.
                let artifact = hearth.join("unresolved_runs").join(GAP_ARTIFACT_ID);
                std::fs::create_dir_all(&artifact).map_err(|e| e.to_string())?;
                std::fs::write(
                    artifact.join("status.yaml"),
                    "version: 1\nkind: missing_playbook\nstate: unknown_terminal\ntransitions:\n  - to: spec\n    at: \"2026-07-28T00:00:00Z\"\n    actor: fable\n    role: doer\n  - to: spec_revision\n    at: \"2026-07-28T01:00:00Z\"\n    actor: nick\n    role: reviewer\n",
                )
                .map_err(|e| e.to_string())?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "playbook measurement is emitted for the unresolved event",
            &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "RetainedTempDir")],
            |mut ctx, _| {
                let hearth = ctx
                    .get::<std::path::PathBuf>(HEARTH_KEY)
                    .ok_or("No coverage-gap hearth")?;
                let artifact = hearth.join("unresolved_runs").join(GAP_ARTIFACT_ID);
                crate::production_engine::emit_unresolved_playbook_measurement_for_test(
                    hearth, &artifact,
                );
                let hearth = ctx
                    .take::<std::path::PathBuf>(HEARTH_KEY)
                    .ok_or("No coverage-gap hearth")?;
                let handle = ctx
                    .take::<anvil_test_support::RetainedTempDir>(HANDLE_KEY)
                    .ok_or("No coverage-gap hearth handle")?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                out.set(HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        check_def(
            "the playbook-measurement record reports terminal reached {string} outcome {string} success {string}",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, params| {
                let terminal_reached = params.get_string(0).ok_or("Expected terminal reached")?;
                let outcome = params.get_string(1).ok_or("Expected outcome")?;
                let success = params.get_string(2).ok_or("Expected success")?;
                let hearth = ctx
                    .get::<std::path::PathBuf>(HEARTH_KEY)
                    .ok_or("No coverage-gap hearth")?;
                let line = std::fs::read_to_string(hearth.join("playbook-measurement.jsonl"))
                    .map_err(|error| error.to_string())?;
                let record: serde_json::Value =
                    serde_json::from_str(line.trim()).map_err(|error| error.to_string())?;
                if record["terminal_reached"].to_string() == terminal_reached
                    && record["outcome"].as_str() == Some(outcome)
                    && record["success"].to_string() == success
                {
                    Ok(())
                } else {
                    Err(format!("Unexpected coverage-gap record: {}", record))
                }
            },
        ),
        check_def(
            "the playbook-measurement record carries explicit correlation keys for the unresolved event",
            &[(HEARTH_KEY, "PathBuf")],
            |ctx, _| {
                let hearth = ctx
                    .get::<std::path::PathBuf>(HEARTH_KEY)
                    .ok_or("No coverage-gap hearth")?;
                let line = std::fs::read_to_string(hearth.join("playbook-measurement.jsonl"))
                    .map_err(|error| error.to_string())?;
                let record: serde_json::Value =
                    serde_json::from_str(line.trim()).map_err(|error| error.to_string())?;
                for key in [
                    "conversation_hash",
                    "project_label",
                    "playbook_run_id",
                ] {
                    if record[key].as_str().unwrap_or_default().is_empty() {
                        return Err(format!("Coverage-gap record lacks {}: {}", key, record));
                    }
                }
                Ok(())
            },
        ),
    ]
}
