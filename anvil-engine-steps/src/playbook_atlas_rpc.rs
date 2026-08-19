//! Step module for `playbook_atlas_rpc.feature` (engine seam).
//!
//! Seeds a HERMETIC fixture hearth carrying four playbooks that exercise the
//! honest-measurement contract:
//!   - `playbook_generation` — valid, properly-gated, with a 4-dim / 8-anchor
//!     success rubric + a status.yaml declaring `owner_kit: forge-kit`;
//!   - `council_experiment_design` — valid but PLANTED with a rubber-stamp
//!     `design_review` gate (`is_review_gate:false` with a single null-satisfaction
//!     exit) and NO status.yaml (so `owner_kit == ""`). Independent of the mutable
//!     live `track` allowlist;
//!   - `free_probe` — a `register: free` kind with no rubric (calibration "none");
//!   - `broken_playbook` — a loader-invalid machine (undeclared role) surfaced as
//!     a degenerate `loads:false` entry with its load error.
//!
//! Reuses the shared `engine` module's "the engine is started with that hearth"
//! step. Adds the seeding step, the gRPC PlaybookAtlas / PlaybookAtlasDetail
//! calls + assertions, and the /ws `playbook_atlas` / `playbook_atlas_detail`
//! requests + assertions (same connect-send-recv shape as `ws_bridge`).

use anvil_test_support::engine::EngineProcess;
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const PA_GRPC_KEY: &str = "pa_grpc_result";
const PAD_GRPC_KEY: &str = "pad_grpc_result";
const PA_WS_KEY: &str = "pa_ws_response";
const PAD_WS_KEY: &str = "pad_ws_response";

enum GrpcAtlas {
    Success(anvil_engine::proto::PlaybookAtlasResponse),
    Error { code: String, message: String },
}

enum GrpcDetail {
    Success(anvil_engine::proto::PlaybookAtlasDetailResponse),
    Error { code: String, message: String },
}

// ---- fixture machine.yaml builders --------------------------------------

/// A valid, properly-gated playbook carrying a 4-dimension / 8-anchor rubric.
fn playbook_generation_yaml() -> &'static str {
    r#"kind: playbook_generation
route:
  triggers:
    - "generate a playbook"
directory: workflow_generations
registry: workflow_generations.md
description: "Generate a playbook"
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: gather
    role_filters: []
    registry_section: gather
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: "Gather domain material"
        expected_output: "notes.md"
  - name: model
    role_filters: []
    registry_section: model
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: "Draft the machine"
        expected_output: "machine.yaml"
  - name: model_review
    role_filters: []
    registry_section: model_review
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    measurement_by_role:
      reviewer:
        intent: "Review the machine"
        expected_output: "review.md"
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: gather
    to_state: model
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: model
    to_state: model_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: model_review
    to_state: completed
    required_role: reviewer
    required_satisfaction:
      - satisfied
    requires_approver: false
  - from_state: model_review
    to_state: model
    required_role: reviewer
    required_satisfaction:
      - needs_revision
    requires_approver: false
success_rubric:
  dimensions:
    - dimension: correctness
      weight: 3
      evidence_class: artifact_of_consequence
    - dimension: clarity_structure
      weight: 2
      evidence_class: verifiable_citation
    - dimension: pattern_alignment
      weight: 2
      evidence_class: self_description
    - dimension: faithfulness_to_real_process
      weight: 1
      evidence_class: artifact_of_consequence
  grader: playbook_generation_judge
  lagging_signals:
    - adoption
    - churn
  anchors:
    - instance: anchor-1
      band: good
    - instance: anchor-2
      band: good
    - instance: anchor-3
      band: mediocre
    - instance: anchor-4
      band: mediocre
    - instance: anchor-5
      band: good
    - instance: anchor-6
      band: mediocre
    - instance: anchor-7
      band: good
    - instance: anchor-8
      band: mediocre
"#
}

/// A valid machine PLANTED with a rubber-stamp `design_review` gate
/// (`is_review_gate:false`, single null-satisfaction exit) — loads cleanly, but
/// `playbook_integrity` flags `design_review`.
fn council_experiment_design_yaml() -> &'static str {
    r#"kind: council_experiment_design
route:
  triggers:
    - "design a council experiment"
directory: council_experiment_designs
registry: council_experiment_designs.md
description: "Design a council experiment"
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: design
    role_filters: []
    registry_section: design
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: design_review
    role_filters: []
    registry_section: design_review
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
  - from_state: design
    to_state: design_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: design_review
    to_state: completed
    required_role: reviewer
    required_satisfaction: ~
    requires_approver: false
"#
}

/// A `register: free` kind with no success rubric (calibration "none").
fn free_probe_yaml() -> &'static str {
    r#"kind: free_probe
directory: free_probes
registry: free_probes.md
description: "A free-register probe"
required_fields: []
register: free
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
    measurement_by_role:
      doer:
        intent: "Do the free thing"
        expected_output: "output.md"
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
"#
}

/// A loader-INVALID machine: a transition references an undeclared role, so the
/// loader rejects it and it appears in `invalid_artifacts()`.
fn broken_playbook_yaml() -> &'static str {
    r#"kind: broken
directory: brokens
registry: brokens.md
description: "A deliberately loader-invalid machine"
required_fields: []
roles:
  - doer
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
    required_role: ghost_role
    required_satisfaction: ~
    requires_approver: false
"#
}

fn write_playbook(
    playbooks: &std::path::Path,
    dir: &str,
    machine_yaml: &str,
    status_yaml: Option<&str>,
) -> Result<(), String> {
    let wf_dir = playbooks.join(dir);
    std::fs::create_dir_all(&wf_dir).map_err(|e| format!("create {}: {}", dir, e))?;
    std::fs::write(wf_dir.join("machine.yaml"), machine_yaml)
        .map_err(|e| format!("write {}/machine.yaml: {}", dir, e))?;
    if let Some(status) = status_yaml {
        std::fs::write(wf_dir.join("status.yaml"), status)
            .map_err(|e| format!("write {}/status.yaml: {}", dir, e))?;
    }
    Ok(())
}

// ---- accessors ----------------------------------------------------------

fn grpc_atlas(ctx: &Context) -> Result<&anvil_engine::proto::PlaybookAtlasResponse, String> {
    match ctx.get::<GrpcAtlas>(PA_GRPC_KEY).ok_or("No atlas gRPC result")? {
        GrpcAtlas::Success(resp) => Ok(resp),
        GrpcAtlas::Error { code, message } => {
            Err(format!("Expected atlas success, got gRPC {}: {}", code, message))
        }
    }
}

fn grpc_detail(ctx: &Context) -> Result<&anvil_engine::proto::PlaybookAtlasDetailResponse, String> {
    match ctx.get::<GrpcDetail>(PAD_GRPC_KEY).ok_or("No detail gRPC result")? {
        GrpcDetail::Success(resp) => Ok(resp),
        GrpcDetail::Error { code, message } => {
            Err(format!("Expected detail success, got gRPC {}: {}", code, message))
        }
    }
}

fn atlas_entry<'a>(
    resp: &'a anvil_engine::proto::PlaybookAtlasResponse,
    key: &str,
) -> Option<&'a anvil_engine::proto::AtlasEntry> {
    resp.entries.iter().find(|e| e.kind == key)
}

fn ws_result(ctx: &Context, key: &str) -> Result<Value, String> {
    let response = ctx.get::<Value>(key).ok_or("No /ws response in context")?;
    if let Some(error) = response.get("error") {
        return Err(format!("Expected JSON-RPC result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("JSON-RPC response has no result: {}", response))
}

fn ws_entry<'a>(result: &'a Value, key: &str) -> Option<&'a Value> {
    result
        .get("entries")
        .and_then(Value::as_array)?
        .iter()
        .find(|e| e.get("kind").and_then(Value::as_str) == Some(key))
}

async fn ws_roundtrip(port: u16, request: &Value) -> Result<Value, String> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("ws connect to {} failed: {}", url, e))?;
    socket
        .send(Message::Text(request.to_string()))
        .await
        .map_err(|e| format!("ws send failed: {}", e))?;
    while let Some(frame) = socket.next().await {
        match frame.map_err(|e| format!("ws recv failed: {}", e))? {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| format!("ws reply not JSON: {} (raw: {})", e, text));
            }
            Message::Binary(bytes) => {
                return serde_json::from_slice(&bytes)
                    .map_err(|e| format!("ws binary reply not JSON: {}", e));
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

/// Write a temper `scorecard.json` fixture at
/// `<hearth>/__temper_home__/.temper/workflow-measurements/<instance>/scorecard.json`
/// — the SAME path the engine reads via `resolve_temper_home` +
/// `scorecard_reader::read_scorecard_instances` ("the engine is started with
/// that hearth" sets `ANVIL_TEMPER_HOME` to `<hearth>/__temper_home__`). The
/// data table's rows become the scorecard's `steps` array, in row order.
fn write_scorecard_fixture(
    hearth: &std::path::Path,
    instance: &str,
    track_id: &str,
    mean_quality: f64,
    table: &DataTable,
) -> Result<(), String> {
    let col = |name: &str| -> Result<usize, String> {
        table
            .headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("Missing '{}' column in data table", name))
    };
    let from_col = col("from_state")?;
    let to_col = col("to_state")?;
    let role_col = col("role")?;
    let actor_col = col("actor")?;
    let quality_col = col("quality_score")?;
    let model_col = col("model")?;

    let steps: Vec<Value> = table
        .rows
        .iter()
        .map(|row| -> Result<Value, String> {
            let quality_score: f64 = row[quality_col]
                .trim()
                .parse()
                .map_err(|e| format!("bad quality_score: {}", e))?;
            Ok(json!({
                "from_state": row[from_col].trim(),
                "to_state": row[to_col].trim(),
                "role": row[role_col].trim(),
                "actor": row[actor_col].trim(),
                "quality_score": quality_score,
                "model": row[model_col].trim(),
            }))
        })
        .collect::<Result<Vec<Value>, String>>()?;

    let scorecard = json!({
        "workflow_id": instance,
        "track_id": track_id,
        "step_count": steps.len(),
        "mean_quality": mean_quality,
        "total_cost_usd": 0.0,
        "total_tokens": 0,
        "steps": steps,
    });

    let dir = hearth
        .join("__temper_home__")
        .join(".temper")
        .join("workflow-measurements")
        .join(instance);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir scorecard dir: {}", e))?;
    std::fs::write(
        dir.join("scorecard.json"),
        serde_json::to_string_pretty(&scorecard).map_err(|e| format!("serialize: {}", e))?,
    )
    .map_err(|e| format!("write scorecard.json: {}", e))?;
    Ok(())
}

/// Write a temper `artifact_quality.json` fixture at
/// `<hearth>/__temper_home__/.temper/workflow-measurements/<instance>/artifact_quality.json`
/// — a SIBLING of `scorecard.json` in the same instance directory, written by
/// the SEPARATE real (harsh, 0-10 anchored) artifact-quality grader.
fn write_artifact_quality_fixture(
    hearth: &std::path::Path,
    instance: &str,
    track_id: &str,
    table: &DataTable,
) -> Result<(), String> {
    let col = |name: &str| -> Result<usize, String> {
        table
            .headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("Missing '{}' column in data table", name))
    };
    let to_col = col("to_state")?;
    let role_col = col("role")?;
    let artifact_col = col("artifact")?;
    let score_col = col("artifact_quality_0_10")?;
    let band_col = col("band")?;

    let steps: Vec<Value> = table
        .rows
        .iter()
        .map(|row| -> Result<Value, String> {
            let artifact_quality_0_10: f64 = row[score_col]
                .trim()
                .parse()
                .map_err(|e| format!("bad artifact_quality_0_10: {}", e))?;
            Ok(json!({
                "to_state": row[to_col].trim(),
                "role": row[role_col].trim(),
                "artifact": row[artifact_col].trim(),
                "artifact_quality_0_10": artifact_quality_0_10,
                "band": row[band_col].trim(),
            }))
        })
        .collect::<Result<Vec<Value>, String>>()?;

    let artifact_quality = json!({
        "workflow_id": instance,
        "track_id": track_id,
        "steps": steps,
    });

    let dir = hearth
        .join("__temper_home__")
        .join(".temper")
        .join("workflow-measurements")
        .join(instance);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir artifact_quality dir: {}", e))?;
    std::fs::write(
        dir.join("artifact_quality.json"),
        serde_json::to_string_pretty(&artifact_quality).map_err(|e| format!("serialize: {}", e))?,
    )
    .map_err(|e| format!("write artifact_quality.json: {}", e))?;
    Ok(())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a playbook atlas engine hearth",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| async move {
                let (handle, tmp) = retained_temp_dir("anvil-atlas-")?;
                // Engine hearth predicate needs tracks/ + tracks.md.
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;

                let playbooks = tmp.join("playbooks");
                std::fs::create_dir_all(&playbooks)
                    .map_err(|e| format!("create playbooks: {}", e))?;

                // Valid, gated, rubric-carrying + a status.yaml declaring owner_kit.
                write_playbook(
                    &playbooks,
                    "playbook_generation",
                    playbook_generation_yaml(),
                    Some(
                        "version: 1\nkind: playbook_generation\nowner_kit: forge-kit\nstate: model\nactors: {}\ntransitions: []\n",
                    ),
                )?;
                // Rubber-stamp fixture, NO status.yaml → owner_kit "".
                write_playbook(
                    &playbooks,
                    "council_experiment_design",
                    council_experiment_design_yaml(),
                    None,
                )?;
                // Free-register kind.
                write_playbook(
                    &playbooks,
                    "free_probe",
                    free_probe_yaml(),
                    Some(
                        "version: 1\nkind: free_probe\nowner_kit: labs-kit\nstate: active\nactors: {}\ntransitions: []\n",
                    ),
                )?;
                // Loader-invalid machine — dir name is the artifact_id.
                write_playbook(&playbooks, "broken_playbook", broken_playbook_yaml(), None)?;
                // C-d.1 round 4. A directory under the canonical root with NO
                // machine.yaml: the loader records it as an ExcludedDirectory
                // ("Recorded, not swallowed... The projection states it") and
                // NOTHING read that record — `excluded_directories()` had zero
                // consumers anywhere, and the projection that consumes it has
                // none either. The operator saw a directory on disk and an atlas
                // that did not mention it, which is the "concluded it was never
                // there" failure this surface exists to end.
                std::fs::create_dir_all(playbooks.join("20260909T0000_no_machine_dir"))
                    .map_err(|e| format!("create no-machine dir: {}", e))?;

                // C-d.1 round 5 (L-2). The exclusion de-dupe compares DIRECTORY
                // IDS, and it used to be built from `AtlasEntry::key()` — which
                // is the governed KIND for a loaded entry and the artifact id for
                // an invalid one. Different namespaces. This pair makes that
                // observable: a machine whose directory id and kind DIFFER, plus
                // an empty directory whose basename equals that machine's KIND.
                // Under the id/kind mix the empty directory is silently
                // suppressed from the atlas — the exact "the operator concludes
                // it was never there" failure this wiring exists to end,
                // reintroduced by the code that ends it. Every other fixture in
                // this hearth names its directory after its kind, which is why
                // the defect was invisible here.
                write_playbook(
                    &playbooks,
                    "20261010T0000_atlas_id_differs_from_kind",
                    free_probe_yaml()
                        .replace("kind: free_probe", "kind: atlas_shadow_kind")
                        .replace("directory: free_probes", "directory: atlas_shadow_kinds")
                        .replace("registry: free_probes.md", "registry: atlas_shadow_kinds.md")
                        .as_str(),
                    None,
                )?;
                std::fs::create_dir_all(playbooks.join("atlas_shadow_kind"))
                    .map_err(|e| format!("create kind-named dir: {}", e))?;

                let mut out = Context::new();
                out.set("hearth_path", tmp);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        async_step_def(
            "the PlaybookAtlas RPC is called",
            &[("engine_process", "EngineProcess")],
            &[
                (PA_GRPC_KEY, "GrpcAtlas"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = anvil_test_support::surfaced(anvil_engine::proto::PlaybookAtlasRequest {
                            hearth_path: String::new(),
                        });
                        match client.playbook_atlas(request).await {
                            Ok(response) => GrpcAtlas::Success(response.into_inner()),
                            Err(status) => GrpcAtlas::Error {
                                code: format!("{:?}", status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => GrpcAtlas::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set(PA_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "the PlaybookAtlasDetail RPC is called for kind {string}",
            &[("engine_process", "EngineProcess")],
            &[
                (PAD_GRPC_KEY, "GrpcDetail"),
                ("engine_process", "EngineProcess"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let addr = format!("http://127.0.0.1:{}", engine.port);
                let result = match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
                    Ok(mut client) => {
                        let request = anvil_test_support::surfaced(anvil_engine::proto::PlaybookAtlasDetailRequest {
                            hearth_path: String::new(),
                            kind: kind.clone(),
                        });
                        match client.playbook_atlas_detail(request).await {
                            Ok(response) => GrpcDetail::Success(response.into_inner()),
                            Err(status) => GrpcDetail::Error {
                                code: format!("{:?}", status.code()),
                                message: status.message().to_string(),
                            },
                        }
                    }
                    Err(e) => GrpcDetail::Error {
                        code: "UNAVAILABLE".to_string(),
                        message: format!("Connection failed: {}", e),
                    },
                };
                let mut out = Context::new();
                out.set(PAD_GRPC_KEY, result);
                out.set("engine_process", engine);
                Ok(out)
            },
        ),
        async_step_def(
            "a playbook_atlas JSON-RPC request is sent over /ws with hearth_path {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (PA_WS_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let hearth_path = params.get_string(0).unwrap_or_default().to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "playbook_atlas",
                    "params": { "surface": "test-harness", "hearth_path": hearth_path }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(PA_WS_KEY, response);
                Ok(out)
            },
        ),
        async_step_def(
            "a playbook_atlas_detail JSON-RPC request is sent over /ws for kind {string}",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (PAD_WS_KEY, "Value"),
            ],
            |mut ctx, params| async move {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let request = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "playbook_atlas_detail",
                    "params": { "surface": "test-harness", "hearth_path": "", "kind": kind }
                });
                let response = ws_roundtrip(engine.port, &request).await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<Value>(PAD_WS_KEY, response);
                Ok(out)
            },
        ),
        // ---- gRPC atlas assertions --------------------------------------
        check_def(
            "the atlas has an entry for kind {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let resp = grpc_atlas(&ctx)?;
                atlas_entry(resp, key)
                    .map(|_| ())
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))
            },
        ),
        check_def(
            "the atlas entry {string} reports loads false with an error",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected artifact id")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                if entry.loads {
                    return Err(format!("Entry '{}' reports loads=true, expected false", key));
                }
                if entry.error_message.is_empty() {
                    return Err(format!("Entry '{}' has an empty error_message", key));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas entry {string} reports error code {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected artifact id")?;
                let want = params.get_string(1).ok_or("Expected error code")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key).ok_or_else(|| {
                    format!(
                        "No atlas entry for '{}'. A directory sitting under the canonical root \
                         that the loader resolved NOTHING from is recorded by the registry as an \
                         excluded directory and was surfaced nowhere — so the operator sees a \
                         directory on disk and an atlas that does not mention it.",
                        key
                    )
                })?;
                if entry.error_code != want {
                    return Err(format!(
                        "Entry '{}' reports error code '{}', expected '{}'",
                        key, entry.error_code, want
                    ));
                }
                Ok(())
            },
        ),
        // C-d.1 round 5 (L-2). Two DIFFERENT things can legitimately carry the
        // same atlas key: a loaded machine whose governed KIND is `X`, and an
        // unrelated directory whose BASENAME is `X`. The de-dupe that suppresses
        // double-listing must compare ids to ids; when it compared the excluded
        // directory's id against other entries' KINDS it silently dropped the
        // second row. `atlas_entry(..)` returns the first match and so cannot
        // see the difference — this counts.
        check_def(
            "the atlas lists both a loaded kind and a not-loaded directory named {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected key")?;
                let resp = grpc_atlas(&ctx)?;
                let rows: Vec<_> = resp.entries.iter().filter(|e| e.kind == key).collect();
                let loaded = rows.iter().filter(|e| e.loads).count();
                let excluded = rows
                    .iter()
                    .filter(|e| e.error_code == "playbook_directory_not_loaded")
                    .count();
                if loaded != 1 || excluded != 1 {
                    return Err(format!(
                        "the atlas carries {} row(s) keyed {key:?}: {loaded} loaded and {excluded} \
                         not-loaded, expected exactly one of each. A machine whose governed kind \
                         is {key:?} and an unrelated DIRECTORY basenamed {key:?} are different \
                         facts about this hearth; suppressing the directory because some machine's \
                         KIND matched its ID leaves the operator looking at a directory on disk \
                         and an atlas that denies it exists — which is the failure this wiring was \
                         added to end.",
                        rows.len()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas entry for kind {string} reports loads true",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                if entry.loads {
                    Ok(())
                } else {
                    Err(format!("Entry '{}' reports loads=false, expected true", key))
                }
            },
        ),
        check_def(
            "the atlas entry for kind {string} reports rubber-stamp gate {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let gate = params.get_string(1).ok_or("Expected gate")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                let gates = entry
                    .integrity
                    .as_ref()
                    .map(|i| i.rubber_stamp_gates.clone())
                    .unwrap_or_default();
                if gates.iter().any(|g| g == gate) {
                    Ok(())
                } else {
                    Err(format!(
                        "Entry '{}' rubber_stamp_gates {:?} does not contain '{}'",
                        key, gates, gate
                    ))
                }
            },
        ),
        check_def(
            "the atlas entry for kind {string} reports no rubber-stamp gates",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                let gates = entry
                    .integrity
                    .as_ref()
                    .map(|i| i.rubber_stamp_gates.clone())
                    .unwrap_or_default();
                if gates.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Entry '{}' has rubber_stamp_gates {:?}", key, gates))
                }
            },
        ),
        check_def(
            "the atlas entry for kind {string} has register {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let register = params.get_string(1).ok_or("Expected register")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                if entry.register == register {
                    Ok(())
                } else {
                    Err(format!(
                        "Entry '{}' register '{}' != expected '{}'",
                        key, entry.register, register
                    ))
                }
            },
        ),
        check_def(
            "the atlas entry for kind {string} has owner_kit {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let owner = params.get_string(1).unwrap_or_default();
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                if entry.owner_kit == owner {
                    Ok(())
                } else {
                    Err(format!(
                        "Entry '{}' owner_kit '{}' != expected '{}'",
                        key, entry.owner_kit, owner
                    ))
                }
            },
        ),
        check_def(
            "the atlas entry for kind {string} has calibration {string}",
            &[(PA_GRPC_KEY, "GrpcAtlas")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let calibration = params.get_string(1).ok_or("Expected calibration")?;
                let resp = grpc_atlas(&ctx)?;
                let entry = atlas_entry(resp, key)
                    .ok_or_else(|| format!("No atlas entry for '{}'", key))?;
                if entry.calibration == calibration {
                    Ok(())
                } else {
                    Err(format!(
                        "Entry '{}' calibration '{}' != expected '{}'",
                        key, entry.calibration, calibration
                    ))
                }
            },
        ),
        // ---- gRPC detail assertions -------------------------------------
        check_def(
            "the atlas detail has {int} rubric dimensions",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let resp = grpc_detail(&ctx)?;
                let rubric = resp
                    .success_rubric
                    .as_ref()
                    .ok_or("detail has no success_rubric")?;
                if rubric.dimensions.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "detail has {} rubric dimensions, expected {}",
                        rubric.dimensions.len(),
                        expected
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail rubric anchors_count is {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u32;
                let resp = grpc_detail(&ctx)?;
                let rubric = resp
                    .success_rubric
                    .as_ref()
                    .ok_or("detail has no success_rubric")?;
                if rubric.anchors_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "detail anchors_count {} != expected {}",
                        rubric.anchors_count, expected
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail rubric grader_declared is true",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, _params| {
                let resp = grpc_detail(&ctx)?;
                let rubric = resp
                    .success_rubric
                    .as_ref()
                    .ok_or("detail has no success_rubric")?;
                if rubric.grader_declared {
                    Ok(())
                } else {
                    Err("detail rubric grader_declared is false, expected true".to_string())
                }
            },
        ),
        check_def(
            "the atlas detail has a review-gate state {string}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected state name")?;
                let resp = grpc_detail(&ctx)?;
                let state = resp
                    .states
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or_else(|| format!("detail has no state '{}'", name))?;
                if state.is_review_gate {
                    Ok(())
                } else {
                    Err(format!("state '{}' is_review_gate is false", name))
                }
            },
        ),
        check_def(
            "the atlas detail has an edge from {string} to {string} with required_satisfaction {string}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let from = params.get_string(0).ok_or("Expected from")?;
                let to = params.get_string(1).ok_or("Expected to")?;
                let satisfaction = params.get_string(2).ok_or("Expected satisfaction")?;
                let resp = grpc_detail(&ctx)?;
                let edge = resp
                    .edges
                    .iter()
                    .find(|e| e.from == from && e.to == to)
                    .ok_or_else(|| format!("detail has no edge {} -> {}", from, to))?;
                if edge.required_satisfaction.iter().any(|s| s == satisfaction) {
                    Ok(())
                } else {
                    Err(format!(
                        "edge {} -> {} required_satisfaction {:?} does not contain '{}'",
                        from, to, edge.required_satisfaction, satisfaction
                    ))
                }
            },
        ),
        // ---- /ws atlas assertions ---------------------------------------
        check_def(
            "the /ws playbook_atlas result has an entry for kind {string}",
            &[(PA_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let result = ws_result(&ctx, PA_WS_KEY)?;
                ws_entry(&result, key)
                    .map(|_| ())
                    .ok_or_else(|| format!("No /ws atlas entry for '{}'", key))
            },
        ),
        check_def(
            "the /ws playbook_atlas entry for kind {string} reports rubber-stamp gate {string}",
            &[(PA_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let gate = params.get_string(1).ok_or("Expected gate")?;
                let result = ws_result(&ctx, PA_WS_KEY)?;
                let entry = ws_entry(&result, key)
                    .ok_or_else(|| format!("No /ws atlas entry for '{}'", key))?;
                let has = entry
                    .get("integrity")
                    .and_then(|i| i.get("rubber_stamp_gates"))
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().any(|g| g.as_str() == Some(gate)))
                    .unwrap_or(false);
                if has {
                    Ok(())
                } else {
                    Err(format!(
                        "/ws entry '{}' rubber_stamp_gates does not contain '{}': {}",
                        key, gate, entry
                    ))
                }
            },
        ),
        check_def(
            "the /ws playbook_atlas entry {string} reports loads false with an error",
            &[(PA_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected artifact id")?;
                let result = ws_result(&ctx, PA_WS_KEY)?;
                let entry = ws_entry(&result, key)
                    .ok_or_else(|| format!("No /ws atlas entry for '{}'", key))?;
                let loads = entry.get("loads").and_then(Value::as_bool).unwrap_or(true);
                let error = entry
                    .get("error_message")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if loads {
                    return Err(format!("/ws entry '{}' loads=true, expected false", key));
                }
                if error.is_empty() {
                    return Err(format!("/ws entry '{}' has empty error_message", key));
                }
                Ok(())
            },
        ),
        check_def(
            "the /ws playbook_atlas entry for kind {string} has register {string}",
            &[(PA_WS_KEY, "Value")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected kind")?;
                let register = params.get_string(1).ok_or("Expected register")?;
                let result = ws_result(&ctx, PA_WS_KEY)?;
                let entry = ws_entry(&result, key)
                    .ok_or_else(|| format!("No /ws atlas entry for '{}'", key))?;
                let actual = entry.get("register").and_then(Value::as_str).unwrap_or("");
                if actual == register {
                    Ok(())
                } else {
                    Err(format!(
                        "/ws entry '{}' register '{}' != expected '{}'",
                        key, actual, register
                    ))
                }
            },
        ),
        // ---- /ws detail assertions --------------------------------------
        check_def(
            "the /ws playbook_atlas_detail result has {int} rubric dimensions",
            &[(PAD_WS_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ws_result(&ctx, PAD_WS_KEY)?;
                let dims = result
                    .get("success_rubric")
                    .and_then(|r| r.get("dimensions"))
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .ok_or("/ws detail has no success_rubric.dimensions")?;
                if dims == expected {
                    Ok(())
                } else {
                    Err(format!("/ws detail has {} dimensions, expected {}", dims, expected))
                }
            },
        ),
        check_def(
            "the /ws playbook_atlas_detail result rubric anchors_count is {int}",
            &[(PAD_WS_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u64;
                let result = ws_result(&ctx, PAD_WS_KEY)?;
                let anchors = result
                    .get("success_rubric")
                    .and_then(|r| r.get("anchors_count"))
                    .and_then(Value::as_u64)
                    .ok_or("/ws detail has no success_rubric.anchors_count")?;
                if anchors == expected {
                    Ok(())
                } else {
                    Err(format!("/ws detail anchors_count {} != expected {}", anchors, expected))
                }
            },
        ),
        // ---- temper scorecard fixture + honest-measurement assertions ----
        check_def(
            "a temper scorecard for instance {string} kind {string} mean_quality {string}:",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let instance = params.get_string(0).ok_or("Expected instance")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let mean_quality: f64 = params
                    .get_string(2)
                    .ok_or("Expected mean_quality")?
                    .parse()
                    .map_err(|e| format!("bad mean_quality: {}", e))?;
                let table = params.data_table().ok_or("Expected data table")?;
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?;
                write_scorecard_fixture(hearth, &instance, &kind, mean_quality, table)
            },
        ),
        check_def(
            "a temper artifact quality for instance {string} kind {string}:",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let instance = params.get_string(0).ok_or("Expected instance")?.to_string();
                let kind = params.get_string(1).ok_or("Expected kind")?.to_string();
                let table = params.data_table().ok_or("Expected data table")?;
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?;
                write_artifact_quality_fixture(hearth, &instance, &kind, table)
            },
        ),
        check_def(
            "the atlas detail step from {string} to {string} role {string} has artifact_quality {int} sample_count {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let expected_score = params.get_int(3).ok_or("Expected artifact_quality")?;
                let expected_count = params.get_int(4).ok_or("Expected sample_count")? as u32;
                let resp = grpc_detail(&ctx)?;
                let row = resp
                    .step_quality
                    .iter()
                    .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
                    .ok_or_else(|| {
                        format!(
                            "no atlas detail step_quality row for {} -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                if row.artifact_sample_count == 0 {
                    return Err(format!(
                        "step {} -> {} role {} has no artifact_quality (sample_count 0)",
                        from_state, to_state, role
                    ));
                }
                let actual = row.artifact_quality.round() as i64;
                if actual != expected_score {
                    return Err(format!(
                        "expected artifact_quality {}, got {} (raw {})",
                        expected_score, actual, row.artifact_quality
                    ));
                }
                if row.artifact_sample_count != expected_count {
                    return Err(format!(
                        "expected artifact_sample_count {}, got {}",
                        expected_count, row.artifact_sample_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas detail step from {string} to {string} role {string} has no artifact_quality",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let resp = grpc_detail(&ctx)?;
                let row = resp
                    .step_quality
                    .iter()
                    .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
                    .ok_or_else(|| {
                        format!(
                            "no atlas detail step_quality row for {} -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                if row.artifact_sample_count != 0 {
                    return Err(format!(
                        "expected no artifact_quality for {} -> {} role {}, got {} (sample_count {})",
                        from_state, to_state, role, row.artifact_quality, row.artifact_sample_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas detail has measured_instance_count {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u32;
                let resp = grpc_detail(&ctx)?;
                if resp.measured_instance_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "detail measured_instance_count {} != expected {}",
                        resp.measured_instance_count, expected
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail has overall_mean_quality permille {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected permille")? as i64;
                let resp = grpc_detail(&ctx)?;
                let actual = (resp.overall_mean_quality * 1000.0).round() as i64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "detail overall_mean_quality permille {} != expected {} (raw {})",
                        actual, expected, resp.overall_mean_quality
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail step from {string} to {string} role {string} has mean_quality permille {int} sample_count {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let expected_permille = params.get_int(3).ok_or("Expected permille")? as i64;
                let expected_count = params.get_int(4).ok_or("Expected sample_count")? as u32;
                let resp = grpc_detail(&ctx)?;
                let row = resp
                    .step_quality
                    .iter()
                    .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
                    .ok_or_else(|| {
                        format!(
                            "no atlas detail step_quality row for {} -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                let actual_permille = (row.mean_quality * 1000.0).round() as i64;
                if actual_permille != expected_permille {
                    return Err(format!(
                        "step_quality mean_quality permille {} != expected {} (raw {})",
                        actual_permille, expected_permille, row.mean_quality
                    ));
                }
                if row.sample_count != expected_count {
                    return Err(format!(
                        "step_quality sample_count {} != expected {}",
                        row.sample_count, expected_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas detail has {int} recent measurements",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let resp = grpc_detail(&ctx)?;
                if resp.recent_measurements.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "detail recent_measurements len {} != expected {}",
                        resp.recent_measurements.len(),
                        expected
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail recent measurement {int} is instance {string} to_state {string}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let position = params.get_int(0).ok_or("Expected position")? as usize;
                let expected_instance = params.get_string(1).ok_or("Expected instance")?.to_string();
                let expected_to_state = params.get_string(2).ok_or("Expected to_state")?.to_string();
                let resp = grpc_detail(&ctx)?;
                let entry = resp
                    .recent_measurements
                    .get(position.saturating_sub(1))
                    .ok_or_else(|| format!("no recent measurement at position {}", position))?;
                if entry.instance_id != expected_instance {
                    return Err(format!(
                        "recent measurement {} instance '{}' != expected '{}'",
                        position, entry.instance_id, expected_instance
                    ));
                }
                if entry.to_state != expected_to_state {
                    return Err(format!(
                        "recent measurement {} to_state '{}' != expected '{}'",
                        position, entry.to_state, expected_to_state
                    ));
                }
                Ok(())
            },
        ),
        // ---- overall artifact-quality (the REAL headline number) --------
        check_def(
            "the atlas detail has overall_artifact_quality {int} sample_count {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected_score = params.get_int(0).ok_or("Expected overall_artifact_quality")?;
                let expected_count = params.get_int(1).ok_or("Expected sample_count")? as u32;
                let resp = grpc_detail(&ctx)?;
                let actual = resp.overall_artifact_quality.round() as i64;
                if actual != expected_score {
                    return Err(format!(
                        "expected overall_artifact_quality {}, got {} (raw {})",
                        expected_score, actual, resp.overall_artifact_quality
                    ));
                }
                if resp.artifact_measured_count != expected_count {
                    return Err(format!(
                        "expected artifact_measured_count {}, got {}",
                        expected_count, resp.artifact_measured_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the atlas detail has exactly {int} step_quality row for to_state {string} role {string}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let resp = grpc_detail(&ctx)?;
                let matching = resp
                    .step_quality
                    .iter()
                    .filter(|s| s.to_state == to_state && s.role == role)
                    .count();
                if matching == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected exactly {} step_quality row(s) for to_state '{}' role '{}', got {}",
                        expected, to_state, role, matching
                    ))
                }
            },
        ),
        check_def(
            "the atlas detail recent measurement {int} has artifact_quality {int} sample_count {int}",
            &[(PAD_GRPC_KEY, "GrpcDetail")],
            |ctx, params| {
                let position = params.get_int(0).ok_or("Expected position")? as usize;
                let expected_score = params.get_int(1).ok_or("Expected artifact_quality")?;
                let expected_count = params.get_int(2).ok_or("Expected sample_count")? as u32;
                let resp = grpc_detail(&ctx)?;
                let entry = resp
                    .recent_measurements
                    .get(position.saturating_sub(1))
                    .ok_or_else(|| format!("no recent measurement at position {}", position))?;
                let actual = entry.artifact_quality.round() as i64;
                if actual != expected_score {
                    return Err(format!(
                        "recent measurement {} artifact_quality {} != expected {} (raw {})",
                        position, actual, expected_score, entry.artifact_quality
                    ));
                }
                if entry.artifact_sample_count != expected_count {
                    return Err(format!(
                        "recent measurement {} artifact_sample_count {} != expected {}",
                        position, entry.artifact_sample_count, expected_count
                    ));
                }
                Ok(())
            },
        ),
        // ---- /ws detail artifact-quality assertions ----------------------
        check_def(
            "the /ws playbook_atlas_detail result step {string} role {string} has artifact_quality {int} sample_count {int}",
            &[(PAD_WS_KEY, "Value")],
            |ctx, params| {
                let to_state = params.get_string(0).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(1).ok_or("Expected role")?.to_string();
                let expected_score = params.get_int(2).ok_or("Expected artifact_quality")?;
                let expected_count = params.get_int(3).ok_or("Expected sample_count")? as i64;
                let result = ws_result(&ctx, PAD_WS_KEY)?;
                let step = result
                    .get("step_quality")
                    .and_then(Value::as_array)
                    .ok_or("/ws detail has no step_quality array")?
                    .iter()
                    .find(|s| {
                        s.get("to_state").and_then(Value::as_str) == Some(to_state.as_str())
                            && s.get("role").and_then(Value::as_str) == Some(role.as_str())
                    })
                    .ok_or_else(|| format!("/ws detail has no step_quality row for to_state '{}' role '{}'", to_state, role))?;
                let actual_score = step
                    .get("artifact_quality")
                    .and_then(Value::as_f64)
                    .ok_or("/ws step_quality row has no artifact_quality field")?
                    .round() as i64;
                if actual_score != expected_score {
                    return Err(format!(
                        "/ws step_quality artifact_quality {} != expected {}",
                        actual_score, expected_score
                    ));
                }
                let actual_count = step
                    .get("artifact_sample_count")
                    .and_then(Value::as_i64)
                    .ok_or("/ws step_quality row has no artifact_sample_count field")?;
                if actual_count != expected_count {
                    return Err(format!(
                        "/ws step_quality artifact_sample_count {} != expected {}",
                        actual_count, expected_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the /ws playbook_atlas_detail result has overall_artifact_quality {int} sample_count {int}",
            &[(PAD_WS_KEY, "Value")],
            |ctx, params| {
                let expected_score = params.get_int(0).ok_or("Expected overall_artifact_quality")?;
                let expected_count = params.get_int(1).ok_or("Expected sample_count")? as i64;
                let result = ws_result(&ctx, PAD_WS_KEY)?;
                let actual_score = result
                    .get("overall_artifact_quality")
                    .and_then(Value::as_f64)
                    .ok_or("/ws detail has no overall_artifact_quality")?
                    .round() as i64;
                if actual_score != expected_score {
                    return Err(format!(
                        "/ws overall_artifact_quality {} != expected {}",
                        actual_score, expected_score
                    ));
                }
                let actual_count = result
                    .get("artifact_measured_count")
                    .and_then(Value::as_i64)
                    .ok_or("/ws detail has no artifact_measured_count")?;
                if actual_count != expected_count {
                    return Err(format!(
                        "/ws artifact_measured_count {} != expected {}",
                        actual_count, expected_count
                    ));
                }
                Ok(())
            },
        ),
    ]
}
