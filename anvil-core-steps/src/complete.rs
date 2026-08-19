use crate::reflection_write::FaultyReflectionWriteAdapter;
use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::complete::{
    CompleteCommandHandler, CompleteError, CompleteOutcome, CompleteRequest, CompleteResult,
};
use anvil_core::domain::complete_events::CompleteEvent;
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core::domain::shared_types::{ActorIdentity, TransitionContent};
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_reflection_write_adapter::FileSystemReflectionWriteAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core_hearth::in_memory_query_adapter::InMemoryQueryAdapter;
use anvil_core::ports::actor_write_port::ActorWritePort;
use anvil_core::ports::reflection_write_port::ReflectionWritePort;
use anvil_core::ports::snapshot_port::SnapshotPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::Arc;

type CompleteFsResult = Result<CompleteResult, CompleteError>;
type CompleteDomainResult = Result<CompleteOutcome, CompleteError>;

/// Map complete transition to_state to the registry section name and projection label.
/// This mirrors the original execute_doer / execute_reviewer logic which used explicit
/// section names, not registry_section_for() (which maps spec_review → "spec").
fn complete_registry_section(to_state: &str) -> (&'static str, &'static str) {
    match to_state {
        "spec_review" => ("spec_review", "Spec Review"),
        "plan" => ("plan", "Planned"),
        _ => ("", ""),
    }
}

fn dispatch_complete_events(
    outcome: CompleteOutcome,
    _hearth_path: &PathBuf,
    reflection_write: &dyn ReflectionWritePort,
    actor_write: &dyn ActorWritePort,
    snapshot_adapter: &FileSystemSnapshotAdapter,
) -> CompleteFsResult {
    let mut result = outcome.result;
    for event in outcome.events {
        match event {
            CompleteEvent::ActorUpserted {
                artifact_path,
                identity,
            } => {
                actor_write
                    .upsert_actor_configuration(&artifact_path, &identity)
                    .map_err(|e| CompleteError::IoError {
                        message: format!("actor upsert failed: {}", e),
                    })?;
            }
            CompleteEvent::ReflectionWritten {
                artifact_path,
                source_state,
                filename,
                body,
            } => {
                let path = reflection_write
                    .write_reflection_file(&artifact_path, &source_state, &filename, &body)
                    .map_err(CompleteError::from)?;
                result.reflection_path = path;
            }
            CompleteEvent::CarryForwardWritten {
                artifact_path,
                body,
            } => {
                // Slice C: write carry-forward.md as a primary artifact, before
                // the status/registry/projection transition. Mirrors the engine
                // router's CarryForwardWritten arm.
                let path = snapshot_adapter
                    .write_carry_forward(&artifact_path, &body)
                    .map_err(|e| CompleteError::IoError {
                        message: format!("carry-forward.md write failed: {}", e),
                    })?;
                result.carry_forward_path = path;
            }
            CompleteEvent::TransitionRecorded {
                artifact_path,
                to_state,
                at,
                role,
                approver,
                note,
                actor_name,
                satisfaction,
            } => {
                // === Criticality 1: status.yaml append (fail-fast) ===
                let transition = TransitionContent {
                    to: to_state.clone(),
                    at: at.clone(),
                    actor: actor_name.clone(),
                    role: role.clone(),
                    approver: approver.clone(),
                    note: note.clone(),
                    satisfaction: satisfaction.clone(),
                    event_type: None,
                };
                snapshot_adapter
                    .append_transition(&artifact_path, &transition)
                    .map_err(|e| CompleteError::IoError {
                        message: format!("status.yaml append failed: {}", e),
                    })?;

                let (registry_section, projection_section) = complete_registry_section(&to_state);
                let artifact_id = artifact_path.rsplit('/').next().unwrap_or(&artifact_path);

                // === Criticality 2: registry (warn-and-continue) ===
                if !registry_section.is_empty() {
                    match snapshot_adapter.registry_entry_exists("tracks.md", artifact_id) {
                        Ok(true) => {
                            if let Err(e) = snapshot_adapter.move_registry_entry(
                                "tracks.md",
                                artifact_id,
                                registry_section,
                            ) {
                                result
                                    .warnings
                                    .push(format!("registry update failed: {}", e));
                            }
                        }
                        Ok(false) => {
                            match snapshot_adapter.build_registry_entry_text(
                                "track",
                                &artifact_path,
                                registry_section,
                            ) {
                                Ok(entry_text) => {
                                    if let Err(e) = snapshot_adapter.create_registry_entry(
                                        "tracks.md",
                                        artifact_id,
                                        "track",
                                        registry_section,
                                        &entry_text,
                                    ) {
                                        result
                                            .warnings
                                            .push(format!("registry update failed: {}", e));
                                    }
                                }
                                Err(e) => result
                                    .warnings
                                    .push(format!("registry update failed: {}", e)),
                            }
                        }
                        Err(e) => result
                            .warnings
                            .push(format!("registry update failed: {}", e)),
                    }
                }

                // === Criticality 3: projection (warn-and-continue) ===
                if !projection_section.is_empty() {
                    if let Err(e) =
                        snapshot_adapter.move_execution_row(artifact_id, projection_section)
                    {
                        result
                            .warnings
                            .push(format!("projection update failed (execution.md): {}", e));
                    }
                }
            }
        }
    }
    Ok(result)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def("a complete fs hearth with:", &[], &[("fs_hearth", "PathBuf"), ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>")], |_ctx, params| {
            let table = params.data_table().ok_or("Expected data table")?;
            let (handle, tmp) = retained_temp_dir("anvil-complete-fs-")?;
            std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
            let mut pairs: Vec<(String, String)> = Vec::new();
            if table.headers.len() >= 2 { pairs.push((table.headers[0].clone(), table.headers[1].clone())); }
            for row in &table.rows { if row.len() >= 2 { pairs.push((row[0].clone(), row[1].clone())); } }
            for (path, content) in pairs {
                let full = tmp.join(path.trim());
                if let Some(parent) = full.parent() { std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?; }
                let content = content.replace("\\n", "\n");
                std::fs::write(&full, content).map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
            }
            let mut out = Context::new();
            out.set("fs_hearth", tmp);
            out.set("fs_hearth_handle", handle);
            Ok(out)
        }),
        step_def("complete fs is executed with:", &[("fs_hearth", "PathBuf")], &[("fs_hearth", "PathBuf"), ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"), ("complete_result", "CompleteFsResult"), ("reflection_adapter_call_count", "usize")], |ctx, params| {
            let table = params.data_table().ok_or("Expected data table")?;
            let mut request = CompleteRequest { at: "2026-04-19T00:00:00Z".to_string(), ..Default::default() };
            let mut pairs: Vec<(String, String)> = Vec::new();
            if table.headers.len() >= 2 { pairs.push((table.headers[0].trim().to_string(), table.headers[1].trim().to_string())); }
            for row in &table.rows { if row.len() >= 2 { pairs.push((row[0].trim().to_string(), row[1].trim().to_string())); } }
            for (key, value) in pairs {
                match key.as_str() {
                    "artifact_path" => request.artifact_path = value,
                    "actor_name" => request.actor_name = value,
                    "actor_type" => request.actor_type = value,
                    "actor_model" => request.actor_model = value,
                    "actor_provider" => request.actor_provider = value,
                    "actor_context_window" => request.actor_context_window = value.parse().unwrap_or(0),
                    "actor_sdk_version" => request.actor_sdk_version = value,
                    "actor_entrypoint" => request.actor_entrypoint = value,
                    "satisfaction" => request.satisfaction = value,
                    "approver" => request.approver = value,
                    "note" => request.note = value,
                    "at" => request.at = value,
                    "reflection_notes" => request.reflection_notes = value,
                    "findings" => request.findings = value.replace("\\n", "\n"),
                    other => return Err(format!("Unknown complete request key: '{}'", other)),
                }
            }
            if let Some(override_notes) = ctx.get::<String>("pending_reflection_notes") { request.reflection_notes = override_notes.clone(); }
            let hearth_path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.clone();
            let adapter = FileSystemSnapshotAdapter::new(hearth_path.clone());
            let actor_write = FileSystemActorWriteAdapter::new(hearth_path.clone());
            let query_adapter = FileSystemQueryAdapter::new(hearth_path.clone());
            // This fs site drives knowledge artifacts too, so it needs the
            // composite registry (HearthPlaybookRegistry scan + seed fallback).
            let complete_registry = CompositePlaybookRegistry::new(
                HearthPlaybookRegistry::new(hearth_path.clone()),
                SeedPlaybookRegistry,
            );
            let domain_outcome =
                CompleteCommandHandler::execute(&query_adapter, &complete_registry, request);
            let (result, adapter_call_count): (CompleteFsResult, usize) = match domain_outcome {
                Err(e) => (Err(e), 0),
                Ok(outcome) => {
                    if let Some(fault_msg) = ctx.get::<String>("reflection_fault_message") {
                        let faulty = Arc::new(FaultyReflectionWriteAdapter::new());
                        faulty.arm(fault_msg.clone());
                        let r = dispatch_complete_events(outcome, &hearth_path, faulty.as_ref(), &actor_write, &adapter);
                        let count = faulty.call_count();
                        (r, count)
                    } else {
                        let reflection_write = FileSystemReflectionWriteAdapter::new(hearth_path.clone());
                        let r = dispatch_complete_events(outcome, &hearth_path, &reflection_write, &actor_write, &adapter);
                        (r, 0)
                    }
                }
            };
            let mut out = Context::new();
            out.set("fs_hearth", hearth_path);
            carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
            out.set("complete_result", result);
            out.set("reflection_adapter_call_count", adapter_call_count);
            Ok(out)
        }),
        step_def("the complete request has reflection_notes set to:", &[("fs_hearth", "PathBuf")], &[("fs_hearth", "PathBuf"), ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"), ("pending_reflection_notes", "String")], |ctx, params| {
            let notes = params.doc_string().ok_or("Expected doc string with reflection notes")?.to_string();
            let hearth_path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.clone();
            let mut out = Context::new();
            out.set("fs_hearth", hearth_path);
            carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
            out.set("pending_reflection_notes", notes);
            Ok(out)
        }),
        step_def("the reflection write adapter will fail on the next call with {string}", &[("fs_hearth", "PathBuf")], &[("fs_hearth", "PathBuf"), ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"), ("reflection_fault_message", "String")], |ctx, params| {
            let msg = params.get_string(0).ok_or("Expected fault message")?.to_string();
            let hearth_path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.clone();
            let mut out = Context::new();
            out.set("fs_hearth", hearth_path);
            carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
            out.set("reflection_fault_message", msg);
            Ok(out)
        }),
        check_def("the complete result is successful", &[("complete_result", "CompleteFsResult")], |ctx, _params| {
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result new_state is {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected new_state")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if res.new_state == *expected => Ok(()),
                Ok(res) => Err(format!("Expected new_state '{}', got '{}'", expected, res.new_state)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result transition_at is {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected timestamp")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if res.transition_at == *expected => Ok(()),
                Ok(res) => Err(format!("Expected transition_at '{}', got '{}'", expected, res.transition_at)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result artifact_path is {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if res.artifact_path == *expected => Ok(()),
                Ok(res) => Err(format!("Expected artifact_path '{}', got '{}'", expected, res.artifact_path)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result carry_forward_path ends with {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let suffix = params.get_string(0).ok_or("Expected suffix")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) => if res.carry_forward_path.ends_with(suffix) { Ok(()) } else { Err(format!("Expected carry_forward_path to end with '{}', got '{}'", suffix, res.carry_forward_path)) },
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result carry_forward_path is empty", &[("complete_result", "CompleteFsResult")], |ctx, _params| {
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if res.carry_forward_path.is_empty() => Ok(()),
                Ok(res) => Err(format!("Expected empty carry_forward_path, got '{}'", res.carry_forward_path)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result carry_forward_path is non-empty", &[("complete_result", "CompleteFsResult")], |ctx, _params| {
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if !res.carry_forward_path.is_empty() => Ok(()),
                Ok(_) => Err("Expected non-empty carry_forward_path, got empty".to_string()),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete fs artifact {string} status.yaml contains {string}", &[("fs_hearth", "PathBuf"), ("complete_result", "CompleteFsResult")], |ctx, params| {
            let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?;
            let needle = params.get_string(1).ok_or("Expected needle")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(_) => {
                    let hearth = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?;
                    let status_path = hearth.join(artifact_path).join("status.yaml");
                    let content = std::fs::read_to_string(&status_path)
                        .map_err(|e| format!("Failed to read {}: {}", status_path.display(), e))?;
                    if content.contains(needle) {
                        Ok(())
                    } else {
                        Err(format!(
                            "{} does not contain '{}'. Content:\n{}",
                            status_path.display(),
                            needle,
                            content
                        ))
                    }
                }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result is a CompleteError containing {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let needle = params.get_string(0).ok_or("Expected needle")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Err(e) => { let msg = format!("{}", e); if msg.contains(needle) { Ok(()) } else { Err(format!("Expected error containing '{}', got '{}'", needle, msg)) } }
                Ok(_) => Err("Expected error, got success".to_string()),
            }
        }),
        check_def("the complete result reflection_path is empty", &[("complete_result", "CompleteFsResult")], |ctx, _params| {
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if res.reflection_path.is_empty() => Ok(()),
                Ok(res) => Err(format!("Expected empty reflection_path, got '{}'", res.reflection_path)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result reflection_path is non-empty", &[("complete_result", "CompleteFsResult")], |ctx, _params| {
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) if !res.reflection_path.is_empty() => Ok(()),
                Ok(_) => Err("Expected non-empty reflection_path, got empty".to_string()),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result reflection_path ends with {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let suffix = params.get_string(0).ok_or("Expected suffix")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) => if res.reflection_path.ends_with(suffix) { Ok(()) } else { Err(format!("Expected reflection_path to end with '{}', got '{}'", suffix, res.reflection_path)) },
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete result reflection_path contains {string}", &[("complete_result", "CompleteFsResult")], |ctx, params| {
            let needle = params.get_string(0).ok_or("Expected needle")?;
            match ctx.get::<CompleteFsResult>("complete_result").ok_or("No complete_result")? {
                Ok(res) => if res.reflection_path.contains(needle) { Ok(()) } else { Err(format!("Expected reflection_path to contain '{}', got '{}'", needle, res.reflection_path)) },
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the reflection file {string} ends with a newline", &[("fs_hearth", "PathBuf")], |ctx, params| {
            let rel = params.get_string(0).ok_or("Expected path")?;
            let path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.join(rel);
            let content = std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            if content.last() == Some(&b'\n') { Ok(()) } else { Err(format!("Expected {} to end with newline", path.display())) }
        }),
        check_def("the reflection file frontmatter {string} starts with {string}", &[("fs_hearth", "PathBuf")], |ctx, params| {
            let rel = params.get_string(0).ok_or("Expected path")?;
            let expected = params.get_string(1).ok_or("Expected prefix")?;
            let path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.join(rel);
            let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            if content.starts_with(expected) { Ok(()) } else { Err(format!("Expected {} to start with '{}', got prefix: '{}'", path.display(), expected, &content.chars().take(10).collect::<String>())) }
        }),
        check_def("the reflection write adapter call count is {int}", &[("reflection_adapter_call_count", "usize")], |ctx, params| {
            let expected: usize = params.get_int(0).ok_or("Expected int")? as usize;
            let count = ctx.get::<usize>("reflection_adapter_call_count").copied().unwrap_or(0);
            if count == expected { Ok(()) } else { Err(format!("Expected reflection write adapter call count {}, got {}", expected, count)) }
        }),
        check_def("the reflection file frontmatter {string} ends frontmatter before the body", &[("fs_hearth", "PathBuf")], |ctx, params| {
            let rel = params.get_string(0).ok_or("Expected path")?;
            let path = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?.join(rel);
            let content = std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            let after_first = content.strip_prefix("---\n").ok_or_else(|| format!("Expected file to start with '---\\n' in {}", path.display()))?;
            let closing_pos = after_first.find("\n---\n").ok_or_else(|| format!("No closing '---' found in frontmatter of {}", path.display()))?;
            let after_close = &after_first[closing_pos + 5..];
            if after_close.starts_with('\n') || after_close.is_empty() { Ok(()) } else { Err(format!("Expected blank line after frontmatter in {}, got '{}'", path.display(), &after_close.chars().take(20).collect::<String>())) }
        }),
        step_def("a CompleteEvent::TransitionRecorded with artifact_path {string} to_state {string} at {string} actor_name {string} role {string}", &[], &[("complete_event", "CompleteEvent")], |_ctx, params| {
            let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
            let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
            let at = params.get_string(2).ok_or("Expected at")?.to_string();
            let actor_name = params.get_string(3).ok_or("Expected actor_name")?.to_string();
            let role = params.get_string(4).ok_or("Expected role")?.to_string();
            let event = CompleteEvent::TransitionRecorded { artifact_path, to_state, at, role, approver: None, note: None, actor_name, satisfaction: None };
            let mut out = Context::new(); out.set("complete_event", event); Ok(out)
        }),
        step_def("a CompleteEvent::ActorUpserted with artifact_path {string} actor_name {string} actor_type {string}", &[], &[("complete_event", "CompleteEvent")], |_ctx, params| {
            let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
            let actor_name = params.get_string(1).ok_or("Expected actor_name")?.to_string();
            let actor_type = params.get_string(2).ok_or("Expected actor_type")?.to_string();
            let identity = ActorIdentity { name: actor_name, actor_type, model: "test-model".to_string(), provider: "test".to_string(), context_window: 0, sdk_version: String::new(), entrypoint: String::new(), registered_at: "2026-04-20T00:00:00Z".to_string() };
            let event = CompleteEvent::ActorUpserted { artifact_path, identity };
            let mut out = Context::new(); out.set("complete_event", event); Ok(out)
        }),
        step_def("a CompleteEvent::ReflectionWritten with artifact_path {string} source_state {string} filename {string} body {string}", &[], &[("complete_event", "CompleteEvent")], |_ctx, params| {
            let artifact_path = params.get_string(0).ok_or("Expected artifact_path")?.to_string();
            let source_state = params.get_string(1).ok_or("Expected source_state")?.to_string();
            let filename = params.get_string(2).ok_or("Expected filename")?.to_string();
            let body = params.get_string(3).ok_or("Expected body")?.to_string();
            let event = CompleteEvent::ReflectionWritten { artifact_path, source_state, filename, body };
            let mut out = Context::new(); out.set("complete_event", event); Ok(out)
        }),
        check_def("the CompleteEvent is a TransitionRecorded variant", &[("complete_event", "CompleteEvent")], |ctx, _| {
            let event = ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")?;
            if event.is_transition_recorded() { Ok(()) } else { Err(format!("Expected TransitionRecorded, got {:?}", event)) }
        }),
        check_def("the CompleteEvent is an ActorUpserted variant", &[("complete_event", "CompleteEvent")], |ctx, _| {
            let event = ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")?;
            if event.is_actor_upserted() { Ok(()) } else { Err(format!("Expected ActorUpserted, got {:?}", event)) }
        }),
        check_def("the CompleteEvent is a ReflectionWritten variant", &[("complete_event", "CompleteEvent")], |ctx, _| {
            let event = ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")?;
            if event.is_reflection_written() { Ok(()) } else { Err(format!("Expected ReflectionWritten, got {:?}", event)) }
        }),
        check_def("the CompleteEvent TransitionRecorded artifact_path is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::TransitionRecorded { artifact_path, .. } => if artifact_path == expected { Ok(()) } else { Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path)) },
                e => Err(format!("Expected TransitionRecorded, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent TransitionRecorded to_state is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected to_state")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::TransitionRecorded { to_state, .. } => if to_state == expected { Ok(()) } else { Err(format!("Expected to_state '{}', got '{}'", expected, to_state)) },
                e => Err(format!("Expected TransitionRecorded, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent TransitionRecorded role is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected role")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::TransitionRecorded { role, .. } => if role == expected { Ok(()) } else { Err(format!("Expected role '{}', got '{}'", expected, role)) },
                e => Err(format!("Expected TransitionRecorded, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent TransitionRecorded approver is absent", &[("complete_event", "CompleteEvent")], |ctx, _| {
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::TransitionRecorded { approver, .. } => if approver.is_none() { Ok(()) } else { Err(format!("Expected approver to be None, got {:?}", approver)) },
                e => Err(format!("Expected TransitionRecorded, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent TransitionRecorded note is absent", &[("complete_event", "CompleteEvent")], |ctx, _| {
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::TransitionRecorded { note, .. } => if note.is_none() { Ok(()) } else { Err(format!("Expected note to be None, got {:?}", note)) },
                e => Err(format!("Expected TransitionRecorded, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent ActorUpserted artifact_path is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::ActorUpserted { artifact_path, .. } => if artifact_path == expected { Ok(()) } else { Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path)) },
                e => Err(format!("Expected ActorUpserted, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent ActorUpserted identity actor_name is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected actor_name")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::ActorUpserted { identity, .. } => if identity.name == *expected { Ok(()) } else { Err(format!("Expected identity.name '{}', got '{}'", expected, identity.name)) },
                e => Err(format!("Expected ActorUpserted, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent ReflectionWritten artifact_path is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::ReflectionWritten { artifact_path, .. } => if artifact_path == expected { Ok(()) } else { Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path)) },
                e => Err(format!("Expected ReflectionWritten, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent ReflectionWritten source_state is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected source_state")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::ReflectionWritten { source_state, .. } => if source_state == expected { Ok(()) } else { Err(format!("Expected source_state '{}', got '{}'", expected, source_state)) },
                e => Err(format!("Expected ReflectionWritten, got {:?}", e)),
            }
        }),
        check_def("the CompleteEvent ReflectionWritten filename is {string}", &[("complete_event", "CompleteEvent")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected filename")?;
            match ctx.get::<CompleteEvent>("complete_event").ok_or("No complete_event")? {
                CompleteEvent::ReflectionWritten { filename, .. } => if filename == expected { Ok(()) } else { Err(format!("Expected filename '{}', got '{}'", expected, filename)) },
                e => Err(format!("Expected ReflectionWritten, got {:?}", e)),
            }
        }),
        step_def("a CompleteOutcome with new_state {string} and zero events", &[], &[("complete_outcome", "CompleteOutcome")], |_ctx, params| {
            let new_state = params.get_string(0).ok_or("Expected new_state")?.to_string();
            let outcome = CompleteOutcome { result: CompleteResult { new_state, selected_required_role: String::new(), transition_at: "2026-04-20T00:00:00Z".to_string(), artifact_path: "tracks/test".to_string(), warnings: vec![], reflection_path: String::new(), carry_forward_path: String::new(), from_state: String::new(), kind: String::new() }, events: vec![] };
            let mut out = Context::new(); out.set("complete_outcome", outcome); Ok(out)
        }),
        check_def("the CompleteOutcome new_state is {string}", &[("complete_outcome", "CompleteOutcome")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected new_state")?;
            let outcome = ctx.get::<CompleteOutcome>("complete_outcome").ok_or("No complete_outcome")?;
            if outcome.result.new_state == *expected { Ok(()) } else { Err(format!("Expected new_state '{}', got '{}'", expected, outcome.result.new_state)) }
        }),
        check_def("the CompleteOutcome has {int} events", &[("complete_outcome", "CompleteOutcome")], |ctx, params| {
            let expected = params.get_int(0).ok_or("Expected count")? as usize;
            let outcome = ctx.get::<CompleteOutcome>("complete_outcome").ok_or("No complete_outcome")?;
            if outcome.events.len() == expected { Ok(()) } else { Err(format!("Expected {} events, got {}", expected, outcome.events.len())) }
        }),

        // ==================== Domain-seam steps (InMemoryQueryAdapter path) ====================
        // NOTE: Given steps for seeding InMemoryQueryAdapter are registered in query_port.rs
        // under the key "qp_adapter". We read from "qp_adapter" here.

        step_def("complete is called via query adapter with:", &[("qp_adapter", "InMemoryQueryAdapter")], &[("qp_adapter", "InMemoryQueryAdapter"), ("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let table = params.data_table().ok_or("Expected data table")?;
            let mut request = CompleteRequest { at: "2026-04-20T00:00:00Z".to_string(), ..Default::default() };
            let mut pairs: Vec<(String, String)> = Vec::new();
            if table.headers.len() >= 2 { pairs.push((table.headers[0].trim().to_string(), table.headers[1].trim().to_string())); }
            for row in &table.rows { if row.len() >= 2 { pairs.push((row[0].trim().to_string(), row[1].trim().to_string())); } }
            for (key, value) in pairs {
                match key.as_str() {
                    "artifact_path" => request.artifact_path = value,
                    "actor_name" => request.actor_name = value,
                    "actor_type" => request.actor_type = value,
                    "actor_model" => request.actor_model = value,
                    "actor_provider" => request.actor_provider = value,
                    "actor_context_window" => request.actor_context_window = value.parse().unwrap_or(0),
                    "actor_sdk_version" => request.actor_sdk_version = value,
                    "actor_entrypoint" => request.actor_entrypoint = value,
                    "satisfaction" => request.satisfaction = value,
                    "approver" => request.approver = value,
                    "note" => request.note = value,
                    "at" => request.at = value,
                    "reflection_notes" => request.reflection_notes = value,
                    "findings" => request.findings = value.replace("\\n", "\n"),
                    other => return Err(format!("Unknown complete request key: '{other}'")),
                }
            }
            let adapter = ctx.get::<InMemoryQueryAdapter>("qp_adapter").ok_or("No qp_adapter")?.clone();
            // This in-memory site has no hearth dir to scan; the seed registry
            // resolves track/playbook kinds, keeping in-memory features green.
            let result = CompleteCommandHandler::execute(&adapter, &SeedPlaybookRegistry, request);
            let mut out = Context::new();
            out.set("qp_adapter", adapter);
            out.set("complete_domain_result", result);
            Ok(out)
        }),
        check_def("the complete outcome is successful", &[("complete_domain_result", "CompleteDomainResult")], |ctx, _params| {
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(_) => Ok(()), Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome new_state is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected new_state")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) if outcome.result.new_state == *expected => Ok(()),
                Ok(outcome) => Err(format!("Expected new_state '{}', got '{}'", expected, outcome.result.new_state)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome has {int} events", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_int(0).ok_or("Expected count")? as usize;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) if outcome.events.len() == expected => Ok(()),
                Ok(outcome) => Err(format!("Expected {} events, got {}", expected, outcome.events.len())),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome event {int} is ActorUpserted", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let idx = params.get_int(0).ok_or("Expected index")? as usize;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { let event = outcome.events.get(idx).ok_or_else(|| format!("No event at index {}", idx))?; if event.is_actor_upserted() { Ok(()) } else { Err(format!("Expected ActorUpserted at index {}, got {:?}", idx, event)) } }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome event {int} is TransitionRecorded", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let idx = params.get_int(0).ok_or("Expected index")? as usize;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { let event = outcome.events.get(idx).ok_or_else(|| format!("No event at index {}", idx))?; if event.is_transition_recorded() { Ok(()) } else { Err(format!("Expected TransitionRecorded at index {}, got {:?}", idx, event)) } }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome event {int} is ReflectionWritten", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let idx = params.get_int(0).ok_or("Expected index")? as usize;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { let event = outcome.events.get(idx).ok_or_else(|| format!("No event at index {}", idx))?; if event.is_reflection_written() { Ok(()) } else { Err(format!("Expected ReflectionWritten at index {}, got {:?}", idx, event)) } }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded to_state is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected to_state")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { to_state, .. } = event { return if to_state == expected { Ok(()) } else { Err(format!("Expected to_state '{}', got '{}'", expected, to_state)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded role is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected role")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { role, .. } = event { return if role == expected { Ok(()) } else { Err(format!("Expected role '{}', got '{}'", expected, role)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded actor_name is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected actor_name")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { actor_name, .. } = event { return if actor_name == expected { Ok(()) } else { Err(format!("Expected actor_name '{}', got '{}'", expected, actor_name)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded approver is absent", &[("complete_domain_result", "CompleteDomainResult")], |ctx, _params| {
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { approver, .. } = event { return if approver.is_none() { Ok(()) } else { Err(format!("Expected approver to be absent, got {:?}", approver)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded approver is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected approver")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { approver, .. } = event { return match approver { Some(a) if a.as_str() == expected => Ok(()), Some(a) => Err(format!("Expected approver '{}', got '{}'", expected, a)), None => Err(format!("Expected approver '{}', got absent", expected)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome TransitionRecorded note is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected note")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::TransitionRecorded { note, .. } = event { return match note { Some(n) if n.as_str() == expected => Ok(()), Some(n) => Err(format!("Expected note '{}', got '{}'", expected, n)), None => Err(format!("Expected note '{}', got absent", expected)) }; } } Err("No TransitionRecorded event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome ActorUpserted actor_name is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected actor_name")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::ActorUpserted { identity, .. } = event { return if identity.name == *expected { Ok(()) } else { Err(format!("Expected actor_name '{}', got '{}'", expected, identity.name)) }; } } Err("No ActorUpserted event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome ActorUpserted artifact_path is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::ActorUpserted { artifact_path, .. } = event { return if artifact_path == expected { Ok(()) } else { Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path)) }; } } Err("No ActorUpserted event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome ReflectionWritten source_state is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected source_state")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::ReflectionWritten { source_state, .. } = event { return if source_state == expected { Ok(()) } else { Err(format!("Expected source_state '{}', got '{}'", expected, source_state)) }; } } Err("No ReflectionWritten event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome ReflectionWritten artifact_path is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected artifact_path")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::ReflectionWritten { artifact_path, .. } = event { return if artifact_path == expected { Ok(()) } else { Err(format!("Expected artifact_path '{}', got '{}'", expected, artifact_path)) }; } } Err("No ReflectionWritten event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome ReflectionWritten filename is {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_string(0).ok_or("Expected filename")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => { for event in &outcome.events { if let CompleteEvent::ReflectionWritten { filename, .. } = event { return if filename == expected { Ok(()) } else { Err(format!("Expected filename '{}', got '{}'", expected, filename)) }; } } Err("No ReflectionWritten event found".to_string()) }
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome is a CompleteError containing {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let needle = params.get_string(0).ok_or("Expected needle")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Err(e) => { let msg = format!("{}", e); if msg.contains(needle) { Ok(()) } else { Err(format!("Expected error containing '{}', got '{}'", needle, msg)) } }
                Ok(_) => Err("Expected error, got success".to_string()),
            }
        }),
        check_def("the complete outcome has {int} warnings", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let expected = params.get_int(0).ok_or("Expected count")? as usize;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) if outcome.result.warnings.len() == expected => Ok(()),
                Ok(outcome) => Err(format!("Expected {} warnings, got {}: {:?}", expected, outcome.result.warnings.len(), outcome.result.warnings)),
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        check_def("the complete outcome warnings contain {string}", &[("complete_domain_result", "CompleteDomainResult")], |ctx, params| {
            let needle = params.get_string(0).ok_or("Expected needle")?;
            match ctx.get::<CompleteDomainResult>("complete_domain_result").ok_or("No complete_domain_result")? {
                Ok(outcome) => if outcome.result.warnings.iter().any(|w| w.contains(needle)) { Ok(()) } else { Err(format!("Expected a warning containing '{}', got {:?}", needle, outcome.result.warnings)) },
                Err(e) => Err(format!("Expected success, got error: {}", e)),
            }
        }),
        // ===== BP3/BP4/BP6: seed a complete fs hearth with the knowledge
        // machine.yaml + a knowledge artifact in a given state, so
        // `complete fs is executed with:` drives it through the real composite
        // registry + fs adapters. =====
        step_def(
            "a complete fs hearth with a knowledge artifact {string} in state {string}",
            &[],
            &[("fs_hearth", "PathBuf"), ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>")],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let (handle, tmp) = retained_temp_dir("anvil-complete-knowledge-")?;
                std::fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;
                // Structural signature.
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
                // knowledge registry pre-seeded with all the sections the
                // machine declares, so registry moves between sections succeed.
                std::fs::write(tmp.join("knowledge.md"), KNOWLEDGE_REGISTRY_SEED)
                    .map_err(|e| format!("Failed to write knowledge.md: {}", e))?;
                // The knowledge artifact in the requested state, with a prior
                // transition + a registered actor so the snapshot read resolves.
                let art_dir = tmp.join("knowledge").join(&id);
                std::fs::create_dir_all(&art_dir)
                    .map_err(|e| format!("Failed to create artifact dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: knowledge_lifecycle\nstate: {state}\nactors:\n  Seed-000000:\n    type: agent\n    configurations:\n      - at: \"2026-06-01T00:00:00Z\"\n        model: claude-opus-4-8\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: \"\"\n          entrypoint: claude-code\ntransitions:\n  - to: {state}\n    at: 2026-06-01T00:00:00Z\n    actor: Seed-000000\n    role: doer\n",
                    state = state
                );
                std::fs::write(art_dir.join("status.yaml"), status)
                    .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                // The machine on disk (resolved only via HearthPlaybookRegistry).
                let wf_dir = tmp.join("playbooks").join("20260529T0409_knowledge_lifecycle");
                std::fs::create_dir_all(&wf_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;
                std::fs::write(
                    wf_dir.join("machine.yaml"),
                    anvil_test_support::query_port::knowledge_lifecycle_machine_yaml(),
                )
                .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;
                let mut out = Context::new();
                out.set("fs_hearth", tmp);
                out.set("fs_hearth_handle", handle);
                Ok(out)
            },
        ),
    ]
}

/// A knowledge.md registry pre-seeded with every section the
/// knowledge_lifecycle machine declares, so registry moves succeed.
const KNOWLEDGE_REGISTRY_SEED: &str = "# Knowledge\n\n## Ingesting\n\n## Awaiting Ingest Review\n\n## Organizing\n\n## Compiling\n\n## Awaiting Compile Review\n\n## Validating\n\n## Awaiting Validation Review\n\n## Published\n\n## Rejected\n\n## Archived\n";
