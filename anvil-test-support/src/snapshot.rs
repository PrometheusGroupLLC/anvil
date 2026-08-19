use crate::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::snapshot::{
    SnapshotCommandHandler, SnapshotError, SnapshotRequest, SnapshotResult,
};
use anvil_core_hearth::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core_hearth::test_actor_write_adapter::TestActorWriteAdapter;
use anvil_core_hearth::test_snapshot_adapter::TestSnapshotAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

type SnapshotOutcome = Result<SnapshotResult, SnapshotError>;
type SnapshotReadOutcome = Result<String, SnapshotError>;

fn fresh_adapter() -> TestSnapshotAdapter {
    TestSnapshotAdapter::new()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ==================== Setup ====================
        step_def(
            "a snapshot adapter",
            &[],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("snapshot_adapter", fresh_adapter());
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has artifact {string} of kind {string} in state {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let path = params.get_string(0).ok_or("Expected path")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let state = params.get_string(2).ok_or("Expected state")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_artifact(&path, &kind, &state);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has existing registry entry for {string} in {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let file = params.get_string(1).ok_or("Expected file")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_existing_registry_entry(&file, &id);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has state {string} declaring projection_targets for {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected state")?.to_string();
                let path = params.get_string(1).ok_or("Expected path")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_state_declaring_projection(&path, &to_state);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has fixed generated name {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let name = params.get_string(0).ok_or("Expected name")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_fixed_generated_name(&name);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has fixed registry entry text for {string}: {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let path = params.get_string(0).ok_or("Expected path")?.to_string();
                let text = params.get_string(1).ok_or("Expected text")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_fixed_registry_entry_text(&path, &text);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter has seeded actor {string} for {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let path = params.get_string(1).ok_or("Expected path")?.to_string();
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_status(&path, &[actor.as_str()]);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter will fail status append",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, _params| {
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_fail_status_append();
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter will fail registry move",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, _params| {
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_fail_registry_move();
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        step_def(
            "the snapshot adapter will fail projection update",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, _params| {
                let taken = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let adapter = taken.with_fail_projection_update();
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                Ok(out)
            },
        ),
        // ==================== Execution ====================
        step_def(
            "snapshot is executed with:",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            &[
                ("snapshot_adapter", "TestSnapshotAdapter"),
                ("snapshot_result", "SnapshotOutcome"),
            ],
            |mut ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut request = SnapshotRequest {
                    at: "2026-04-17T00:00:00Z".to_string(),
                    ..Default::default()
                };
                // Data-table pairs: in Gherkin a vertical table's first
                // line becomes `headers` and subsequent lines become
                // `rows`. Our step uses the table as a list of
                // (key, value) pairs — include the header line as the
                // first pair.
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    let key = key.as_str();
                    match key {
                        "artifact_path" => request.artifact_path = value,
                        "to_state" => request.to_state = value,
                        "actor_name" => request.actor_name = value,
                        "actor_role" => request.actor_role = value,
                        "approver" => request.approver = value,
                        "note" => request.note = value,
                        "actor_type" => request.actor_type = value,
                        "actor_model" => request.actor_model = value,
                        "actor_provider" => request.actor_provider = value,
                        "actor_context_window" => {
                            request.actor_context_window = value.parse().unwrap_or(0)
                        }
                        "actor_sdk_version" => request.actor_sdk_version = value,
                        "actor_entrypoint" => request.actor_entrypoint = value,
                        "projection_only" => request.projection_only = value == "true",
                        "event_type" => request.event_type = value,
                        "allow_reserved_event_type" => {
                            request.allow_reserved_event_type = value == "true"
                        }
                        "at" => request.at = value,
                        other => {
                            return Err(format!("Unknown snapshot request key: '{}'", other));
                        }
                    }
                }
                let adapter = ctx
                    .take::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let actor_write = TestActorWriteAdapter::new();
                let result: SnapshotOutcome =
                    SnapshotCommandHandler::execute(&adapter, &actor_write, request);
                let mut out = Context::new();
                out.set("snapshot_adapter", adapter);
                out.set("snapshot_result", result);
                Ok(out)
            },
        ),
        // ==================== Result assertions ====================
        check_def(
            "the snapshot result is successful",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, _params| {
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) if res.success => Ok(()),
                    Ok(res) => Err(format!("Expected success, got success=false: {:?}", res)),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result actor_name is {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor_name")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) if res.actor_name == *expected => Ok(()),
                    Ok(res) => Err(format!(
                        "Expected actor_name '{}', got '{}'",
                        expected, res.actor_name
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result projections_updated contains {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected file")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) => {
                        if res.projections_updated.iter().any(|s| s == expected) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected projections_updated to contain '{}', got {:?}",
                                expected, res.projections_updated
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result projections_updated is empty",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, _params| {
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) if res.projections_updated.is_empty() => Ok(()),
                    Ok(res) => Err(format!(
                        "Expected empty projections_updated, got {:?}",
                        res.projections_updated
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result status_updated is {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected bool")?;
                let want = expected == "true";
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) if res.status_updated == want => Ok(()),
                    Ok(res) => Err(format!(
                        "Expected status_updated={}, got {}",
                        want, res.status_updated
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result registry_updated is {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected bool")?;
                let want = expected == "true";
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) if res.registry_updated == want => Ok(()),
                    Ok(res) => Err(format!(
                        "Expected registry_updated={}, got {}",
                        want, res.registry_updated
                    )),
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result has {int} warnings",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) => {
                        if res.warnings.len() as i64 == expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected {} warnings, got {}: {:?}",
                                expected,
                                res.warnings.len(),
                                res.warnings
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result warnings contain {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) => {
                        if res.warnings.iter().any(|w| w.contains(needle)) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected a warning containing '{}', got {:?}",
                                needle, res.warnings
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result has {int} begin-adoption warnings",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) => {
                        let count = res
                            .warnings
                            .iter()
                            .filter(|w| w.starts_with("begin_adoption:"))
                            .count() as i64;
                        if count == expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected {} begin-adoption warnings, got {}: {:?}",
                                expected, count, res.warnings
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the snapshot result is an ActorNameRequired error",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, _params| {
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Err(SnapshotError::ActorNameRequired) => Ok(()),
                    other => Err(format!("Expected ActorNameRequired, got {:?}", other)),
                }
            },
        ),
        check_def(
            "the snapshot result is an ActorParamsRequired error for {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected field")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Err(SnapshotError::ActorParamsRequired { field }) if field == expected => {
                        Ok(())
                    }
                    other => Err(format!(
                        "Expected ActorParamsRequired for '{}', got {:?}",
                        expected, other
                    )),
                }
            },
        ),
        check_def(
            "the snapshot result is an InvalidArgument error containing {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |mut ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Err(SnapshotError::InvalidArgument { reason }) => {
                        if reason.contains(needle) {
                            Ok(())
                        } else {
                            Err(format!("Expected '{}' in reason, got '{}'", needle, reason))
                        }
                    }
                    Err(e) => Err(format!("Expected InvalidArgument, got {:?}", e)),
                    Ok(_) => Err("Expected InvalidArgument, got success".to_string()),
                }
            },
        ),
        check_def(
            "the snapshot result is an IoError",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, _params| {
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Err(SnapshotError::IoError { .. }) => Ok(()),
                    Err(e) => Err(format!("Expected IoError, got {:?}", e)),
                    Ok(_) => Err("Expected IoError, got success".to_string()),
                }
            },
        ),
        // ==================== Adapter recorder assertions ====================
        check_def(
            "the snapshot adapter moved registry entry {string} in {string} to section {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let file = params.get_string(1).ok_or("Expected file")?;
                let section = params.get_string(2).ok_or("Expected section")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let moves = adapter.moved_registry_entries.lock().unwrap();
                let hit = moves
                    .iter()
                    .any(|(f, i, s)| f == file && i == id && s == section);
                if hit {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected move registry entry '{}' in '{}' to '{}'; recorded: {:?}",
                        id, file, section, *moves
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter created registry entry in {string} under section {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let file = params.get_string(0).ok_or("Expected file")?;
                let section = params.get_string(1).ok_or("Expected section")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let creates = adapter.created_registry_entries.lock().unwrap();
                let hit = creates.iter().any(|(f, s, _)| f == file && s == section);
                if hit {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected create registry entry in '{}' under '{}'; recorded: {:?}",
                        file, section, *creates
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter created registry entry with text containing {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let creates = adapter.created_registry_entries.lock().unwrap();
                let hit = creates.iter().any(|(_, _, t)| t.contains(needle));
                if hit {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected created entry text containing '{}'; recorded: {:?}",
                        needle, *creates
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded no registry moves",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, _params| {
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let moves = adapter.moved_registry_entries.lock().unwrap();
                if moves.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no registry moves, got {:?}", *moves))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded no registry creates",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, _params| {
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let creates = adapter.created_registry_entries.lock().unwrap();
                if creates.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no registry creates, got {:?}", *creates))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded no transitions",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, _params| {
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let transitions = adapter.appended_transitions.lock().unwrap();
                if transitions.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no transitions, got {:?}", *transitions))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded no seeded actors",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, _params| {
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let actors = adapter.seeded_actors.lock().unwrap();
                if actors.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no seeded actors, got {:?}", *actors))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded an execution row move to {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let section = params.get_string(0).ok_or("Expected section")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let moves = adapter.moved_execution_rows.lock().unwrap();
                if moves.iter().any(|(_, s)| s == section) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected execution row move to '{}'; recorded: {:?}",
                        section, *moves
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded an intent row move to {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let section = params.get_string(0).ok_or("Expected section")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let moves = adapter.moved_intent_rows.lock().unwrap();
                if moves.iter().any(|(_, _, s)| s == section) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected intent row move to '{}'; recorded: {:?}",
                        section, *moves
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded an authoring projection for {string} phase {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact path")?;
                let phase = params.get_string(1).ok_or("Expected phase")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let writes = adapter.wrote_authoring_projections.lock().unwrap();
                if writes
                    .iter()
                    .any(|(p, label, _)| p == artifact_path && label == phase)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected authoring projection for '{}' phase '{}'; recorded: {:?}",
                        artifact_path, phase, *writes
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded an artifact projection for {string} phase {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |ctx, params| {
                let artifact_path = params.get_string(0).ok_or("Expected artifact path")?;
                let phase = params.get_string(1).ok_or("Expected phase")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let writes = adapter.wrote_artifact_projections.lock().unwrap();
                if writes
                    .iter()
                    .any(|(p, label, _)| p == artifact_path && label == phase)
                {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected artifact projection for '{}' phase '{}'; recorded: {:?}",
                        artifact_path, phase, *writes
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded {int} sparks rebuilds",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let actual = adapter.rebuilt_sparks_projection.lock().unwrap().len() as i64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} sparks rebuilds, got {}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter recorded {int} decisions rebuilds",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let actual = adapter.rebuilt_decisions_projection.lock().unwrap().len() as i64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} decisions rebuilds, got {}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the snapshot adapter transition note is {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected note")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let transitions = adapter.appended_transitions.lock().unwrap();
                if let Some((_, t)) = transitions.last() {
                    match &t.note {
                        Some(n) if n == expected => Ok(()),
                        _ => Err(format!(
                            "Expected transition note '{}', got {:?}",
                            expected, t.note
                        )),
                    }
                } else {
                    Err("No transitions recorded".to_string())
                }
            },
        ),
        check_def(
            "the snapshot adapter transition actor is {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let transitions = adapter.appended_transitions.lock().unwrap();
                if let Some((_, t)) = transitions.last() {
                    if t.actor == *expected {
                        Ok(())
                    } else {
                        Err(format!("Expected actor '{}', got '{}'", expected, t.actor))
                    }
                } else {
                    Err("No transitions recorded".to_string())
                }
            },
        ),
        check_def(
            "the snapshot adapter seeded actor name is {string}",
            &[("snapshot_adapter", "TestSnapshotAdapter")],
            |mut ctx, params| {
                let expected = params.get_string(0).ok_or("Expected name")?;
                let adapter = ctx
                    .get::<TestSnapshotAdapter>("snapshot_adapter")
                    .ok_or("No snapshot_adapter")?;
                let actors = adapter.seeded_actors.lock().unwrap();
                if actors.iter().any(|(_, a)| a.name == *expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected seeded actor name '{}', got {:?}",
                        expected,
                        actors
                            .iter()
                            .map(|(_, a)| a.name.clone())
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        // ==================== FS-seam steps ====================
        step_def(
            "a snapshot fs hearth with:",
            &[],
            &[
                ("fs_hearth", "PathBuf"),
                ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = retained_temp_dir("anvil-snapshot-fs-")?;
                std::fs::create_dir_all(&tmp)
                    .map_err(|e| format!("Failed to create hearth: {}", e))?;
                // Expect two columns: path, content. Treat both header
                // and rows as entries.
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((table.headers[0].clone(), table.headers[1].clone()));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].clone(), row[1].clone()));
                    }
                }
                for (path, content) in pairs {
                    let full = tmp.join(path.trim());
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("Failed to create dir: {}", e))?;
                    }
                    // Replace escaped newlines with real ones for
                    // multi-line content pasted into a single cell.
                    let content = content.replace("\\n", "\n");
                    std::fs::write(&full, content)
                        .map_err(|e| format!("Failed to write {}: {}", full.display(), e))?;
                }
                let mut out = Context::new();
                out.set("fs_hearth", tmp);
                out.set("fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "snapshot fs is executed with:",
            &[("fs_hearth", "PathBuf")],
            &[
                ("fs_hearth", "PathBuf"),
                ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("snapshot_result", "SnapshotOutcome"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut request = SnapshotRequest {
                    at: "2026-04-17T00:00:00Z".to_string(),
                    ..Default::default()
                };
                let mut pairs: Vec<(String, String)> = Vec::new();
                if table.headers.len() >= 2 {
                    pairs.push((
                        table.headers[0].trim().to_string(),
                        table.headers[1].trim().to_string(),
                    ));
                }
                for row in &table.rows {
                    if row.len() >= 2 {
                        pairs.push((row[0].trim().to_string(), row[1].trim().to_string()));
                    }
                }
                for (key, value) in pairs {
                    match key.as_str() {
                        "artifact_path" => request.artifact_path = value,
                        "to_state" => request.to_state = value,
                        "actor_name" => request.actor_name = value,
                        "actor_role" => request.actor_role = value,
                        "approver" => request.approver = value,
                        "note" => request.note = value,
                        "actor_type" => request.actor_type = value,
                        "actor_model" => request.actor_model = value,
                        "actor_provider" => request.actor_provider = value,
                        "actor_context_window" => {
                            request.actor_context_window = value.parse().unwrap_or(0)
                        }
                        "actor_sdk_version" => request.actor_sdk_version = value,
                        "actor_entrypoint" => request.actor_entrypoint = value,
                        "projection_only" => request.projection_only = value == "true",
                        "event_type" => request.event_type = value,
                        "allow_reserved_event_type" => {
                            request.allow_reserved_event_type = value == "true"
                        }
                        "at" => request.at = value,
                        other => return Err(format!("Unknown key: '{}'", other)),
                    }
                }
                let hearth_path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth_path.clone());
                let actor_write = FileSystemActorWriteAdapter::new(hearth_path.clone());
                let result: SnapshotOutcome =
                    SnapshotCommandHandler::execute(&adapter, &actor_write, request);
                let mut out = Context::new();
                out.set("fs_hearth", hearth_path);
                carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
                out.set("snapshot_result", result);
                Ok(out)
            },
        ),
        check_def(
            "the file {string} contains {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected text")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' in {}, content:\n{}",
                        needle,
                        path.display(),
                        content
                    ))
                }
            },
        ),
        // ===== Transition event-store assertions (event-store upcast) =====
        // After the upcast a transition is a per-file event under
        // `<artifact>/transitions/`, NOT a status.yaml array/`state:` mutation.
        // These steps assert the new surface (resolved state through the seam,
        // and the event-file content) so scenarios exercising a NEW transition
        // stay honest to the new contract.
        check_def(
            "the resolved state of {string} is {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?;
                let expected = params.get_string(1).ok_or("Expected state")?;
                let hearth = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth);
                let state = adapter
                    .read_artifact_state(artifact.as_ref() as &str)
                    .map_err(|e| format!("read_artifact_state({}): {}", artifact, e))?;
                if state == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected resolved state '{}' for '{}', got '{}'",
                        expected, artifact, state
                    ))
                }
            },
        ),
        // Asserts SOME transition event file under `<artifact>/transitions/`
        // contains the given substring (e.g. "to: plan", "actor: X").
        check_def(
            "a transition event for {string} contains {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let artifact = params.get_string(0).ok_or("Expected artifact")?;
                let needle = params.get_string(1).ok_or("Expected text")?;
                let hearth = ctx.get::<PathBuf>("fs_hearth").ok_or("No fs_hearth")?;
                let dir = hearth.join(artifact.as_ref() as &str).join("transitions");
                let entries = std::fs::read_dir(&dir)
                    .map_err(|e| format!("No transitions dir for '{}': {}", artifact, e))?;
                for entry in entries.flatten() {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if content.contains(needle.as_ref() as &str) {
                            return Ok(());
                        }
                    }
                }
                Err(format!(
                    "No transition event file for '{}' contains '{}'",
                    artifact, needle
                ))
            },
        ),
        check_def(
            "the file {string} still starts with {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let prefix = params.get_string(1).ok_or("Expected prefix")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if content.starts_with(prefix) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected file to start with '{}', got: {}",
                        prefix,
                        &content.chars().take(80).collect::<String>()
                    ))
                }
            },
        ),
        check_def(
            "the file {string} does not contain {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected text")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                if !content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!("Expected '{}' NOT in {}", needle, path.display()))
                }
            },
        ),
        check_def(
            "the file {string} contains {string} under section {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let section = params.get_string(2).ok_or("Expected section header")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let header_pat = format!("{}\n", section);
                let section_pos = match content.find(&header_pat) {
                    Some(p) => p,
                    None => {
                        return Err(format!(
                            "section '{}' not found in {}",
                            section,
                            path.display()
                        ))
                    }
                };
                let after = &content[section_pos + header_pat.len()..];
                let next = after.find("\n## ").unwrap_or(after.len());
                let section_content = &after[..next];
                if section_content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "'{}' not found under '{}' in {}. Section content:\n{}",
                        needle,
                        section,
                        path.display(),
                        section_content
                    ))
                }
            },
        ),
        // Section-aware negative assertion: confirms a needle does NOT
        // appear within the content of a named section (e.g. "## spec").
        // The section is identified by its header line; content ends at
        // the next "## " header or end-of-file. If the section header is
        // absent the assertion trivially passes (nothing to find).
        check_def(
            "the file {string} does not contain {string} under section {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let section = params.get_string(2).ok_or("Expected section header")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let header_pat = format!("{}\n", section);
                let section_pos = match content.find(&header_pat) {
                    Some(p) => p,
                    None => return Ok(()), // section absent → trivially passes
                };
                let after = &content[section_pos + header_pat.len()..];
                let next = after.find("\n## ").unwrap_or(after.len());
                let section_content = &after[..next];
                if section_content.contains(needle) {
                    Err(format!(
                        "'{}' unexpectedly found under '{}' in {}",
                        needle,
                        section,
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the file {string} matches regex {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let pattern = params.get_string(1).ok_or("Expected pattern")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let re = simple_regex(pattern);
                if re(&content) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} to match '{}'; content:\n{}",
                        path.display(),
                        pattern,
                        content
                    ))
                }
            },
        ),
        check_def(
            "the snapshot result actor_name matches {string}",
            &[("snapshot_result", "SnapshotOutcome")],
            |ctx, params| {
                let pattern = params.get_string(0).ok_or("Expected pattern")?;
                let r = ctx
                    .get::<SnapshotOutcome>("snapshot_result")
                    .ok_or("No snapshot_result")?;
                match r {
                    Ok(res) => {
                        if simple_regex(pattern)(&res.actor_name) {
                            Ok(())
                        } else {
                            Err(format!(
                                "actor_name '{}' does not match '{}'",
                                res.actor_name, pattern
                            ))
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
        check_def(
            "the directory {string} does not exist",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                if path.exists() {
                    Err(format!(
                        "Expected directory '{}' to NOT exist",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // Asserts that a directory is either absent or contains no files (empty).
        // Used by Phase 2 write-failed scenarios to verify atomic-write invariant:
        // no partial files remain after a failed reflection write.
        check_def(
            "the directory {string} is empty or absent",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                if !path.exists() {
                    return Ok(()); // absent — satisfies the invariant
                }
                // Exists — check that it is empty.
                let entries = std::fs::read_dir(&path)
                    .map_err(|e| format!("Failed to read directory '{}': {}", path.display(), e))?;
                let count = entries.count();
                if count == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected directory '{}' to be empty or absent, but it has {} entry/entries",
                        path.display(),
                        count
                    ))
                }
            },
        ),
        // Asserts that a directory exists and contains exactly N files (not subdirectories).
        // Used by Phase 3 three-pillar enumerability and multi-actor accumulation scenarios
        // to verify each source-state subdirectory holds the expected number of reflection files.
        check_def(
            "the directory {string} contains exactly {int} file",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                // index 1: {int} is the second capture, after {string} at index 0
                let expected_count: i64 = params.get_int(1).ok_or("Expected count")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                if !path.exists() {
                    return Err(format!(
                        "Expected directory '{}' to exist with {} file(s), but it does not exist",
                        path.display(),
                        expected_count
                    ));
                }
                let entries = std::fs::read_dir(&path)
                    .map_err(|e| format!("Failed to read directory '{}': {}", path.display(), e))?;
                let file_count = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                    .count() as i64;
                if file_count == expected_count {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected directory '{}' to contain exactly {} file(s), but found {}",
                        path.display(),
                        expected_count,
                        file_count
                    ))
                }
            },
        ),
        // ===== N2: snapshot read-site invocation step =====
        // The existing snapshot fs feature exercises only append_transition
        // (the writer). Nothing currently invokes SnapshotPort::read_artifact_state
        // (fs_snapshot_adapter.rs read site). This step drives it directly via the
        // REAL FileSystemSnapshotAdapter so the fallback / parse path is exercised.
        step_def(
            "snapshot fs read_artifact_state is called for {string}",
            &[("fs_hearth", "PathBuf")],
            &[
                ("fs_hearth", "PathBuf"),
                ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("snapshot_read_result", "SnapshotReadOutcome"),
            ],
            |ctx, params| {
                let artifact_path = params
                    .get_string(0)
                    .ok_or("Expected artifact_path")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .clone();
                let adapter = FileSystemSnapshotAdapter::new(hearth.clone());
                let result = adapter.read_artifact_state(&artifact_path);
                let mut out = Context::new();
                out.set("fs_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "fs_hearth_handle");
                out.set("snapshot_read_result", result);
                Ok(out)
            },
        ),
        // ===== F2: top-level-line check =====
        // The existing "the file ... contains ..." check is a plain substring
        // match that a nested transitions sub-line (e.g. "  - to: decided")
        // would satisfy. This check asserts a column-0 (unindented) line equals
        // the expected string (matches ^state: decided$), proving a TOP-LEVEL
        // state line exists, not merely a substring.
        check_def(
            "the file {string} has a top-level line {string}",
            &[("fs_hearth", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let expected = params.get_string(1).ok_or("Expected line")?;
                let path = ctx
                    .get::<PathBuf>("fs_hearth")
                    .ok_or("No fs_hearth")?
                    .join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let found = content
                    .lines()
                    .any(|l| !l.starts_with(char::is_whitespace) && l == expected.as_ref() as &str);
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected a top-level line '{}' in {}, content:\n{}",
                        expected,
                        path.display(),
                        content
                    ))
                }
            },
        ),
        check_def(
            "the snapshot read state is {string}",
            &[("snapshot_read_result", "SnapshotReadOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let result = ctx
                    .get::<SnapshotReadOutcome>("snapshot_read_result")
                    .ok_or("No snapshot_read_result")?;
                match result {
                    Ok(s) if s == expected.as_ref() as &str => Ok(()),
                    Ok(s) => Err(format!(
                        "Expected snapshot read state '{}', got '{}'",
                        expected, s
                    )),
                    Err(e) => Err(format!("Expected success '{}', got error: {}", expected, e)),
                }
            },
        ),
    ]
}

/// Minimal regex matcher supporting `^`, `$`, `\d`, `{m,n}`, literal
/// characters, and character classes like `[A-Z]`. Good enough for
/// validating the actor-name pattern and similar constrained checks.
fn simple_regex(pattern: &str) -> Box<dyn Fn(&str) -> bool> {
    let pat = pattern.to_string();
    Box::new(move |s: &str| regex_lite_match(&pat, s))
}

pub struct SimpleRegex;

impl SimpleRegex {
    pub fn matches(pattern: &str, text: &str) -> bool {
        regex_lite_match(pattern, text)
    }
}

fn regex_lite_match(pattern: &str, text: &str) -> bool {
    // Parse pattern into tokens.
    let tokens = parse_regex(pattern);
    if pattern.starts_with('^') {
        regex_try_from(&tokens, text, 0, pattern.ends_with('$'))
    } else {
        for start in 0..=text.len() {
            if regex_try_from(&tokens, text, start, pattern.ends_with('$')) {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
enum RegexTok {
    Literal(char),
    Digit,
    Class(Vec<(char, char)>),
    Quant(Box<RegexTok>, usize, usize),
}

fn parse_regex(pattern: &str) -> Vec<RegexTok> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '^' || c == '$' {
            i += 1;
            continue;
        }
        let tok = if c == '\\' && i + 1 < chars.len() && chars[i + 1] == 'd' {
            i += 2;
            RegexTok::Digit
        } else if c == '[' {
            let end = chars[i..].iter().position(|&x| x == ']').unwrap_or(0);
            let inner: Vec<char> = chars[i + 1..i + end].to_vec();
            let mut ranges: Vec<(char, char)> = Vec::new();
            let mut j = 0;
            while j < inner.len() {
                if j + 2 < inner.len() && inner[j + 1] == '-' {
                    ranges.push((inner[j], inner[j + 2]));
                    j += 3;
                } else {
                    ranges.push((inner[j], inner[j]));
                    j += 1;
                }
            }
            i += end + 1;
            RegexTok::Class(ranges)
        } else {
            i += 1;
            RegexTok::Literal(c)
        };
        // Handle quantifier following the token.
        if i < chars.len() && chars[i] == '{' {
            let end = chars[i..].iter().position(|&x| x == '}').unwrap_or(0);
            let inner: String = chars[i + 1..i + end].iter().collect();
            let parts: Vec<&str> = inner.split(',').collect();
            let min: usize = parts[0].parse().unwrap_or(0);
            let max: usize = if parts.len() > 1 {
                parts[1].parse().unwrap_or(min)
            } else {
                min
            };
            i += end + 1;
            out.push(RegexTok::Quant(Box::new(tok), min, max));
        } else if i < chars.len() && chars[i] == '+' {
            i += 1;
            out.push(RegexTok::Quant(Box::new(tok), 1, 1000));
        } else if i < chars.len() && chars[i] == '*' {
            i += 1;
            out.push(RegexTok::Quant(Box::new(tok), 0, 1000));
        } else {
            out.push(tok);
        }
    }
    out
}

fn regex_try_from(tokens: &[RegexTok], text: &str, start: usize, anchor_end: bool) -> bool {
    regex_match_at(tokens, 0, text, start, anchor_end)
}

fn regex_match_at(
    tokens: &[RegexTok],
    tok_i: usize,
    text: &str,
    pos: usize,
    anchor_end: bool,
) -> bool {
    if tok_i >= tokens.len() {
        return if anchor_end { pos == text.len() } else { true };
    }
    let tok = &tokens[tok_i];
    match tok {
        RegexTok::Literal(c) => {
            if pos < text.len() && text.as_bytes()[pos] as char == *c {
                regex_match_at(tokens, tok_i + 1, text, pos + 1, anchor_end)
            } else {
                false
            }
        }
        RegexTok::Digit => {
            if pos < text.len() && (text.as_bytes()[pos] as char).is_ascii_digit() {
                regex_match_at(tokens, tok_i + 1, text, pos + 1, anchor_end)
            } else {
                false
            }
        }
        RegexTok::Class(ranges) => {
            if pos < text.len() {
                let c = text.as_bytes()[pos] as char;
                if ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi) {
                    regex_match_at(tokens, tok_i + 1, text, pos + 1, anchor_end)
                } else {
                    false
                }
            } else {
                false
            }
        }
        RegexTok::Quant(inner, min, max) => {
            let mut matched = 0;
            let mut cur = pos;
            // Greedy match up to max.
            while matched < *max && regex_single_match(inner, text, cur) {
                cur += 1;
                matched += 1;
            }
            // Try backtracking from max down to min.
            while matched >= *min {
                if regex_match_at(tokens, tok_i + 1, text, cur, anchor_end) {
                    return true;
                }
                if matched == 0 {
                    break;
                }
                matched -= 1;
                cur -= 1;
            }
            false
        }
    }
}

fn regex_single_match(tok: &RegexTok, text: &str, pos: usize) -> bool {
    if pos >= text.len() {
        return false;
    }
    let c = text.as_bytes()[pos] as char;
    match tok {
        RegexTok::Literal(lit) => c == *lit,
        RegexTok::Digit => c.is_ascii_digit(),
        RegexTok::Class(ranges) => ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi),
        RegexTok::Quant(_, _, _) => false,
    }
}
