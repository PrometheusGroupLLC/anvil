//! Step module for `playbook_activity_rpc.feature` (engine seam).
//!
//! Seeds a real engine hearth with playbook machines + status.yaml files
//! carrying (or omitting) `contributed_by`, then reuses the shared `engine`
//! module's "the engine is started with that hearth" and route-RPC steps
//! (keyed on `hearth_path` / `engine_process`). This module adds ONLY the
//! seeding step, the PlaybookActivity RPC call, and the response assertions —
//! no duplicate engine-lifecycle steps.

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core::ports::activity_log_port::{ActivityLogRecord, ActivityLogWritePort};

const RESULT_KEY: &str = "wa_rpc_result";

enum WaRpcResult {
    Success(anvil_engine::proto::PlaybookActivityResponse),
    Error { code: String, message: String },
}

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn minimal_route_machine_yaml(kind: &str, description: &str) -> String {
    // A driven machine with the kind as a trigger so the route RPC resolves to
    // it on an exact-match message. Mirrors the minimal loader contract.
    format!(
        r#"kind: {kind}
route:
  triggers:
    - "{kind}"
directory: {kind}s
registry: {kind}s.md
description: "{description}"
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: active
    role_filters: []
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: active
    to_state: completed
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
"#,
        kind = kind,
        description = description
    )
}

fn seed_playbook(
    hearth: &std::path::Path,
    kind: &str,
    owner: &str,
    description: &str,
) -> Result<(), String> {
    let dir = hearth.join("playbooks").join(format!("{}_dir", kind));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create playbook dir: {}", e))?;
    std::fs::write(
        dir.join("machine.yaml"),
        minimal_route_machine_yaml(kind, description),
    )
    .map_err(|e| format!("write machine.yaml: {}", e))?;
    // status.yaml carries contributed_by (or omits it when owner is empty).
    let status = if owner.is_empty() {
        "version: 1\nkind: playbook\nstate: active\nactors: {}\ntransitions: []\n".to_string()
    } else {
        format!(
            "version: 1\nkind: playbook\nstate: active\nactors: {{}}\ncontributed_by: {}\ntransitions: []\n",
            owner
        )
    };
    std::fs::write(dir.join("status.yaml"), status)
        .map_err(|e| format!("write status.yaml: {}", e))?;
    Ok(())
}

fn entries(
    result: &WaRpcResult,
) -> Result<Vec<anvil_engine::proto::PlaybookActivityEntry>, String> {
    match result {
        WaRpcResult::Success(resp) => Ok(resp
            .owners
            .iter()
            .flat_map(|g| g.entries.iter().cloned())
            .collect()),
        WaRpcResult::Error { code, message } => {
            Err(format!("Expected success, got gRPC {}: {}", code, message))
        }
    }
}

fn build_playbook_hearth(
    table: &DataTable,
) -> Result<(RetainedTempDir, std::path::PathBuf), String> {
    let kind_col = column_index(table, "kind")?;
    let owner_col = column_index(table, "owner")?;
    let desc_col = column_index(table, "description")?;

    let (handle, tmp) = retained_temp_dir("anvil-wa-rpc-")?;
    // Engine hearth predicate needs tracks/ + tracks.md.
    std::fs::create_dir_all(tmp.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    std::fs::create_dir_all(tmp.join("playbooks"))
        .map_err(|e| format!("create playbooks: {}", e))?;

    for row in &table.rows {
        seed_playbook(
            &tmp,
            row[kind_col].trim(),
            row[owner_col].trim(),
            row[desc_col].trim(),
        )?;
    }
    Ok((handle, tmp))
}

/// Scaffold a single playbook sub-hearth at `dir` (engine hearth predicate:
/// `tracks/` + `tracks.md`) seeded with one playbook `kind`/`owner`.
fn scaffold_sub_hearth(dir: &std::path::Path, kind: &str, owner: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(dir.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    std::fs::create_dir_all(dir.join("playbooks"))
        .map_err(|e| format!("create playbooks: {}", e))?;
    seed_playbook(dir, kind, owner, "Shared playbook")
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // Build a PARENT directory holding two sub-hearths (A and B). The parent
        // itself is NOT a hearth — it is the directory passed as --permitted-root.
        // Discovery must scan it and find both sub-hearths. `hearth_path` carries
        // sub-hearth A forward as the engine's explicit --hearth default.
        step_def(
            "a permitted parent root with two sub-hearths each seeded with playbook {string} owned by {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("wa_parent_root", "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected playbook kind")?;
                let owner = params.get_string(1).ok_or("Expected owner")?;
                let (handle, parent) = retained_temp_dir("anvil-wa-parent-")?;
                let sub_a = parent.join("alpha-hearth");
                let sub_b = parent.join("beta-hearth");
                scaffold_sub_hearth(&sub_a, kind.trim(), owner.trim())?;
                scaffold_sub_hearth(&sub_b, kind.trim(), owner.trim())?;
                let mut out = Context::new();
                out.set("hearth_path", sub_a);
                // The parent temp dir handle owns BOTH sub-hearths; retain it via
                // the standard hearth_path_handle slot so it survives the run.
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                out.set("wa_parent_root", parent);
                Ok(out)
            },
        ),
        step_def(
            "a playbook activity engine hearth with playbooks:",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = build_playbook_hearth(table)?;
                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // Seed a NON-route activity-log record directly into the playbook hearth.
        // Proves the owner roll-up's call_count folds the universal activity log
        // (begin/snapshot/complete turns), not just the routing-activity sink.
        step_def(
            "an activity log record with command {string} kind {string} is appended to the hearth",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let command = params.get_string(0).ok_or("Expected command")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                FileSystemActivityLogAdapter::new(&hearth)
                    .append_activity_log(&ActivityLogRecord {
                        command,
                        outcome: "ok".to_string(),
                        artifact_kind: kind,
                        from_state: String::new(),
                        to_state: String::new(),
                        actor_hash: None,
                        at: "2026-06-18T12:00:00Z".to_string(),
                        source: String::new(),
                        conversation_hash: None,
                        project_label: None,
                        playbook_run_id: None,
                        call_state: None,
                    })
                    .map_err(|e| format!("append_activity_log failed: {}", e))?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>("hearth_path_handle") {
                    out.set::<RetainedTempDir>("hearth_path_handle", std::sync::Arc::clone(h));
                }
                Ok(out)
            },
        ),
        // Seed a SECONDARY playbook hearth into uts_rpc_hearth_b so the shared
        // "the engine is started with both permitted hearths" step folds BOTH.
        step_def(
            "a playbook activity secondary hearth with playbooks:",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("uts_rpc_hearth_b", "PathBuf"),
                ("uts_rpc_hearth_b_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, tmp) = build_playbook_hearth(table)?;
                let mut out = Context::new();
                // Carry the primary hearth forward (brine retains only provides).
                if let Some(p) = ctx.get::<std::path::PathBuf>("hearth_path") {
                    out.set("hearth_path", p.clone());
                }
                if let Some(h) = ctx.get::<RetainedTempDir>("hearth_path_handle") {
                    out.set::<RetainedTempDir>("hearth_path_handle", std::sync::Arc::clone(h));
                }
                out.set("uts_rpc_hearth_b", tmp);
                out.set::<RetainedTempDir>("uts_rpc_hearth_b_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the PlaybookActivity RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                (RESULT_KEY, "WaRpcResult"),
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let hearth_path = ctx.get::<std::path::PathBuf>("hearth_path").cloned();
                let addr = format!("http://127.0.0.1:{}", port);
                let result =
                    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(
                        addr,
                    )
                    .await
                    {
                        Ok(mut client) => {
                            let request =
                                anvil_test_support::surfaced(anvil_engine::proto::PlaybookActivityRequest {
                                    hearth_path: String::new(),
                                    all_hearths: false,
                                });
                            match client.playbook_activity(request).await {
                                Ok(response) => WaRpcResult::Success(response.into_inner()),
                                Err(status) => WaRpcResult::Error {
                                    code: format!("{:?}", status.code()),
                                    message: status.message().to_string(),
                                },
                            }
                        }
                        Err(e) => WaRpcResult::Error {
                            code: "UNAVAILABLE".to_string(),
                            message: format!("Connection failed: {}", e),
                        },
                    };
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                out.set("engine_process", engine);
                if let Some(hp) = hearth_path {
                    out.set("hearth_path", hp);
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the PlaybookActivity RPC is called across all hearths",
            &[("engine_process", "EngineProcess")],
            &[
                (RESULT_KEY, "WaRpcResult"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let port = engine.port;
                let addr = format!("http://127.0.0.1:{}", port);
                let result =
                    match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(
                        addr,
                    )
                    .await
                    {
                        Ok(mut client) => {
                            let request =
                                anvil_test_support::surfaced(anvil_engine::proto::PlaybookActivityRequest {
                                    hearth_path: String::new(),
                                    all_hearths: true,
                                });
                            match client.playbook_activity(request).await {
                                Ok(response) => WaRpcResult::Success(response.into_inner()),
                                Err(status) => WaRpcResult::Error {
                                    code: format!("{:?}", status.code()),
                                    message: status.message().to_string(),
                                },
                            }
                        }
                        Err(e) => WaRpcResult::Error {
                            code: "UNAVAILABLE".to_string(),
                            message: format!("Connection failed: {}", e),
                        },
                    };
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        check_def(
            "the playbook activity RPC resolved hearth is the all-hearths sentinel",
            &[(RESULT_KEY, "WaRpcResult")],
            |ctx, _params| {
                let result = ctx
                    .get::<WaRpcResult>(RESULT_KEY)
                    .ok_or("No wa_rpc_result")?;
                match result {
                    WaRpcResult::Success(resp) => {
                        if resp.resolved_hearth == "(all hearths)" {
                            Ok(())
                        } else {
                            Err(format!(
                                "expected all-hearths sentinel, got '{}'",
                                resp.resolved_hearth
                            ))
                        }
                    }
                    WaRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the playbook activity RPC included {int} hearths",
            &[(RESULT_KEY, "WaRpcResult")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<WaRpcResult>(RESULT_KEY)
                    .ok_or("No wa_rpc_result")?;
                match result {
                    WaRpcResult::Success(resp) => {
                        if resp.hearths_included.len() == expected {
                            Ok(())
                        } else {
                            Err(format!(
                                "expected {} hearths_included, got {}",
                                expected,
                                resp.hearths_included.len()
                            ))
                        }
                    }
                    WaRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        // Asserts the owner group EXISTS and CONTAINS the expected kinds. The
        // engine registry includes the compiled-in seed lifecycle machines
        // (all anvil-owned), so the "anvil" group legitimately carries extra
        // kinds beyond the test-seeded ones — a containment check is the right
        // contract here, not exact-equality.
        check_def(
            "the playbook activity RPC groups owner {string} with kinds {string}",
            &[(RESULT_KEY, "WaRpcResult")],
            |ctx, params| {
                let owner = params.get_string(0).ok_or("Expected owner")?.to_string();
                let expected: Vec<String> = params
                    .get_string(1)
                    .ok_or("Expected kinds")?
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let result = ctx
                    .get::<WaRpcResult>(RESULT_KEY)
                    .ok_or("No wa_rpc_result")?;
                match result {
                    WaRpcResult::Success(resp) => {
                        let group = resp
                            .owners
                            .iter()
                            .find(|g| g.owner == owner)
                            .ok_or_else(|| format!("No group for owner '{}'", owner))?;
                        let actual: Vec<String> =
                            group.entries.iter().map(|e| e.kind.clone()).collect();
                        let missing: Vec<&String> =
                            expected.iter().filter(|k| !actual.contains(k)).collect();
                        if missing.is_empty() {
                            Ok(())
                        } else {
                            Err(format!(
                                "Owner '{}': missing kinds {:?} (group has {:?})",
                                owner, missing, actual
                            ))
                        }
                    }
                    WaRpcResult::Error { code, message } => {
                        Err(format!("Expected success, got gRPC {}: {}", code, message))
                    }
                }
            },
        ),
        check_def(
            "the playbook activity RPC entry for kind {string} has description {string}",
            &[(RESULT_KEY, "WaRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params
                    .get_string(1)
                    .ok_or("Expected description")?
                    .to_string();
                let result = ctx
                    .get::<WaRpcResult>(RESULT_KEY)
                    .ok_or("No wa_rpc_result")?;
                let all = entries(result)?;
                let entry = all
                    .iter()
                    .find(|e| e.kind == kind)
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                if entry.description == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected description '{}', got '{}'",
                        kind, expected, entry.description
                    ))
                }
            },
        ),
        check_def(
            "the playbook activity RPC entry for kind {string} has call count {int}",
            &[(RESULT_KEY, "WaRpcResult")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let expected = params.get_int(1).ok_or("Expected count")? as u64;
                let result = ctx
                    .get::<WaRpcResult>(RESULT_KEY)
                    .ok_or("No wa_rpc_result")?;
                let all = entries(result)?;
                let entry = all
                    .iter()
                    .find(|e| e.kind == kind)
                    .ok_or_else(|| format!("No entry for kind '{}'", kind))?;
                if entry.call_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Kind '{}': expected call count {}, got {}",
                        kind, expected, entry.call_count
                    ))
                }
            },
        ),
    ]
}
