//! Step module for `run_detail_rpc.feature` (engine seam).
//!
//! Seeds a HERMETIC fixture hearth holding a real run — a `status.yaml` with a
//! real `actors:` table and a real per-file transition event store under
//! `transitions/` — then asks the engine for that run's record over BOTH
//! surfaces: the gRPC `RunDetail` RPC and the `/ws` `run_detail` JSON-RPC
//! method. Every answer is normalized into one [`Answer`] shape and EVERY
//! assertion runs against both, plus an equality check between them. That is
//! the point: the two surfaces share `compute_run_detail`, and a claim that they
//! cannot diverge is worth exactly what a test that would notice the divergence
//! is worth.
//!
//! TWO THINGS IN THE FIXTURE ARE DELIBERATELY ADVERSARIAL, so the assertions
//! can actually fail:
//!
//! * the root run's three event FILES are named so their lexical order is the
//!   REVERSE of their timestamps. An implementation that returns them in
//!   `read_dir` / filename order reds on the ordering assertion instead of
//!   passing by luck.
//! * the two nested runs' IDS sort in the opposite order to their first steps.
//!   MEASURED: replacing the by-first-step ordering with `sort_by_key(id)` reds
//!   the nesting scenario here (and its sibling in
//!   `anvil-core/features/run_detail.feature`). Correlated ids would have let
//!   an id-ordered implementation pass the scenario named for the rule.

use anvil_test_support::engine::spawn_engine_for_hearth;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio_tungstenite::tungstenite::Message;

const HEARTH_KEY: &str = "rd_hearth";
const HEARTH_HANDLE_KEY: &str = "rd_hearth_handle";
const GRPC_KEY: &str = "rd_grpc_result";
const WS_KEY: &str = "rd_ws_response";

/// The run every scenario asks about.
const ROOT_RUN: &str = "20260210T0900_root_run";
/// The nested run whose FIRST STEP is earliest — and whose id sorts LAST.
const EARLY_CHILD: &str = "20260210T1400_alpha_child";
/// The nested run whose first step is latest — and whose id sorts FIRST.
const LATE_CHILD: &str = "20260210T1200_zulu_child";

/// Keys the fixture-seeding Givens thread forward. A brine Map step's output
/// context is retained to its declared `provides` only, so the second Given has
/// to re-emit what the first one produced.
const HEARTH_KEYS: &[(&str, &str)] = &[
    (HEARTH_KEY, "PathBuf"),
    (HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
];

/// Keys the two When steps produce: one raw answer per surface.
const ANSWER_KEYS: &[(&str, &str)] = &[(GRPC_KEY, "GrpcRunDetail"), (WS_KEY, "Value")];

enum GrpcRunDetail {
    Success(anvil_engine::proto::RunDetailResponse),
    Error { code: String, message: String },
}

// ── the fixture ────────────────────────────────────────────────────────────

fn write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {}", path.display(), e))
}

/// Write one REAL transition event file — the authoritative per-file record the
/// engine's fold reads. `file_name` is chosen by the caller so the fixture can
/// make filename order disagree with timestamp order.
fn write_event(
    run_dir: &Path,
    file_name: &str,
    to: &str,
    at: &str,
    actor: &str,
    role: &str,
    approver: Option<&str>,
    note: Option<&str>,
) -> Result<(), String> {
    let mut yaml = format!("to: {}\nat: \"{}\"\nactor: {}\nrole: {}\n", to, at, actor, role);
    if let Some(approver) = approver {
        yaml.push_str(&format!("approver: {}\n", approver));
    }
    if let Some(note) = note {
        yaml.push_str(&format!("note: \"{}\"\n", note));
    }
    write(&run_dir.join("transitions").join(file_name), &yaml)
}

/// Seed the run every scenario asks about: a real `actors:` table (one agent
/// with two configurations, one human) and three real transition events whose
/// FILENAMES sort in the reverse of their timestamps.
fn seed_root_run(hearth: &Path) -> Result<(), String> {
    let dir = hearth.join("tracks").join(ROOT_RUN);
    write(
        &dir.join("status.yaml"),
        "version: 1\n\
         kind: track\n\
         state: implementing\n\
         actors:\n\
         \x20 fable:\n\
         \x20   type: agent\n\
         \x20   configurations:\n\
         \x20     - at: \"2026-02-10T09:00:00Z\"\n\
         \x20       model: claude-sonnet\n\
         \x20       provider: anthropic\n\
         \x20     - at: \"2026-02-10T11:00:00Z\"\n\
         \x20       model: claude-opus-5\n\
         \x20       provider: anthropic\n\
         \x20 nick:\n\
         \x20   type: human\n\
         transitions: []\n",
    )?;
    // zzz / mmm / aaa: lexically DESCENDING while the timestamps ascend.
    write_event(&dir, "zzz_open.yaml", "spec", "2026-02-10T09:00:00Z", "fable", "doer", None, Some("opened the track"))?;
    write_event(&dir, "mmm_review.yaml", "spec_review", "2026-02-10T10:00:00Z", "fable", "doer", None, None)?;
    write_event(&dir, "aaa_approved.yaml", "implementing", "2026-02-10T11:00:00Z", "nick", "reviewer", Some("nick"), Some("approved the spec"))?;
    Ok(())
}

/// Seed one run started from inside the root run. `id` and `at` are decorrelated
/// on purpose — see the module doc.
fn seed_nested_run(hearth: &Path, id: &str, at: &str) -> Result<(), String> {
    let dir = hearth.join("tracks").join(id);
    write(
        &dir.join("status.yaml"),
        &format!(
            "version: 1\nkind: track\nstate: spec\nparent_id: {}\nactors: {{}}\ntransitions: []\n",
            ROOT_RUN
        ),
    )?;
    write_event(&dir, "0001_open.yaml", "spec", at, "opus", "doer", None, None)
}

fn hearth(ctx: &Context) -> Result<PathBuf, String> {
    ctx.get::<PathBuf>(HEARTH_KEY)
        .cloned()
        .ok_or_else(|| "No fixture hearth".to_string())
}

// ── asking both surfaces ───────────────────────────────────────────────────

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
                    .map_err(|e| format!("ws reply not JSON: {} (raw: {})", e, text))
            }
            Message::Binary(bytes) => {
                return serde_json::from_slice(&bytes)
                    .map_err(|e| format!("ws binary reply not JSON: {}", e))
            }
            Message::Close(_) => return Err("ws closed before a reply frame".to_string()),
            _ => continue,
        }
    }
    Err("ws stream ended before a reply frame".to_string())
}

/// Ask BOTH surfaces for `instance_id`'s record against the fixture hearth and
/// stash both answers. The engine is spawned here and dropped as soon as both
/// answers are in hand — every assertion runs on the captured answers.
async fn ask_both_surfaces(hearth: &Path, instance_id: &str) -> Result<Context, String> {
    let engine = spawn_engine_for_hearth(hearth)?;
    let port = engine.port;
    let hearth_path = hearth.display().to_string();

    let addr = format!("http://127.0.0.1:{}", port);
    let grpc =
        match anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr).await {
            Ok(mut client) => {
                let request = anvil_test_support::surfaced(anvil_engine::proto::RunDetailRequest {
                    hearth_path: hearth_path.clone(),
                    instance_id: instance_id.to_string(),
                });
                match client.run_detail(request).await {
                    Ok(response) => GrpcRunDetail::Success(response.into_inner()),
                    Err(status) => GrpcRunDetail::Error {
                        code: format!("{:?}", status.code()),
                        message: status.message().to_string(),
                    },
                }
            }
            Err(e) => GrpcRunDetail::Error {
                code: "UNAVAILABLE".to_string(),
                message: format!("Connection failed: {}", e),
            },
        };

    let ws = ws_roundtrip(
        port,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "run_detail",
            "params": { "surface": "test-harness", "hearth_path": hearth_path, "instance_id": instance_id }
        }),
    )
    .await?;
    drop(engine);

    let mut out = Context::new();
    out.set(GRPC_KEY, grpc);
    out.set::<Value>(WS_KEY, ws);
    Ok(out)
}

// ── one normalized answer shape, read off either surface ───────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    to_state: String,
    at: String,
    actor: String,
    role: String,
    approver: String,
    note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Actor {
    name: String,
    actor_type: String,
    model: String,
    provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    instance_id: String,
    kind: String,
    state: String,
    parent_instance_id: String,
    depth: u32,
    steps: Vec<Step>,
    actors: Vec<Actor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    found: bool,
    error_message: String,
    runs: Vec<Run>,
}

impl Answer {
    fn run(&self, instance_id: &str) -> Result<&Run, String> {
        self.runs
            .iter()
            .find(|r| r.instance_id == instance_id)
            .ok_or_else(|| {
                format!(
                    "the answer holds no run '{}' (it holds: {:?})",
                    instance_id,
                    self.runs.iter().map(|r| &r.instance_id).collect::<Vec<_>>()
                )
            })
    }

    fn order(&self) -> Vec<&str> {
        self.runs.iter().map(|r| r.instance_id.as_str()).collect()
    }
}

impl Run {
    fn step(&self, to_state: &str) -> Result<&Step, String> {
        self.steps
            .iter()
            .find(|s| s.to_state == to_state)
            .ok_or_else(|| format!("run '{}' has no step to '{}'", self.instance_id, to_state))
    }
}

fn grpc_answer(ctx: &Context) -> Result<Answer, String> {
    let response = match ctx.get::<GrpcRunDetail>(GRPC_KEY).ok_or("No RunDetail gRPC answer")? {
        GrpcRunDetail::Success(response) => response,
        GrpcRunDetail::Error { code, message } => {
            return Err(format!("expected an answer, got gRPC {}: {}", code, message))
        }
    };
    Ok(Answer {
        found: response.found,
        error_message: response.error_message.clone(),
        runs: response
            .nodes
            .iter()
            .map(|node| Run {
                instance_id: node.instance_id.clone(),
                kind: node.kind.clone(),
                state: node.state.clone(),
                parent_instance_id: node.parent_instance_id.clone(),
                depth: node.depth,
                steps: node
                    .steps
                    .iter()
                    .map(|s| Step {
                        to_state: s.to_state.clone(),
                        at: s.at.clone(),
                        actor: s.actor.clone(),
                        role: s.role.clone(),
                        approver: s.approver.clone(),
                        note: s.note.clone(),
                    })
                    .collect(),
                actors: node
                    .actors
                    .iter()
                    .map(|a| Actor {
                        name: a.name.clone(),
                        actor_type: a.actor_type.clone(),
                        model: a.model.clone(),
                        provider: a.provider.clone(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// The `/ws` answer's `result` object — the exact payload the panel consumes.
fn ws_result(ctx: &Context) -> Result<Value, String> {
    let response = ctx.get::<Value>(WS_KEY).ok_or("No /ws answer")?;
    if let Some(error) = response.get("error") {
        return Err(format!("expected a /ws result, got error: {}", error));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| format!("/ws answer has no result: {}", response))
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn ws_answer(ctx: &Context) -> Result<Answer, String> {
    let result = ws_result(ctx)?;
    Ok(Answer {
        found: result.get("found").and_then(Value::as_bool).unwrap_or(false),
        error_message: text(&result, "error_message"),
        runs: list(&result, "nodes")
            .iter()
            .map(|node| Run {
                instance_id: text(node, "instance_id"),
                kind: text(node, "kind"),
                state: text(node, "state"),
                parent_instance_id: text(node, "parent_instance_id"),
                depth: node.get("depth").and_then(Value::as_u64).unwrap_or(u32::MAX as u64) as u32,
                steps: list(node, "steps")
                    .iter()
                    .map(|s| Step {
                        to_state: text(s, "to_state"),
                        at: text(s, "at"),
                        actor: text(s, "actor"),
                        role: text(s, "role"),
                        approver: text(s, "approver"),
                        note: text(s, "note"),
                    })
                    .collect(),
                actors: list(node, "actors")
                    .iter()
                    .map(|a| Actor {
                        name: text(a, "name"),
                        actor_type: text(a, "actor_type"),
                        model: text(a, "model"),
                        provider: text(a, "provider"),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// Run one assertion against BOTH surfaces, and check the two answers are
/// IDENTICAL besides.
///
/// The identity check rides along on every assertion deliberately: the two
/// surfaces share `compute_run_detail`, and the only way that sharing stays true
/// is if a divergence reds something. Asserting content on one surface and
/// trusting the other would leave the serializer — the half that is NOT shared —
/// unchecked.
fn both<F>(ctx: &Context, assert: F) -> Result<(), String>
where
    F: Fn(&Answer) -> Result<(), String>,
{
    let grpc = grpc_answer(ctx)?;
    let ws = ws_answer(ctx)?;
    assert(&grpc).map_err(|e| format!("gRPC: {}", e))?;
    assert(&ws).map_err(|e| format!("/ws: {}", e))?;
    if grpc != ws {
        return Err(format!(
            "the two surfaces disagree — gRPC {:?} vs /ws {:?}",
            grpc, ws
        ));
    }
    Ok(())
}

// ── the no-cost check ──────────────────────────────────────────────────────

/// Every token a spend figure could reasonably be named, so the check catches a
/// field added under any of the names a future author might pick.
const COST_TOKENS: &[&str] = &[
    "cost", "spend", "price", "usd", "dollar", "token", "budget", "charge", "bill",
];

fn cost_shaped(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    COST_TOKENS.iter().any(|token| lower.contains(token))
}

fn all_keys(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                out.push(key.clone());
                all_keys(child, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|child| all_keys(child, out)),
        _ => {}
    }
}

/// Every field name the five RunDetail wire messages declare in
/// `proto/anvil.proto`.
///
/// The `/ws` payload alone cannot prove the gRPC surface carries no cost field:
/// a proto3 field left at its default is simply absent from a JSON view. The
/// CONTRACT is what has to be checked, so this reads the proto source.
fn run_detail_proto_fields() -> Result<Vec<String>, String> {
    let path = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .ok_or("no workspace root above anvil-test-support")?
        .join("proto/anvil.proto");
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let wanted = [
        "RunDetailRequest",
        "RunDetailResponse",
        "RunNode",
        "RunStep",
        "RunActor",
    ];
    let mut fields = Vec::new();
    let mut inside = false;
    let mut seen = 0usize;
    for raw in src.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("message ") {
            inside = rest
                .split_whitespace()
                .next()
                .is_some_and(|name| wanted.contains(&name));
            seen += usize::from(inside);
            continue;
        }
        if line == "}" {
            inside = false;
            continue;
        }
        if !inside || line.starts_with("//") {
            continue;
        }
        if let Some((decl, _)) = line.split_once('=') {
            if let Some(name) = decl.split_whitespace().last() {
                fields.push(name.to_string());
            }
        }
    }
    if seen != wanted.len() {
        return Err(format!(
            "found {} of the {} RunDetail messages in {} — the no-cost check would be vacuous",
            seen,
            wanted.len(),
            path.display()
        ));
    }
    Ok(fields)
}

// ── steps ──────────────────────────────────────────────────────────────────

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a hearth holding a run that has taken several steps",
            &[],
            HEARTH_KEYS,
            |_ctx, _params| async move {
                let (handle, tmp) = retained_temp_dir("anvil-run-detail-")?;
                std::fs::create_dir_all(tmp.join("tracks"))
                    .map_err(|e| format!("create tracks: {}", e))?;
                std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;
                seed_root_run(&tmp)?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, tmp);
                out.set::<RetainedTempDir>(HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        async_step_def(
            "two further runs were started from inside that run",
            HEARTH_KEYS,
            HEARTH_KEYS,
            |ctx, _params| async move {
                let tmp = hearth(&ctx)?;
                // Ids sort alpha-before-zulu; their first steps run the other
                // way round. An id-ordered implementation reds.
                seed_nested_run(&tmp, EARLY_CHILD, "2026-02-10T12:00:00Z")?;
                seed_nested_run(&tmp, LATE_CHILD, "2026-02-10T14:00:00Z")?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, tmp);
                if let Some(handle) = ctx.get::<RetainedTempDir>(HEARTH_HANDLE_KEY) {
                    out.set::<RetainedTempDir>(HEARTH_HANDLE_KEY, std::sync::Arc::clone(handle));
                }
                Ok(out)
            },
        ),
        async_step_def(
            "the run's record is asked for",
            HEARTH_KEYS,
            ANSWER_KEYS,
            |ctx, _params| async move { ask_both_surfaces(&hearth(&ctx)?, ROOT_RUN).await },
        ),
        async_step_def(
            "a record is asked for under a name no run has",
            HEARTH_KEYS,
            ANSWER_KEYS,
            |ctx, _params| async move {
                ask_both_surfaces(&hearth(&ctx)?, "20260210T0000_no_run_by_this_name").await
            },
        ),
        check_def(
            "the run comes back with its steps in the order they happened",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    if !answer.found {
                        return Err("the answer reports the run was not found".to_string());
                    }
                    let actual: Vec<&str> = answer
                        .run(ROOT_RUN)?
                        .steps
                        .iter()
                        .map(|s| s.to_state.as_str())
                        .collect();
                    let expected = ["spec", "spec_review", "implementing"];
                    if actual == expected {
                        Ok(())
                    } else {
                        Err(format!("steps {:?}, expected {:?}", actual, expected))
                    }
                })
            },
        ),
        check_def(
            "every step says when it happened and who took it",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    for step in &answer.run(ROOT_RUN)?.steps {
                        if step.at.is_empty() || step.actor.is_empty() {
                            return Err(format!(
                                "step to '{}' has at='{}' actor='{}' — a step must say when it \
                                 happened and who took it",
                                step.to_state, step.at, step.actor
                            ));
                        }
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "a step that needed approval names the person who approved it",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    let run = answer.run(ROOT_RUN)?;
                    let approved = run.step("implementing")?;
                    if approved.approver != "nick" {
                        return Err(format!(
                            "the step to 'implementing' names approver '{}', expected 'nick'",
                            approved.approver
                        ));
                    }
                    // The control: a step nobody had to approve must name NOBODY,
                    // so the assertion above cannot be satisfied by stamping every
                    // step with the same name.
                    let opened = run.step("spec")?;
                    if !opened.approver.is_empty() {
                        return Err(format!(
                            "the step to 'spec' needed no approval yet names approver '{}'",
                            opened.approver
                        ));
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "no step carries a cost figure",
            ANSWER_KEYS,
            |ctx, _params| {
                // Leg 1 — the CONTRACT. A proto3 field at its default is simply
                // absent from a JSON view, so the payload alone cannot prove the
                // gRPC surface carries no cost field. The proto source can.
                let offending: Vec<String> = run_detail_proto_fields()?
                    .into_iter()
                    .filter(|name| cost_shaped(name))
                    .collect();
                if !offending.is_empty() {
                    return Err(format!(
                        "the RunDetail wire messages declare cost-shaped field(s) {:?} — anvil \
                         records no cost anywhere, so any value there is fabricated",
                        offending
                    ));
                }
                // Leg 2 — the PAYLOAD the panel actually reads.
                let mut keys = Vec::new();
                all_keys(&ws_result(&ctx)?, &mut keys);
                let offending: Vec<String> =
                    keys.into_iter().filter(|key| cost_shaped(key)).collect();
                if !offending.is_empty() {
                    return Err(format!("the /ws payload carries cost-shaped key(s) {:?}", offending));
                }
                // The control: both legs above pass against an EMPTY answer.
                both(&ctx, |answer| {
                    if answer.runs.iter().all(|r| r.steps.is_empty()) {
                        Err("the answer carries no steps at all — the no-cost check would be \
                             vacuous"
                            .to_string())
                    } else {
                        Ok(())
                    }
                })
            },
        ),
        check_def(
            "the runs started from inside it come back nested under it",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    // Ordered by each nested run's FIRST STEP, ascending — which
                    // is the reverse of their ids.
                    let expected = [ROOT_RUN, EARLY_CHILD, LATE_CHILD];
                    if answer.order() != expected {
                        return Err(format!(
                            "record order {:?}, expected {:?}",
                            answer.order(),
                            expected
                        ));
                    }
                    for run in answer.runs.iter().skip(1) {
                        if run.depth != 1 || run.parent_instance_id != ROOT_RUN {
                            return Err(format!(
                                "run '{}' is at depth {} under '{}', expected depth 1 under '{}'",
                                run.instance_id, run.depth, run.parent_instance_id, ROOT_RUN
                            ));
                        }
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "each nested run carries its own steps",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    for (id, at) in [
                        (EARLY_CHILD, "2026-02-10T12:00:00Z"),
                        (LATE_CHILD, "2026-02-10T14:00:00Z"),
                    ] {
                        let run = answer.run(id)?;
                        match run.steps.as_slice() {
                            [step] if step.to_state == "spec" && step.at == at => {}
                            other => {
                                return Err(format!(
                                    "run '{}' carries {:?}, expected its OWN single step to 'spec' \
                                     at '{}'",
                                    id, other, at
                                ))
                            }
                        }
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "the run that was asked for is not itself nested under anything",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    let first = answer
                        .runs
                        .first()
                        .ok_or("the answer carries no runs at all")?;
                    if first.instance_id != ROOT_RUN {
                        return Err(format!(
                            "the answer leads with '{}', expected the run that was asked for ('{}')",
                            first.instance_id, ROOT_RUN
                        ));
                    }
                    if first.depth != 0 || !first.parent_instance_id.is_empty() {
                        return Err(format!(
                            "run '{}' is at depth {} nested under '{}', expected depth 0 under \
                             nothing",
                            first.instance_id, first.depth, first.parent_instance_id
                        ));
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "the answer says plainly that there is no such run",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    if answer.found {
                        return Err("found=true for a run that is not there".to_string());
                    }
                    if !answer.error_message.is_empty() {
                        return Err(format!(
                            "the answer errored with '{}' — a run that is not there is an ANSWER, \
                             not a failure",
                            answer.error_message
                        ));
                    }
                    Ok(())
                })
            },
        ),
        check_def(
            "the answer carries no runs at all",
            ANSWER_KEYS,
            |ctx, _params| {
                both(&ctx, |answer| {
                    if answer.runs.is_empty() {
                        Ok(())
                    } else {
                        Err(format!("the answer carries {:?}", answer.order()))
                    }
                })
            },
        ),
    ]
}
