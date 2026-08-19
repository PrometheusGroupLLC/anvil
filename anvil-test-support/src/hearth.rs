use crate::{carry_retained_temp_dir, retained_temp_dir};
use anvil_core::domain::shared_types::ActivityLog;
use anvil_core::domain::{is_terminal_state_for_kind, ArtifactSummary, ArtifactType, HearthError};
use anvil_core_hearth::fs_hearth_reader::FileSystemHearthReader;
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::test_hearth_reader::TestHearthReader;
use anvil_core::ports::hearth_reader::HearthReaderPort;
use anvil_core::ports::query_port::QueryPort;

// Type alias used in hearth check steps
type HookFilenamesResult = Result<Vec<String>, HearthError>;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;


fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a hearth with the following artifacts:",
            &[],
            &[("hearth_reader", "TestHearthReader")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;

                let id_col = column_index(table, "id")?;
                let type_col = column_index(table, "type")?;
                let state_col = column_index(table, "state")?;
                let summary_col = column_index(table, "summary")?;

                let mut artifacts = Vec::new();
                for row in &table.rows {
                    let id = row[id_col].trim().to_string();
                    let type_str = &row[type_col];
                    let state = row[state_col].trim().to_string();
                    let summary = row[summary_col].trim().to_string();

                    artifacts.push(ArtifactSummary {
                        id,
                        artifact_type: crate::parse_artifact_type(type_str)?,
                        state,
                        summary,
                        execution_route: String::new(),
                    });
                }

                let reader = TestHearthReader::new(artifacts);
                let mut out = Context::new();
                out.set("hearth_reader", reader);
                Ok(out)
            },
        ),
        step_def(
            "listing active artifacts",
            &[("hearth_reader", "TestHearthReader")],
            &[("active_artifacts", "Vec<ArtifactSummary>")],
            |ctx, _params| {
                let reader = ctx
                    .get::<TestHearthReader>("hearth_reader")
                    .ok_or("No hearth_reader")?;

                let all = reader.list_artifacts().map_err(|e| e.to_string())?;
                let active: Vec<ArtifactSummary> = all
                    .into_iter()
                    .filter(|a| !is_terminal_state_for_kind(a.artifact_type.as_str(), &a.state))
                    .collect();

                let mut out = Context::new();
                out.set("active_artifacts", active);
                Ok(out)
            },
        ),
        check_def(
            "{int} artifacts are returned",
            &[("active_artifacts", "Vec<ArtifactSummary>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")?;
                let artifacts = ctx
                    .get::<Vec<ArtifactSummary>>("active_artifacts")
                    .ok_or("No active_artifacts")?;
                let actual = artifacts.len() as i64;
                if actual != expected {
                    return Err(format!(
                        "Expected {} artifacts, got {} ({:?})",
                        expected,
                        actual,
                        artifacts.iter().map(|a| &a.id).collect::<Vec<_>>()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the returned artifacts include {string} with type {string} and state {string}",
            &[("active_artifacts", "Vec<ArtifactSummary>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let type_str = params.get_string(1).ok_or("Expected type")?;
                let state = params.get_string(2).ok_or("Expected state")?;
                let artifacts = ctx
                    .get::<Vec<ArtifactSummary>>("active_artifacts")
                    .ok_or("No active_artifacts")?;

                let expected_type = crate::parse_artifact_type(type_str)?;

                let found = artifacts.iter().find(|a| a.id == id);
                match found {
                    None => Err(format!(
                        "Artifact '{}' not found in active list ({:?})",
                        id,
                        artifacts.iter().map(|a| &a.id).collect::<Vec<_>>()
                    )),
                    Some(a) => {
                        if a.artifact_type != expected_type {
                            return Err(format!(
                                "Artifact '{}' type: expected {:?}, got {:?}",
                                id, expected_type, a.artifact_type
                            ));
                        }
                        if a.state != state {
                            return Err(format!(
                                "Artifact '{}' state: expected {}, got {}",
                                id, state, a.state
                            ));
                        }
                        Ok(())
                    }
                }
            },
        ),
        check_def(
            "the returned artifacts do not include {string}",
            &[("active_artifacts", "Vec<ArtifactSummary>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let artifacts = ctx
                    .get::<Vec<ArtifactSummary>>("active_artifacts")
                    .ok_or("No active_artifacts")?;

                if artifacts.iter().any(|a| a.id == id) {
                    return Err(format!(
                        "Artifact '{}' should not be in active list but was found",
                        id
                    ));
                }
                Ok(())
            },
        ),
        // --- Filesystem hearth reader steps ---
        step_def(
            "a hearth directory with the following structure:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let path_col = column_index(table, "path")?;
                let state_col = column_index(table, "state")?;
                // Optional `kind` column: when present, the explicit kind is
                // written into status.yaml so the artifact resolves to that kind
                // even when its directory prefix is not a legacy KIND_DIR (used to
                // exercise the StateNotReviewable path for unregistered kinds).
                let kind_col = column_index(table, "kind").ok();

                let (handle, tmp) = retained_temp_dir("anvil-test-hearth-")?;

                for row in &table.rows {
                    let artifact_path = row[path_col].trim();
                    let state = row[state_col].trim();

                    let dir = tmp.join(artifact_path);
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| format!("Failed to create dir: {}", e))?;

                    let status_yaml = match kind_col.and_then(|c| row.get(c)) {
                        Some(kind) if !kind.trim().is_empty() => {
                            format!("version: 1\nkind: {}\nstate: {}\n", kind.trim(), state)
                        }
                        _ => format!("version: 1\nstate: {}\n", state),
                    };
                    std::fs::write(dir.join("status.yaml"), status_yaml)
                        .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                }

                // Every anvil hearth carries the structural signature the engine's
                // hearth predicate checks (a tracks/ directory and a tracks.md
                // registry). Guarantee it here so shim-driven RPCs — which send a
                // non-empty hearth_path resolved from .hearth and are therefore
                // predicate-checked — accept this fixture. A later
                // "the hearth tracks.md is seeded with" step overwrites the
                // minimal registry created here.
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("Failed to create tracks/ dir: {}", e))?;
                let registry = tmp.join("tracks.md");
                if !registry.exists() {
                    std::fs::write(&registry, "# Tracks\n")
                        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
                }

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the filesystem hearth reader lists artifacts",
            &[("hearth_path", "PathBuf")],
            &[
                ("fs_result", "FsResult"),
                ("active_artifacts", "Vec<ArtifactSummary>"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let reader = FileSystemHearthReader::new(path.clone());
                let result = reader.list_artifacts();

                let mut out = Context::new();
                // Store artifacts for check steps if result is Ok
                if let Ok(ref artifacts) = result {
                    out.set("active_artifacts", artifacts.clone());
                }
                out.set("fs_result", result);
                // Carry the fixture forward so a following activity-log read step
                // can reach the SAME temp hearth (brine replaces the context per
                // step; the temp-dir handle must survive so the dir is not
                // dropped mid-scenario).
                out.set("hearth_path", path.clone());
                if let Some(handle) = ctx.get::<crate::RetainedTempDir>("hearth_path_handle") {
                    out.set("hearth_path_handle", handle.clone());
                }
                Ok(out)
            },
        ),
        step_def(
            "a hearth path that does not exist",
            &[],
            &[("hearth_path", "PathBuf")],
            |_ctx, _params| {
                let path = PathBuf::from("/tmp/anvil-nonexistent-hearth-path-that-does-not-exist");
                let mut out = Context::new();
                out.set("hearth_path", path);
                Ok(out)
            },
        ),
        step_def(
            "a hearth directory with a malformed status.yaml:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let path_col = column_index(table, "path")?;
                let content_col = column_index(table, "content")?;

                let (handle, tmp) = retained_temp_dir("anvil-test-hearth-")?;

                for row in &table.rows {
                    let artifact_path = row[path_col].trim();
                    // Expand escaped newlines so multi-line status.yaml
                    // content (e.g. a transitions block) can be pasted into a
                    // single table cell, mirroring the snapshot/complete fs
                    // seeding steps.
                    let content = row[content_col].trim().replace("\\n", "\n");

                    let dir = tmp.join(artifact_path);
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| format!("Failed to create dir: {}", e))?;

                    std::fs::write(dir.join("status.yaml"), content)
                        .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                }

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // --- Activity-log degradation steps (per-entry quarantine) ---
        // Read the artifact's `activity:` log through the SAME filesystem query
        // adapter the engine's begin-adoption path uses, so the assertions below
        // exercise the real lenient deserializer end-to-end: which entries
        // survive, which are dropped, and the dropped-count diagnostic.
        step_def(
            "the activity log for {string} is read",
            &[("hearth_path", "PathBuf")],
            &[("activity_log", "ActivityLog")],
            |ctx, params| {
                let artifact_id = params.get_string(0).ok_or("Expected artifact id")?;
                let path = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let adapter = FileSystemQueryAdapter::new(path.clone());
                let log = adapter
                    .read_activity_log(artifact_id)
                    .map_err(|e| format!("read_activity_log failed: {}", e))?;
                let mut out = Context::new();
                out.set("activity_log", log);
                Ok(out)
            },
        ),
        check_def(
            "the activity log retains a begin marker by {string} in state {string}",
            &[("activity_log", "ActivityLog")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?;
                let state = params.get_string(1).ok_or("Expected state")?;
                let log = ctx
                    .get::<ActivityLog>("activity_log")
                    .ok_or("No activity_log")?;
                let found = log
                    .entries
                    .iter()
                    .any(|e| e.kind == "begin" && e.actor == actor && e.state == state);
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected a retained begin marker by '{}' in state '{}', got entries {:?}",
                        actor,
                        state,
                        log.entries
                            .iter()
                            .map(|e| format!("{}/{}@{}", e.actor, e.state, e.at))
                            .collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the activity log has no entry by actor {string}",
            &[("activity_log", "ActivityLog")],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?;
                let log = ctx
                    .get::<ActivityLog>("activity_log")
                    .ok_or("No activity_log")?;
                if log.entries.iter().any(|e| e.actor == actor) {
                    Err(format!(
                        "Expected NO entry by actor '{}', but one survived the parse",
                        actor
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the activity log reports {int} dropped entries",
            &[("activity_log", "ActivityLog")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected dropped count")? as usize;
                let log = ctx
                    .get::<ActivityLog>("activity_log")
                    .ok_or("No activity_log")?;
                if log.dropped == expected {
                    // A non-zero drop must carry a first-error diagnostic; a
                    // zero drop must not — the two are never confused.
                    let has_diag = log.first_error.is_some();
                    if expected > 0 && !has_diag {
                        return Err("Degraded log reported no first_error diagnostic".to_string());
                    }
                    if expected == 0 && has_diag {
                        return Err(format!(
                            "Clean log carried an unexpected diagnostic: {:?}",
                            log.first_error
                        ));
                    }
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} dropped entries, got {} (first_error: {:?})",
                        expected, log.dropped, log.first_error
                    ))
                }
            },
        ),
        check_def(
            "the activity log retains {int} entries",
            &[("activity_log", "ActivityLog")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected retained count")? as usize;
                let log = ctx
                    .get::<ActivityLog>("activity_log")
                    .ok_or("No activity_log")?;
                if log.entries.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} retained entries, got {}",
                        expected,
                        log.entries.len()
                    ))
                }
            },
        ),
        step_def(
            "a hearth directory with artifacts and a registry:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let path_col = column_index(table, "artifact_path")?;
                let state_col = column_index(table, "state")?;

                let (handle, tmp) = retained_temp_dir("anvil-test-hearth-")?;

                for row in &table.rows {
                    let artifact_path = row[path_col].trim();
                    let state = row[state_col].trim();

                    let dir = tmp.join(artifact_path);
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| format!("Failed to create dir: {}", e))?;

                    let status_yaml = format!("version: 1\nstate: {}\n", state);
                    std::fs::write(dir.join("status.yaml"), status_yaml)
                        .map_err(|e| format!("Failed to write status.yaml: {}", e))?;
                }

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the proposals registry contains:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| {
                let content = params.doc_string().ok_or("Expected doc string")?;
                let path = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                std::fs::write(path.join("proposals.md"), content)
                    .map_err(|e| format!("Failed to write proposals.md: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the tracks registry contains:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |mut ctx, params| {
                let content = params.doc_string().ok_or("Expected doc string")?;
                let path = ctx.take::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                std::fs::write(path.join("tracks.md"), content)
                    .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", path);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        check_def(
            "the artifact {string} has summary {string}",
            &[("active_artifacts", "Vec<ArtifactSummary>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?;
                let expected_summary = params.get_string(1).ok_or("Expected summary")?;
                let artifacts = ctx
                    .get::<Vec<ArtifactSummary>>("active_artifacts")
                    .ok_or("No active_artifacts")?;

                let artifact = artifacts
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or_else(|| format!("Artifact '{}' not found", id))?;

                if artifact.summary != expected_summary {
                    return Err(format!(
                        "Artifact '{}' summary: expected '{}', got '{}'",
                        id, expected_summary, artifact.summary
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "a hearth-not-found error is returned with the path",
            &[("fs_result", "FsResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<Result<Vec<ArtifactSummary>, HearthError>>("fs_result")
                    .ok_or("No fs_result")?;
                match result {
                    Err(HearthError::HearthNotFound { .. }) => Ok(()),
                    Err(other) => Err(format!("Expected HearthNotFound, got {:?}", other)),
                    Ok(artifacts) => {
                        Err(format!("Expected error, got {} artifacts", artifacts.len()))
                    }
                }
            },
        ),
        check_def(
            "a malformed-status error is returned for {string}",
            &[("fs_result", "FsResult")],
            |ctx, params| {
                let expected_id = params.get_string(0).ok_or("Expected id")?;
                let result = ctx
                    .get::<Result<Vec<ArtifactSummary>, HearthError>>("fs_result")
                    .ok_or("No fs_result")?;
                match result {
                    Err(HearthError::MalformedStatus { artifact_id, .. }) => {
                        if artifact_id == expected_id {
                            Ok(())
                        } else {
                            Err(format!(
                                "Expected MalformedStatus for '{}', got '{}'",
                                expected_id, artifact_id
                            ))
                        }
                    }
                    Err(other) => Err(format!("Expected MalformedStatus, got {:?}", other)),
                    Ok(artifacts) => {
                        Err(format!("Expected error, got {} artifacts", artifacts.len()))
                    }
                }
            },
        ),
        // ===== Hearth seeding for review / begin scenarios =====
        step_def(
            "the track {string} has spec.md with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Missing track id")?.to_string();
                let content = params.get_string(1).ok_or("Missing content")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let track_dir = hearth.join("tracks").join(&track_id);
                std::fs::create_dir_all(&track_dir)
                    .map_err(|e| format!("Failed to create track dir: {}", e))?;
                std::fs::write(track_dir.join("spec.md"), content.replace("\\n", "\n"))
                    .map_err(|e| format!("Failed to write spec.md: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // Slice C: seed (or overwrite) an arbitrary file inside a track dir,
        // e.g. carry-forward.md. `\n` in content is unescaped to real newlines.
        // Overwrites if the file already exists (supports the fresh-read test).
        step_def(
            "the track {string} has a file {string} with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Missing track id")?.to_string();
                let filename = params.get_string(1).ok_or("Missing filename")?.to_string();
                let content = params.get_string(2).ok_or("Missing content")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let track_dir = hearth.join("tracks").join(&track_id);
                std::fs::create_dir_all(&track_dir)
                    .map_err(|e| format!("Failed to create track dir: {}", e))?;
                std::fs::write(track_dir.join(&filename), content.replace("\\n", "\n"))
                    .map_err(|e| format!("Failed to write {}: {}", filename, e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        // Seed a DAMAGED transition event file under `<track>/transitions/` — an
        // on-disk `.yaml` that is not a parseable TransitionRecord. The lenient
        // status fold skips it; the STRICT adoption read must fail closed on it.
        step_def(
            "the track {string} has a damaged transition event file",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let track_id = params.get_string(0).ok_or("Missing track id")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let transitions_dir = hearth.join("tracks").join(&track_id).join("transitions");
                std::fs::create_dir_all(&transitions_dir)
                    .map_err(|e| format!("Failed to create transitions dir: {}", e))?;
                // Not a valid TransitionRecord — serde_yaml parse fails, so the
                // strict reader surfaces InvalidEvent (fail closed).
                std::fs::write(
                    transitions_dir.join("00000000-damaged.yaml"),
                    "this: [is not, a valid: transition record\n",
                )
                .map_err(|e| format!("Failed to write damaged event: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the hearth tracks.md is seeded with:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let content = params
                    .doc_string()
                    .ok_or("Expected doc string")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                std::fs::write(hearth.join("tracks.md"), content)
                    .map_err(|e| format!("Failed to write tracks.md: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the hearth execution.md is seeded with:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let content = params
                    .doc_string()
                    .ok_or("Expected doc string")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let proj_dir = hearth.join("projections");
                std::fs::create_dir_all(&proj_dir)
                    .map_err(|e| format!("Failed to create projections dir: {}", e))?;
                std::fs::write(proj_dir.join("execution.md"), content)
                    .map_err(|e| format!("Failed to write execution.md: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the hearth forward.md is seeded with:",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let content = params
                    .doc_string()
                    .ok_or("Expected doc string")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let proj_dir = hearth.join("projections");
                std::fs::create_dir_all(&proj_dir)
                    .map_err(|e| format!("Failed to create projections dir: {}", e))?;
                std::fs::write(proj_dir.join("forward.md"), &content)
                    .map_err(|e| format!("Failed to write forward.md: {}", e))?;
                // Persist the pre-call snapshot alongside the projection so
                // the "is unchanged" assertion can read it later without
                // needing to thread a context key through every intervening
                // step. The sentinel file is not interpreted by anvil itself.
                std::fs::write(proj_dir.join(".forward.md.seed"), &content)
                    .map_err(|e| format!("Failed to write forward.md seed: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        check_def(
            "the hearth file {string} contains {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected text")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(rel);
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
        check_def(
            "the hearth file {string} contains {string} under section {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let section = params.get_string(2).ok_or("Expected section header")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let header_pat = format!("{}\n", section);
                let section_pos = content.find(&header_pat).ok_or_else(|| {
                    format!("section '{}' not found in {}", section, path.display())
                })?;
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
        // Creates a regular file (not a directory) at the given relative path under the
        // hearth.  Used in write-failure tests to obstruct `create_dir_all` by placing
        // a file where the adapter expects to create a subdirectory (e.g. `spec_reflection`).
        // Parent directories are created as needed.
        step_def(
            "a file exists at hearth path {string} with content {string}",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?.to_string();
                let content = params.get_string(1).ok_or("Expected content")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let full = hearth.join(&rel);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create parent dirs: {}", e))?;
                }
                std::fs::write(&full, content)
                    .map_err(|e| format!("Failed to write file: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        check_def(
            "the hearth file {string} does not contain {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected text")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if !content.contains(needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected '{}' to NOT be in {}, but it was",
                        needle,
                        path.display()
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
            "the hearth file {string} does not contain {string} under section {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let needle = params.get_string(1).ok_or("Expected needle")?;
                let section = params.get_string(2).ok_or("Expected section header")?;
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let path = hearth.join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
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
        // ===== Playbook hooks discoverability steps (Phase 6, R5.6) =====

        // Given: a hearth directory containing playbook <id> with hook files (data table)
        step_def(
            "a hearth directory containing playbook {string} with hook files:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("playbook_id", "String"),
            ],
            |_ctx, params| {
                let playbook_id = params
                    .get_string(0)
                    .ok_or("Expected playbook_id")?
                    .to_string();
                let table = params.data_table().ok_or("Expected data table")?;
                let filename_col = table
                    .headers
                    .iter()
                    .position(|h| h == "filename")
                    .ok_or("Missing 'filename' column")?;

                let (handle, tmp) = retained_temp_dir("anvil-test-hooks-")?;

                let hooks_dir = tmp.join("playbooks").join(&playbook_id).join("hooks");
                std::fs::create_dir_all(&hooks_dir)
                    .map_err(|e| format!("Failed to create hooks dir: {}", e))?;

                for row in &table.rows {
                    let filename = row[filename_col].trim();
                    std::fs::write(hooks_dir.join(filename), b"# hook content\n")
                        .map_err(|e| format!("Failed to write hook file: {}", e))?;
                }

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                out.set("playbook_id", playbook_id);
                Ok(out)
            },
        ),
        // When: list_playbook_hooks is called for playbook <id>
        step_def(
            "list_playbook_hooks is called for playbook {string}",
            &[("hearth_path", "PathBuf")],
            &[("hook_filenames_result", "HookFilenamesResult")],
            |ctx, params| {
                let playbook_id = params
                    .get_string(0)
                    .ok_or("Expected playbook_id")?
                    .to_string();
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let reader = FileSystemHearthReader::new(hearth_path);
                let result = reader.list_playbook_hooks(&playbook_id);
                let mut out = Context::new();
                out.set("hook_filenames_result", result);
                Ok(out)
            },
        ),
        // Then: the hook filenames include <name>
        check_def(
            "the hook filenames include {string}",
            &[("hook_filenames_result", "HookFilenamesResult")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected filename")?;
                let result = ctx
                    .get::<HookFilenamesResult>("hook_filenames_result")
                    .ok_or("No hook_filenames_result")?;
                let filenames = result.as_ref().map_err(|e| e.to_string())?;
                if filenames.iter().any(|f| f == expected.as_ref() as &str) {
                    Ok(())
                } else {
                    Err(format!(
                        "'{}' not found in hook filenames: {:?}",
                        expected, filenames
                    ))
                }
            },
        ),
        // Then: exactly N hook filenames are returned
        check_def(
            "exactly {int} hook filenames are returned",
            &[("hook_filenames_result", "HookFilenamesResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<HookFilenamesResult>("hook_filenames_result")
                    .ok_or("No hook_filenames_result")?;
                let filenames = result.as_ref().map_err(|e| e.to_string())?;
                if filenames.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} hook filenames, got {}: {:?}",
                        expected,
                        filenames.len(),
                        filenames
                    ))
                }
            },
        ),
        // Given: a hearth directory with a playbook artifact that has a state, hook_file, and hook_content
        // This step seeds the playbook's machine.yaml referencing a hook file whose content we can
        // later assert does NOT appear in the machine yaml.
        step_def(
            "a hearth directory with a playbook artifact {string} that has:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("playbook_id", "String"),
            ],
            |_ctx, params| {
                let playbook_id = params
                    .get_string(0)
                    .ok_or("Expected playbook_id")?
                    .to_string();
                let table = params.data_table().ok_or("Expected data table")?;

                let field_col = table
                    .headers
                    .iter()
                    .position(|h| h == "field")
                    .ok_or("Missing 'field' column")?;
                let value_col = table
                    .headers
                    .iter()
                    .position(|h| h == "value")
                    .ok_or("Missing 'value' column")?;

                let mut state = "active".to_string();
                let mut hook_file = "spec_entry.md".to_string();
                let mut hook_content = "HOOK CONTENT".to_string();

                for row in &table.rows {
                    match row[field_col].trim() {
                        "state" => state = row[value_col].trim().to_string(),
                        "hook_file" => hook_file = row[value_col].trim().to_string(),
                        "hook_content" => hook_content = row[value_col].trim().to_string(),
                        _ => {}
                    }
                }

                let (handle, tmp) = retained_temp_dir("anvil-test-wf-")?;

                let wf_dir = tmp.join("playbooks").join(&playbook_id);
                let hooks_dir = wf_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir)
                    .map_err(|e| format!("Failed to create playbook dir: {}", e))?;

                // Write status.yaml
                let status_yaml = format!("version: 1\nkind: playbook\nstate: {}\n", state);
                std::fs::write(wf_dir.join("status.yaml"), status_yaml)
                    .map_err(|e| format!("Failed to write status.yaml: {}", e))?;

                // Write hook file with the sentinel content
                std::fs::write(hooks_dir.join(&hook_file), &hook_content)
                    .map_err(|e| format!("Failed to write hook file: {}", e))?;

                // Write a machine.yaml that references the hook by filename (but does NOT include
                // the hook's content — machine.yaml is metadata, not hook content delivery)
                let machine_yaml = format!(
                    "kind: my_playbook\n\
                     directory: playbooks\n\
                     registry: workflows.md\n\
                     description: Test playbook.\n\
                     required_fields: []\n\
                     roles: [doer]\n\
                     states:\n\
                     - name: active\n\
                       role_filters: []\n\
                       registry_section: active\n\
                       projection_targets: []\n\
                       is_review_gate: false\n\
                       is_terminal: false\n\
                       hook: {}\n\
                     transitions: []\n",
                    hook_file
                );
                std::fs::write(wf_dir.join("machine.yaml"), machine_yaml)
                    .map_err(|e| format!("Failed to write machine.yaml: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set("hearth_path_handle", handle);
                out.set("playbook_id", playbook_id);
                Ok(out)
            },
        ),
        // When: the filesystem hearth reader reads playbook machine yaml for <id>
        step_def(
            "the filesystem hearth reader reads playbook machine yaml for {string}",
            &[("hearth_path", "PathBuf")],
            &[("machine_yaml_content", "String")],
            |ctx, params| {
                let playbook_id = params
                    .get_string(0)
                    .ok_or("Expected playbook_id")?
                    .to_string();
                let hearth_path = ctx
                    .get::<PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let reader = FileSystemHearthReader::new(hearth_path);
                let yaml_opt = reader
                    .read_playbook_machine_yaml(&playbook_id)
                    .map_err(|e| e.to_string())?;
                let content = yaml_opt.unwrap_or_default();
                let mut out = Context::new();
                out.set("machine_yaml_content", content);
                Ok(out)
            },
        ),
        // Then: the machine yaml does not contain <text>
        check_def(
            "the machine yaml does not contain {string}",
            &[("machine_yaml_content", "String")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected text")?;
                let content = ctx
                    .get::<String>("machine_yaml_content")
                    .ok_or("No machine_yaml_content")?;
                if content.contains(needle.as_ref() as &str) {
                    Err(format!(
                        "machine.yaml unexpectedly contains '{}'. Content:\n{}",
                        needle, content
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
