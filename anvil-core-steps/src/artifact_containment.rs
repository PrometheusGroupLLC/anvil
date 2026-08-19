//! Step definitions for `anvil_core_hearth::containment` — the artifact-path
//! containment guard shared by the MCP shim and the engine fs adapters.
//!
//! The syntax scenarios drive the pure `is_syntactically_invalid` /
//! `contained_relative_path` / `escapes_hearth` functions directly. The
//! engine-boundary scenarios drive the REAL `FileSystemSnapshotAdapter` against
//! a temp hearth that contains an in-hearth symlink pointing OUT of the hearth,
//! proving the fs write refuses an escaping path (defense in depth) even though
//! no shim normalization ran.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::TransitionContent;
use anvil_core_hearth::containment;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const TRACK_REL: &str = "tracks/20260420T0100_contain_track";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a containment hearth with a track at {string}",
            &[],
            &[
                ("ct_hearth", "PathBuf"),
                ("ct_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?.to_string();
                let (handle, hearth) = retained_temp_dir("anvil-containment-")?;
                let dir = hearth.join(TRACK_REL);
                std::fs::create_dir_all(&dir).map_err(|e| format!("create track dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: track\nstate: {state}\nactors: {{}}\ntransitions:\n  - to: {state}\n    at: 2026-04-20T00:00:00Z\n    actor: Author-000001\n    role: spec\n",
                    state = state
                );
                std::fs::write(dir.join("status.yaml"), status)
                    .map_err(|e| format!("write status.yaml: {}", e))?;
                std::fs::write(dir.join("spec.md"), "# Contain Track\n\nBody.\n")
                    .map_err(|e| format!("write spec.md: {}", e))?;
                let mut out = Context::new();
                out.set("ct_hearth", hearth);
                out.set("ct_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a symlink {string} inside the hearth pointing outside it",
            &[("ct_hearth", "PathBuf")],
            &[
                ("ct_hearth", "PathBuf"),
                ("ct_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("ct_external_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let link_name = params
                    .get_string(0)
                    .ok_or("Expected link name")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("ct_hearth")
                    .ok_or("No ct_hearth")?
                    .clone();
                // A real directory OUTSIDE the hearth, with a matching track
                // subtree so a naive join would successfully write into it.
                let (external_handle, external) = retained_temp_dir("anvil-containment-outside-")?;
                std::fs::create_dir_all(external.join(TRACK_REL))
                    .map_err(|e| format!("create external track dir: {}", e))?;
                let link_path = hearth.join(&link_name);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&external, &link_path)
                    .map_err(|e| format!("create symlink: {}", e))?;
                #[cfg(not(unix))]
                return Err("symlink containment scenario requires a unix platform".to_string());
                let mut out = Context::new();
                out.set("ct_hearth", hearth);
                out.set("ct_external_handle", external_handle);
                carry_retained_temp_dir(&ctx, &mut out, "ct_hearth_handle");
                Ok(out)
            },
        ),
        step_def(
            "a transition to {string} is recorded for artifact_path {string}",
            &[("ct_hearth", "PathBuf")],
            &[
                ("ct_hearth", "PathBuf"),
                ("ct_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("ct_external_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("ct_write_result", "String"),
            ],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to")?.to_string();
                let artifact_path = params
                    .get_string(1)
                    .ok_or("Expected artifact_path")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("ct_hearth")
                    .ok_or("No ct_hearth")?
                    .clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
                let transition = TransitionContent {
                    to,
                    at: "2026-04-20T01:00:00Z".to_string(),
                    actor: "Author-000001".to_string(),
                    role: "spec".to_string(),
                    approver: None,
                    note: None,
                    satisfaction: None,
                    event_type: None,
                };
                let result = match adapter.append_transition(&artifact_path, &transition) {
                    Ok(()) => "ok".to_string(),
                    Err(e) => format!("err: {}", e),
                };
                let mut out = Context::new();
                out.set("ct_hearth", hearth);
                out.set("ct_write_result", result);
                carry_retained_temp_dir(&ctx, &mut out, "ct_hearth_handle");
                carry_retained_temp_dir(&ctx, &mut out, "ct_external_handle");
                Ok(out)
            },
        ),
        check_def(
            "the artifact_path {string} is syntactically invalid",
            &[],
            |_ctx, params| {
                let path = params.get_string(0).ok_or("Expected artifact_path")?;
                if containment::is_syntactically_invalid(path.as_ref()) {
                    Ok(())
                } else {
                    Err(format!("Expected '{}' to be syntactically invalid", path))
                }
            },
        ),
        check_def(
            "the artifact_path {string} is syntactically valid",
            &[],
            |_ctx, params| {
                let path = params.get_string(0).ok_or("Expected artifact_path")?;
                if containment::is_syntactically_invalid(path.as_ref()) {
                    Err(format!("Expected '{}' to be syntactically valid", path))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the artifact_path {string} is contained and normalizes to {string}",
            &[("ct_hearth", "PathBuf")],
            |ctx, params| {
                let path = params.get_string(0).ok_or("Expected artifact_path")?;
                let expected = params.get_string(1).ok_or("Expected normalized form")?;
                let hearth = ctx.get::<PathBuf>("ct_hearth").ok_or("No ct_hearth")?;
                match containment::contained_relative_path(hearth, path.as_ref()) {
                    Ok(relative) => {
                        let got = relative.to_string_lossy().to_string();
                        if got == expected.as_ref() as &str {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected '{}' to normalize to '{}', got '{}'",
                                path, expected, got
                            ))
                        }
                    }
                    Err(reason) => Err(format!(
                        "Expected '{}' to be contained, but it was refused: {}",
                        path, reason
                    )),
                }
            },
        ),
        check_def(
            "the artifact_path {string} escapes the hearth",
            &[("ct_hearth", "PathBuf")],
            |ctx, params| {
                let path = params.get_string(0).ok_or("Expected artifact_path")?;
                let hearth = ctx.get::<PathBuf>("ct_hearth").ok_or("No ct_hearth")?;
                if containment::escapes_hearth(hearth, path.as_ref()) {
                    Ok(())
                } else {
                    Err(format!("Expected '{}' to escape the hearth", path))
                }
            },
        ),
        check_def(
            "the transition write is refused",
            &[("ct_write_result", "String")],
            |ctx, _params| {
                let result = ctx
                    .get::<String>("ct_write_result")
                    .ok_or("No ct_write_result")?;
                if result.starts_with("err:") {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected the transition write to be refused, got: {}",
                        result
                    ))
                }
            },
        ),
        check_def(
            "the transition write succeeds",
            &[("ct_write_result", "String")],
            |ctx, _params| {
                let result = ctx
                    .get::<String>("ct_write_result")
                    .ok_or("No ct_write_result")?;
                if result.as_ref() as &str == "ok" {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected the transition write to succeed, got: {}",
                        result
                    ))
                }
            },
        ),
    ]
}
