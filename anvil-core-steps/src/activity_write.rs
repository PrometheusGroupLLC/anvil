//! Step definitions for the `ActivityWritePort` append scenarios at the
//! filesystem seam (`FileSystemActivityWriteAdapter`). The append must
//! preserve the surrounding status.yaml byte-for-byte (F-7), so these steps
//! seed a raw status.yaml doc string, append a begin-marker, and assert on
//! the on-disk text.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::ActivityEntry;
use anvil_core_hearth::fs_activity_write_adapter::FileSystemActivityWriteAdapter;
use anvil_core::ports::activity_write_port::ActivityWritePort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::path::PathBuf;

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an activity write fs hearth with status.yaml at {string}:",
            &[],
            &[
                ("activity_write_fs_hearth", "PathBuf"),
                ("activity_write_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact path")?;
                let content = params.doc_string().ok_or("Expected doc string")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-activity-write-fs-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                let full = tmp.join(artifact_path).join("status.yaml");
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create dir: {}", e))?;
                }
                std::fs::write(&full, content)
                    .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("activity_write_fs_hearth", tmp);
                out.set("activity_write_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "append_activity on fs is called for {string} with actor {string}, state {string}, kind {string}, at {string}",
            &[("activity_write_fs_hearth", "PathBuf")],
            &[
                ("activity_write_fs_hearth", "PathBuf"),
                ("activity_write_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
                let actor = params.get_string(1).ok_or("Expected actor")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let kind = params.get_string(3).ok_or("Expected kind")?.to_string();
                let at = params.get_string(4).ok_or("Expected at")?.to_string();
                let hearth = ctx
                    .take::<PathBuf>("activity_write_fs_hearth")
                    .ok_or("No activity_write_fs_hearth")?;
                let writer = FileSystemActivityWriteAdapter::new(hearth.clone());
                let entry = ActivityEntry { kind, actor, state, at, conversation_id: String::new() };
                writer
                    .append_activity(&artifact_path, &entry)
                    .map_err(|e| format!("append_activity failed: {}", e))?;
                let mut out = Context::new();
                out.set("activity_write_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "activity_write_fs_hearth_handle");
                Ok(out)
            },
        ),
        check_def(
            "the fs activity status.yaml at {string} contains {string}",
            &[("activity_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let content = read_status(&ctx, params)?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "status.yaml does not contain '{}'. Content:\n{}",
                        needle, content
                    ))
                }
            },
        ),
        check_def(
            "the fs activity status.yaml at {string} still contains {string}",
            &[("activity_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let content = read_status(&ctx, params)?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "status.yaml no longer contains '{}'. Content:\n{}",
                        needle, content
                    ))
                }
            },
        ),
        check_def(
            "the fs activity status.yaml at {string} contains exactly one occurrence of {string}",
            &[("activity_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let content = read_status(&ctx, params)?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let count = content.matches(needle).count();
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected exactly one occurrence of '{}', found {}. Content:\n{}",
                        needle, count, content
                    ))
                }
            },
        ),
    ]
}

fn read_status(ctx: &Context, params: &Params) -> Result<String, String> {
    let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
    let hearth = ctx
        .get::<PathBuf>("activity_write_fs_hearth")
        .ok_or("No activity_write_fs_hearth")?;
    std::fs::read_to_string(hearth.join(artifact_path).join("status.yaml"))
        .map_err(|e| format!("read failed: {}", e))
}
