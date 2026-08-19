//! Step definitions for the atomic temp+rename whole-file write helper
//! (`anvil_core_hearth::atomic_write::atomic_write`).
//!
//! These steps exercise the helper directly at the anvil-core library seam
//! (Slice A, no engine subprocess). They prove the crash-safety MECHANISM:
//! a whole-file write goes through a temp sibling and an atomic rename, so a
//! crash mid-write cannot leave a torn live file, and no `.tmp` remains after
//! a successful write.
//!
//! The mid-write observation is deterministic: the helper exposes an
//! injectable observer hook (`atomic_write_observed`) that captures whether a
//! `.tmp` sibling existed at the instant between the temp write and the
//! rename. An in-place `std::fs::write` cannot satisfy this — there is no temp
//! sibling — which is what makes the RED→GREEN transition meaningful.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::ActorIdentity;
use anvil_core_hearth::atomic_write::{atomic_write_observed, AtomicObservation};
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core::ports::actor_write_port::ActorWritePort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

fn collect_with_ext(dir: &std::path::Path, ext: &str, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_with_ext(&path, ext, out);
        } else if path.to_string_lossy().ends_with(ext) {
            out.push(path.display().to_string());
        }
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an empty atomic-write scratch directory",
            &[],
            &[
                ("aw_dir", "PathBuf"),
                ("aw_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = retained_temp_dir("anvil-atomic-write-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create scratch dir: {}", e))?;
                let mut out = Context::new();
                out.set("aw_dir", tmp);
                out.set("aw_dir_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "atomic_write writes {string} with content {string}",
            &[("aw_dir", "PathBuf")],
            &[
                ("aw_dir", "PathBuf"),
                ("aw_dir_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("aw_observation", "AtomicObservation"),
            ],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected filename")?.to_string();
                let content = params.get_string(1).ok_or("Expected content")?.to_string();
                let dir = ctx.get::<PathBuf>("aw_dir").ok_or("No aw_dir")?.clone();
                let path = dir.join(&rel);
                let observation = atomic_write_observed(&path, content.as_bytes())
                    .map_err(|e| format!("atomic_write failed: {}", e))?;
                let mut out = Context::new();
                out.set("aw_dir", dir);
                carry_retained_temp_dir(&ctx, &mut out, "aw_dir_handle");
                out.set("aw_observation", observation);
                Ok(out)
            },
        ),
        check_def(
            "a temp sibling existed during the write",
            &[("aw_observation", "AtomicObservation")],
            |ctx, _params| {
                let obs = ctx
                    .get::<AtomicObservation>("aw_observation")
                    .ok_or("No aw_observation")?;
                if obs.temp_sibling_existed_mid_write {
                    Ok(())
                } else {
                    Err(
                        "Expected a .tmp sibling to exist between write and rename, \
                         but none was observed (in-place write?)"
                            .to_string(),
                    )
                }
            },
        ),
        check_def(
            "the file {string} has content {string}",
            &[("aw_dir", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected filename")?;
                let expected = params.get_string(1).ok_or("Expected content")?;
                let path = ctx.get::<PathBuf>("aw_dir").ok_or("No aw_dir")?.join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if content == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected content '{}', got '{}'",
                        expected, content
                    ))
                }
            },
        ),
        // Drives the real FileSystemActorWriteAdapter (the N4 actor-block
        // whole-file replacement of status.yaml) against the fs_hearth fixture
        // seeded by the snapshot step module.
        step_def(
            "the actor block for {string} is upserted into {string}",
            &[("fs_hearth", "PathBuf")],
            &[
                ("fs_hearth", "PathBuf"),
                ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let actor_name = params.get_string(0).ok_or("Expected actor")?.to_string();
                let artifact_path = params.get_string(1).ok_or("Expected path")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .clone();
                let adapter = FileSystemActorWriteAdapter::new(hearth.clone());
                let identity = ActorIdentity {
                    name: actor_name,
                    actor_type: "agent".to_string(),
                    model: "claude-opus-4-7".to_string(),
                    provider: "anthropic".to_string(),
                    context_window: 1000000,
                    sdk_version: "0.2.111".to_string(),
                    entrypoint: "claude-code".to_string(),
                    registered_at: "2026-04-17T01:00:00Z".to_string(),
                };
                adapter
                    .upsert_actor_configuration(&artifact_path, &identity)
                    .map_err(|e| format!("actor upsert failed: {}", e))?;
                let mut out = Context::new();
                out.set("fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
                Ok(out)
            },
        ),
        // Recursively asserts no `<ext>` files litter the given subtree of the
        // fs_hearth fixture — proving the production write paths route through
        // atomic_write (temp+rename leaves no stray .tmp on success).
        check_def(
            "no {string} files remain under {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let ext = params
                    .get_string(0)
                    .ok_or("Expected extension")?
                    .to_string();
                let rel = params.get_string(1).ok_or("Expected subdir")?;
                let root = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let mut leftovers: Vec<String> = Vec::new();
                collect_with_ext(&root, &ext, &mut leftovers);
                if leftovers.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no '{}' files under {}, found: {:?}",
                        ext,
                        root.display(),
                        leftovers
                    ))
                }
            },
        ),
        check_def(
            "no {string} files remain in the scratch directory",
            &[("aw_dir", "PathBuf")],
            |ctx, params| {
                let ext = params.get_string(0).ok_or("Expected extension")?;
                let dir = ctx.get::<PathBuf>("aw_dir").ok_or("No aw_dir")?;
                let entries = std::fs::read_dir(dir)
                    .map_err(|e| format!("Failed to read scratch dir: {}", e))?;
                let leftovers: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(ext.as_ref() as &str))
                    .collect();
                if leftovers.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no '{}' files, found: {:?}",
                        ext, leftovers
                    ))
                }
            },
        ),
    ]
}
