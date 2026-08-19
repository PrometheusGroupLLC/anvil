//! Step definitions for the `ActorWritePort` three-leg rule scenarios.
//! Two seams: in-memory (`TestActorWriteAdapter`) and filesystem
//! (`FileSystemActorWriteAdapter`).

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::ActorIdentity;
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core_hearth::test_actor_write_adapter::TestActorWriteAdapter;
use anvil_core::ports::actor_write_port::ActorWritePort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

fn identity(name: &str, model: &str, provider: &str) -> ActorIdentity {
    ActorIdentity {
        name: name.to_string(),
        actor_type: "agent".to_string(),
        model: model.to_string(),
        provider: provider.to_string(),
        context_window: 0,
        sdk_version: String::new(),
        entrypoint: String::new(),
        registered_at: String::new(),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an actor write adapter with no existing actors at {string}",
            &[],
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("actor_write_adapter", TestActorWriteAdapter::new());
                Ok(out)
            },
        ),
        step_def(
            "an actor write adapter with actor {string} at {string} with model {string}, provider {string}",
            &[],
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |_ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?.to_string();
                let path = params.get_string(1).ok_or("Expected path")?.to_string();
                let model = params.get_string(2).ok_or("Expected model")?.to_string();
                let provider = params.get_string(3).ok_or("Expected provider")?.to_string();
                let mut id = identity(&name, &model, &provider);
                id.registered_at = "2026-04-17T00:00:00Z".to_string();
                let adapter = TestActorWriteAdapter::new().with_existing_actor(&path, &id);
                let mut out = Context::new();
                out.set("actor_write_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "upsert_actor_configuration is called on {string} with name {string}, model {string}, provider {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |mut ctx, params| {
                let path = params.get_string(0).ok_or("Expected path")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let model = params.get_string(2).ok_or("Expected model")?.to_string();
                let provider = params.get_string(3).ok_or("Expected provider")?.to_string();
                let adapter = ctx
                    .take::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                let mut id = identity(&name, &model, &provider);
                id.registered_at = "2026-04-17T01:00:00Z".to_string();
                adapter
                    .upsert_actor_configuration(&path, &id)
                    .map_err(|e| format!("upsert failed: {}", e))?;
                let mut out = Context::new();
                out.set("actor_write_adapter", adapter);
                Ok(out)
            },
        ),
        check_def(
            "the actor write adapter has a single configurations entry for {string} at {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let path = params.get_string(1).ok_or("Expected path")?;
                let adapter = ctx
                    .get::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                let count = adapter.configurations_count(path, name);
                if count == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected 1 configuration entry for '{}' at '{}', found {}",
                        name, path, count
                    ))
                }
            },
        ),
        check_def(
            "the actor write adapter has {int} configurations entries for {string} at {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let name = params.get_string(1).ok_or("Expected name")?;
                let path = params.get_string(2).ok_or("Expected path")?;
                let adapter = ctx
                    .get::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                let count = adapter.configurations_count(path, name);
                if count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} configuration entries for '{}' at '{}', found {}",
                        expected, name, path, count
                    ))
                }
            },
        ),
        check_def(
            "the actor write adapter's stored configuration for {string} has model {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let model = params.get_string(1).ok_or("Expected model")?;
                let adapter = ctx
                    .get::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                // Find the actor across all stored paths — scenarios use a
                // single path so this is unambiguous in practice.
                let configs_by_path: Vec<_> = ["tracks/test"]
                    .iter()
                    .flat_map(|p| adapter.configurations_for(p, name))
                    .collect();
                if configs_by_path.len() == 1 && configs_by_path[0].model == model {
                    Ok(())
                } else if configs_by_path.is_empty() {
                    Err(format!("No configurations for '{}'", name))
                } else {
                    Err(format!(
                        "Expected exactly one configuration with model '{}' for '{}', found {:?}",
                        model, name, configs_by_path
                    ))
                }
            },
        ),
        check_def(
            "the actor write adapter's first configurations entry for {string} has model {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let model = params.get_string(1).ok_or("Expected model")?;
                let adapter = ctx
                    .get::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                let configs = adapter.configurations_for("tracks/test", name);
                match configs.first() {
                    Some(c) if c.model == model => Ok(()),
                    Some(c) => Err(format!(
                        "First configuration has model '{}', expected '{}'",
                        c.model, model
                    )),
                    None => Err(format!("No configurations stored for '{}'", name)),
                }
            },
        ),
        check_def(
            "the actor write adapter's last configurations entry for {string} has model {string}",
            &[("actor_write_adapter", "TestActorWriteAdapter")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?;
                let model = params.get_string(1).ok_or("Expected model")?;
                let adapter = ctx
                    .get::<TestActorWriteAdapter>("actor_write_adapter")
                    .ok_or("No actor_write_adapter")?;
                let configs = adapter.configurations_for("tracks/test", name);
                match configs.last() {
                    Some(c) if c.model == model => Ok(()),
                    Some(c) => Err(format!(
                        "Last configuration has model '{}', expected '{}'",
                        c.model, model
                    )),
                    None => Err(format!("No configurations stored for '{}'", name)),
                }
            },
        ),
        // ==================== FS-seam steps ====================
        step_def(
            "an actor write fs hearth with status.yaml at {string}:",
            &[],
            &[
                ("actor_write_fs_hearth", "PathBuf"),
                ("actor_write_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact path")?;
                let content = params.doc_string().ok_or("Expected doc string")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-actor-write-fs-")?;
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
                out.set("actor_write_fs_hearth", tmp);
                out.set("actor_write_fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "upsert_actor_configuration on fs is called for {string} with name {string}, model {string}, provider {string}",
            &[("actor_write_fs_hearth", "PathBuf")],
            &[
                ("actor_write_fs_hearth", "PathBuf"),
                ("actor_write_fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
                let name = params.get_string(1).ok_or("Expected name")?.to_string();
                let model = params.get_string(2).ok_or("Expected model")?.to_string();
                let provider = params.get_string(3).ok_or("Expected provider")?.to_string();
                let hearth = ctx
                    .take::<PathBuf>("actor_write_fs_hearth")
                    .ok_or("No actor_write_fs_hearth")?;
                let writer = FileSystemActorWriteAdapter::new(hearth.clone());
                let id = ActorIdentity {
                    name,
                    actor_type: "agent".to_string(),
                    model,
                    provider,
                    context_window: 1000000,
                    sdk_version: String::new(),
                    entrypoint: "claude-code".to_string(),
                    registered_at: "2026-04-17T01:00:00Z".to_string(),
                };
                writer
                    .upsert_actor_configuration(&artifact_path, &id)
                    .map_err(|e| format!("upsert failed: {}", e))?;
                let mut out = Context::new();
                out.set("actor_write_fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "actor_write_fs_hearth_handle");
                Ok(out)
            },
        ),
        check_def(
            "the fs status.yaml at {string} contains {string}",
            &[("actor_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let hearth = ctx
                    .get::<PathBuf>("actor_write_fs_hearth")
                    .ok_or("No actor_write_fs_hearth")?;
                let content = std::fs::read_to_string(hearth.join(artifact_path).join("status.yaml"))
                    .map_err(|e| format!("read failed: {}", e))?;
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
            "the fs status.yaml at {string} still contains {string}",
            &[("actor_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let hearth = ctx
                    .get::<PathBuf>("actor_write_fs_hearth")
                    .ok_or("No actor_write_fs_hearth")?;
                let content = std::fs::read_to_string(hearth.join(artifact_path).join("status.yaml"))
                    .map_err(|e| format!("read failed: {}", e))?;
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
            "the fs status.yaml at {string} contains exactly one occurrence of {string}",
            &[("actor_write_fs_hearth", "PathBuf")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let hearth = ctx
                    .get::<PathBuf>("actor_write_fs_hearth")
                    .ok_or("No actor_write_fs_hearth")?;
                let content = std::fs::read_to_string(hearth.join(artifact_path).join("status.yaml"))
                    .map_err(|e| format!("read failed: {}", e))?;
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
