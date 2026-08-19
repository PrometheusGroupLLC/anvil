//! Step module for `join_coverage_rpc.feature` and `join_coverage_ws.feature`
//! (engine seam, both of its surfaces).
//!
//! Seeds real hearths' delivery-log and activity-log sinks through the
//! PRODUCTION adapters, starts a real engine subprocess, and drives the gRPC
//! `JoinCoverage` RPC. Assertions run over a JSON projection of the response
//! built by exhaustively DESTRUCTURING the generated proto types, so a pooled
//! total added to the wire stops this module compiling rather than slipping
//! past a runtime key check.
//!
//! The /ws steps live HERE rather than in a module of their own, and they
//! project the reply frame's `result` under the SAME context key the gRPC steps
//! use. That is deliberate: every report assertion above then binds the /ws
//! payload verbatim, so the two surfaces are graded by one set of steps and a
//! divergence has nowhere to hide. The transport is `ws_bridge::ws_roundtrip`,
//! shared rather than re-spelled.
//!
//! Hearth basenames are fixed (`hearth-primary` / `hearth-secondary` /
//! `shared-hearth`) because `hearth_label` is that basename and a scenario
//! cannot name a random temp directory. The same-basename pair is assigned so
//! the LOWER canonical path is the primary, which makes the `#1` / `#2`
//! disambiguation ordinals deterministic without asserting anything about
//! tempfile's naming.

use anvil_core::domain::join_episode::JOIN_FILTER_VERSION;
use anvil_core::ports::activity_log_port::{ActivityLogRecord, ActivityLogWritePort};
use anvil_core::ports::delivery_log_port::{
    project_delivery_record, DeliveryObservation, DeliveryLogWritePort,
};
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core_hearth::fs_delivery_log_adapter::FileSystemDeliveryLogAdapter;
use anvil_engine::proto::{
    HearthJoinCoverageReport, JoinBeginUnjoinCounts, JoinCoverageRequest, JoinCoverageResponse,
    JoinTerminalCounts, JoinUnjoinCounts,
};
use anvil_test_support::engine::EngineProcess;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const RESULT_KEY: &str = "jc_result";
const RESULT_2_KEY: &str = "jc_result_2";
// The cross-hearth context keys are the usage-query family's, reused verbatim
// so `the engine is started with both permitted hearths` sees what it requires.
const HEARTH_B_KEY: &str = "uts_rpc_hearth_b";
const HEARTH_B_HANDLE_KEY: &str = "uts_rpc_hearth_b_handle";
const SALT_FILENAME: &str = ".telemetry-salt";
/// The one window both halves of the non-divergence pair ask over, so the
/// comparison covers the optional plumbing an arm could quietly drop and not
/// merely the two hearth paths.
const PARITY_WINDOW: (&str, &str) = ("2026-08-01T00:00:00Z", "2026-09-01T00:00:00Z");

const BOTH_HEARTHS: &[(&str, &str)] = &[
    ("hearth_path", "PathBuf"),
    ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
    (HEARTH_B_KEY, "PathBuf"),
    (HEARTH_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
];

const ENGINE: (&str, &str) = ("engine_process", "EngineProcess");
const HEARTH_A: (&str, &str) = ("hearth_path", "PathBuf");
const HEARTH_A_HANDLE: (&str, &str) = ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>");

/// A cell reading `-` is the table's empty marker; brine trims but does not
/// decode, so an absent value needs a visible token.
fn cell(table: &DataTable, row: &[String], name: &str) -> String {
    match table.headers.iter().position(|h| h == name) {
        Some(i) => {
            let v = row[i].trim();
            if v == "-" {
                String::new()
            } else {
                v.to_string()
            }
        }
        None => String::new(),
    }
}

fn opt(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Create `<temp>/<name>` as a hearth (the engine predicate needs `tracks/` and
/// `tracks.md`). The named subdirectory is what makes `hearth_label` assertable.
fn new_named_hearth(name: &str) -> Result<(RetainedTempDir, PathBuf), String> {
    let (handle, tmp) = retained_temp_dir("anvil-join-cov-")?;
    let hearth = tmp.join(name);
    std::fs::create_dir_all(hearth.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(hearth.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    Ok((handle, hearth))
}

/// Seed one hearth's delivery log through the production writer and the one
/// projection that carries the redaction contract — never a hand-built record.
fn seed_delivery(hearth: &Path, table: &DataTable, only_hearth: Option<&str>) -> Result<(), String> {
    let sink = FileSystemDeliveryLogAdapter::new(hearth);
    for row in &table.rows {
        if let Some(want) = only_hearth {
            if cell(table, row, "hearth") != want {
                continue;
            }
        }
        let record = project_delivery_record(&DeliveryObservation {
            at: &cell(table, row, "at"),
            source: "claude-code",
            project_root: "/synthetic/project-root",
            engine_conversation_hash: &cell(table, row, "conversation_hash"),
            guidance_kind: &cell(table, row, "guidance_kind"),
            engine_candidates: 1,
            guidance_produced: cell(table, row, "guidance_produced") == "true",
            guidance_bytes: 32,
            outcome: &cell(table, row, "outcome"),
            resume_source: "",
            router_cause: "",
        });
        sink.append_delivery_log(&record)
            .map_err(|e| format!("seed delivery log: {}", e))?;
    }
    Ok(())
}

fn seed_activity(hearth: &Path, table: &DataTable) -> Result<(), String> {
    let sink = FileSystemActivityLogAdapter::new(hearth);
    for row in &table.rows {
        sink.append_activity_log(&ActivityLogRecord {
            command: cell(table, row, "command"),
            outcome: "ok".to_string(),
            artifact_kind: cell(table, row, "artifact_kind"),
            from_state: String::new(),
            to_state: cell(table, row, "to_state"),
            actor_hash: None,
            at: cell(table, row, "at"),
            source: String::new(),
            conversation_hash: opt(cell(table, row, "conversation_hash")),
            project_label: None,
            playbook_run_id: opt(cell(table, row, "playbook_run_id")),
            call_state: None,
        })
        .map_err(|e| format!("seed activity log: {}", e))?;
    }
    Ok(())
}

fn carry_hearths(ctx: &Context, out: &mut Context) {
    for (key, handle) in [
        ("hearth_path", "hearth_path_handle"),
        (HEARTH_B_KEY, HEARTH_B_HANDLE_KEY),
    ] {
        if let Some(p) = ctx.get::<PathBuf>(key) {
            out.set(key, p.clone());
        }
        if let Some(h) = ctx.get::<RetainedTempDir>(handle) {
            out.set::<RetainedTempDir>(handle, std::sync::Arc::clone(h));
        }
    }
}

/// Seeding a hearth must hand back the WHOLE hearth set. Brine retains only a
/// step's declared `provides`, so a step that returns `hearth_path` alone drops
/// `hearth_path_handle` — the `TempDir` is dropped, the hearth directory is
/// deleted, and the engine the next step starts has nothing to open.
fn seed_activity_step(
    ctx: Context,
    params: &brine_runner_rust::registry::Params,
    which: &str,
) -> Result<Context, String> {
    let table = params.data_table().ok_or("Expected data table")?;
    seed_activity(&hearth_of(&ctx, which)?, table)?;
    let mut out = Context::new();
    carry_hearths(&ctx, &mut out);
    Ok(out)
}

fn hearth_of(ctx: &Context, which: &str) -> Result<PathBuf, String> {
    let key = match which {
        "primary" => "hearth_path",
        "secondary" => HEARTH_B_KEY,
        other => return Err(format!("Unknown hearth name '{}'", other)),
    };
    ctx.get::<PathBuf>(key)
        .cloned()
        .ok_or_else(|| format!("No {} hearth in context", which))
}

// ── the response projection ────────────────────────────────────────────────
// Every mapping below destructures its proto message EXHAUSTIVELY. A field
// added to the wire — a pooled total above all — fails to compile here, which
// is a stronger guard than any runtime key assertion this module could write.

fn unjoin_json(c: &JoinUnjoinCounts) -> Value {
    let JoinUnjoinCounts {
        no_conversation_key,
        pre_migration_row,
        conversation_absent_from_begin_side,
        no_begin_of_kind_in_conversation,
        superseded_by_later_delivery_of_kind,
    } = c;
    json!({
        "no_conversation_key": no_conversation_key,
        "pre_migration_row": pre_migration_row,
        "conversation_absent_from_begin_side": conversation_absent_from_begin_side,
        "no_begin_of_kind_in_conversation": no_begin_of_kind_in_conversation,
        "superseded_by_later_delivery_of_kind": superseded_by_later_delivery_of_kind,
    })
}

fn begin_unjoin_json(c: &JoinBeginUnjoinCounts) -> Value {
    let JoinBeginUnjoinCounts {
        no_conversation_key,
        conversation_absent_from_delivery_side,
        no_prior_delivery_of_kind,
        no_unconsumed_prior_delivery_of_kind,
    } = c;
    json!({
        "no_conversation_key": no_conversation_key,
        "conversation_absent_from_delivery_side": conversation_absent_from_delivery_side,
        "no_prior_delivery_of_kind": no_prior_delivery_of_kind,
        "no_unconsumed_prior_delivery_of_kind": no_unconsumed_prior_delivery_of_kind,
    })
}

fn terminal_json(c: &JoinTerminalCounts) -> Value {
    let JoinTerminalCounts {
        not_joined,
        not_yet_terminal,
        reached_terminal,
        unknown_run_state,
    } = c;
    json!({
        "not_joined": not_joined,
        "not_yet_terminal": not_yet_terminal,
        "reached_terminal": reached_terminal,
        "unknown_run_state": unknown_run_state,
    })
}

fn hearth_json(r: &HearthJoinCoverageReport) -> Value {
    let HearthJoinCoverageReport {
        hearth_label,
        window_start,
        window_end,
        delivery_rows_read,
        read_defects,
        activity_rows_scanned,
        activity_rows_retained,
        begin_rows_read,
        episode_denominator,
        begin_denominator,
        menu_delivered,
        nothing_delivered,
        no_engine_answer,
        joined,
        unjoin,
        begin_unjoin,
        terminal,
        key_epoch,
        hearth_salt_file_epoch,
        key_epoch_reconciliation,
    } = r;
    json!({
        "hearth_label": hearth_label,
        "window_start": window_start,
        "window_end": window_end,
        "delivery_rows_read": delivery_rows_read,
        "read_defects": read_defects,
        "activity_rows_scanned": activity_rows_scanned,
        "activity_rows_retained": activity_rows_retained,
        "begin_rows_read": begin_rows_read,
        "episode_denominator": episode_denominator,
        "begin_denominator": begin_denominator,
        "menu_delivered": menu_delivered,
        "nothing_delivered": nothing_delivered,
        "no_engine_answer": no_engine_answer,
        "joined": joined,
        "unjoin": unjoin.as_ref().map(unjoin_json).unwrap_or(Value::Null),
        "begin_unjoin": begin_unjoin.as_ref().map(begin_unjoin_json).unwrap_or(Value::Null),
        "terminal": terminal.as_ref().map(terminal_json).unwrap_or(Value::Null),
        "key_epoch": key_epoch,
        "hearth_salt_file_epoch": hearth_salt_file_epoch,
        "key_epoch_reconciliation": key_epoch_reconciliation,
    })
}

fn response_json(resp: &JoinCoverageResponse) -> Value {
    let JoinCoverageResponse {
        per_hearth,
        filter_version,
    } = resp;
    json!({
        "per_hearth": per_hearth.iter().map(hearth_json).collect::<Vec<_>>(),
        "filter_version": filter_version,
    })
}

/// One RPC call, projected. A `Status` is stored as an `error` object rather
/// than failing the step, so a scenario can assert the fail-closed rule.
async fn call_join_coverage(port: u16, request: JoinCoverageRequest) -> Result<Value, String> {
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = anvil_engine::proto::anvil_service_client::AnvilServiceClient::connect(addr)
        .await
        .map_err(|e| format!("connect failed: {}", e))?;
    match client
        .join_coverage(anvil_test_support::surfaced(request))
        .await
    {
        Ok(resp) => Ok(response_json(&resp.into_inner())),
        Err(status) => Ok(json!({
            "error": { "code": format!("{:?}", status.code()), "message": status.message() }
        })),
    }
}

/// Every `When` in this feature is the same call with a different request, so
/// each step is its request shape and nothing else.
///
/// The call carries the hearth set forward as well as its result. Brine retains
/// only a step's declared `provides`, so a `When` that returned the response
/// alone would drop the temp-dir handles and delete the hearths out from under
/// its own `Then`s — including the one that proves no `.telemetry-salt` was
/// written.
fn call_step(
    pattern: &'static str,
    key: &'static str,
    build: fn(&Context, &brine_runner_rust::registry::Params) -> Result<JoinCoverageRequest, String>,
) -> StepDef {
    let mut provides: Vec<(&str, &str)> = vec![(key, "Value"), ENGINE];
    if key == RESULT_2_KEY {
        provides.push((RESULT_KEY, "Value"));
    }
    provides.extend_from_slice(BOTH_HEARTHS);
    async_step_def(
        pattern,
        &[ENGINE],
        &provides,
        move |mut ctx, params| async move {
            let request = build(&ctx, &params)?;
            let engine = ctx
                .take::<EngineProcess>("engine_process")
                .ok_or("No engine_process")?;
            let value = call_join_coverage(engine.port, request).await?;
            let mut out = Context::new();
            carry_hearths(&ctx, &mut out);
            if let Some(first) = ctx.get::<Value>(RESULT_KEY) {
                out.set::<Value>(RESULT_KEY, first.clone());
            }
            out.set::<Value>(key, value);
            out.set("engine_process", engine);
            Ok(out)
        },
    )
}

/// The /ws twin of `call_step`. One JSON-RPC frame over the shared transport,
/// projected onto the SAME keys the gRPC steps assert over: the whole envelope
/// under `ws_bridge`'s response key (so the shipped envelope assertions bite
/// verbatim) and its `result` under `RESULT_KEY` (so every report assertion in
/// this module binds the /ws payload too). An error frame stores the error
/// shape rather than failing the step, so a scenario can assert the fail-closed
/// rule. `surface` is always named — the bridge refuses an unattributed read.
fn ws_call_step(pattern: &'static str, build: fn(&Context) -> Result<Value, String>) -> StepDef {
    let mut provides: Vec<(&str, &str)> = vec![
        (RESULT_KEY, "Value"),
        (crate::ws_bridge::WS_RESPONSE_KEY, "Value"),
        ENGINE,
    ];
    provides.extend_from_slice(BOTH_HEARTHS);
    async_step_def(
        pattern,
        &[ENGINE],
        &provides,
        move |mut ctx, _params| async move {
            let mut request_params = build(&ctx)?;
            request_params["surface"] = json!("test-harness");
            let engine = ctx
                .take::<EngineProcess>("engine_process")
                .ok_or("No engine_process")?;
            let frame = json!({
                "jsonrpc": "2.0",
                "id": 31,
                "method": "join_coverage",
                "params": request_params,
            });
            let response = crate::ws_bridge::ws_roundtrip(engine.port, &frame).await?;
            let projected = match response.get("result") {
                Some(result) => result.clone(),
                None => json!({ "error": response.get("error").cloned().unwrap_or(Value::Null) }),
            };
            let mut out = Context::new();
            carry_hearths(&ctx, &mut out);
            out.set::<Value>(RESULT_KEY, projected);
            out.set::<Value>(crate::ws_bridge::WS_RESPONSE_KEY, response);
            out.set("engine_process", engine);
            Ok(out)
        },
    )
}

/// D13 #4 at the /ws surface: the payload's key set, declared once. Anything
/// off this list fails the scenario whatever it is called — a pooled total, a
/// raw path, a salt, a conversation id, or a field a future change adds without
/// amending this list. The nested groups are listed with their parents because
/// the check walks the payload rather than its top level.
const WS_PAYLOAD_ALLOWLIST: &[&str] = &[
    // the envelope
    "per_hearth",
    "filter_version",
    // one per-hearth report
    "hearth_label",
    "window_start",
    "window_end",
    "delivery_rows_read",
    "read_defects",
    "activity_rows_scanned",
    "activity_rows_retained",
    "begin_rows_read",
    "episode_denominator",
    "begin_denominator",
    "menu_delivered",
    "nothing_delivered",
    "no_engine_answer",
    "joined",
    "unjoin",
    "begin_unjoin",
    "terminal",
    "key_epoch",
    "hearth_salt_file_epoch",
    "key_epoch_reconciliation",
    // unjoin
    "no_conversation_key",
    "pre_migration_row",
    "conversation_absent_from_begin_side",
    "no_begin_of_kind_in_conversation",
    "superseded_by_later_delivery_of_kind",
    // begin_unjoin
    "conversation_absent_from_delivery_side",
    "no_prior_delivery_of_kind",
    "no_unconsumed_prior_delivery_of_kind",
    // terminal
    "not_joined",
    "not_yet_terminal",
    "reached_terminal",
    "unknown_run_state",
];

/// A sibling of the primary hearth, made a real hearth so the policy refuses it
/// for being outside the permitted roots rather than for not being a hearth.
fn unpermitted_sibling(primary: &Path) -> Result<String, String> {
    let unpermitted = primary
        .parent()
        .ok_or("primary hearth has no parent")?
        .join("hearth-unpermitted");
    std::fs::create_dir_all(unpermitted.join("tracks"))
        .map_err(|e| format!("create unpermitted hearth: {}", e))?;
    std::fs::write(unpermitted.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;
    Ok(as_str(&unpermitted))
}

fn blank_request() -> JoinCoverageRequest {
    JoinCoverageRequest {
        hearth_path: String::new(),
        hearth_paths: Vec::new(),
        window_start: String::new(),
        window_end: String::new(),
        project_label: String::new(),
    }
}

fn as_str(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

// ── assertion helpers ──────────────────────────────────────────────────────

fn result(ctx: &Context, key: &str) -> Result<Value, String> {
    ctx.get::<Value>(key)
        .cloned()
        .ok_or_else(|| format!("No {} in context", key))
}

fn reports(ctx: &Context) -> Result<Vec<Value>, String> {
    let value = result(ctx, RESULT_KEY)?;
    if let Some(error) = value.get("error") {
        return Err(format!("Expected a report, got error: {}", error));
    }
    Ok(value["per_hearth"].as_array().cloned().unwrap_or_default())
}

fn report_for(ctx: &Context, label: &str) -> Result<Value, String> {
    let all = reports(ctx)?;
    let matches: Vec<&Value> = all
        .iter()
        .filter(|r| r["hearth_label"] == json!(label))
        .collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(format!(
            "No per-hearth report labelled '{}'; labels were {:?}",
            label,
            all.iter().map(|r| r["hearth_label"].clone()).collect::<Vec<_>>()
        )),
        n => Err(format!(
            "{} per-hearth reports are labelled '{}' — one hearth must yield one report",
            n, label
        )),
    }
}

/// `a.b` reaches a nested bucket; a bare name reaches a top-level count.
fn dotted<'a>(report: &'a Value, field: &str) -> Result<&'a Value, String> {
    let mut cursor = report;
    for part in field.split('.') {
        cursor = cursor
            .get(part)
            .ok_or_else(|| format!("Report has no field '{}' (at '{}')", field, part))?;
    }
    Ok(cursor)
}

fn number(report: &Value, field: &str) -> Result<u64, String> {
    dotted(report, field)?
        .as_u64()
        .ok_or_else(|| format!("Field '{}' is not a number", field))
}

fn text(report: &Value, field: &str) -> Result<String, String> {
    Ok(dotted(report, field)?
        .as_str()
        .ok_or_else(|| format!("Field '{}' is not a string", field))?
        .to_string())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== hearth seeding =====
        step_def(
            "a delivery log primary hearth seeded with rows:",
            &[],
            &[HEARTH_A, HEARTH_A_HANDLE],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, hearth) = new_named_hearth("hearth-primary")?;
                seed_delivery(&hearth, table, None)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a delivery log primary hearth with no sinks",
            &[],
            &[HEARTH_A, HEARTH_A_HANDLE],
            |_ctx, _params| {
                let (handle, hearth) = new_named_hearth("hearth-primary")?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a delivery log secondary hearth seeded with rows:",
            &[HEARTH_A, HEARTH_A_HANDLE],
            &[
                HEARTH_A,
                HEARTH_A_HANDLE,
                (HEARTH_B_KEY, "PathBuf"),
                (HEARTH_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (handle, hearth) = new_named_hearth("hearth-secondary")?;
                seed_delivery(&hearth, table, None)?;
                let mut out = Context::new();
                carry_hearths(&ctx, &mut out);
                out.set(HEARTH_B_KEY, hearth);
                out.set::<RetainedTempDir>(HEARTH_B_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "two same-named delivery log hearths seeded with rows:",
            &[],
            &[
                HEARTH_A,
                HEARTH_A_HANDLE,
                (HEARTH_B_KEY, "PathBuf"),
                (HEARTH_B_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let (h1, p1) = new_named_hearth("shared-hearth")?;
                let (h2, p2) = new_named_hearth("shared-hearth")?;
                // The disambiguation ordinal is assigned in canonical-path
                // order, so the scenario can only name `#1` if the lower path is
                // the one it calls primary. Tempfile's names are random; the
                // assignment is not.
                let ((ha, pa), (hb, pb)) = if p1 <= p2 {
                    ((h1, p1), (h2, p2))
                } else {
                    ((h2, p2), (h1, p1))
                };
                seed_delivery(&pa, table, Some("primary"))?;
                seed_delivery(&pb, table, Some("secondary"))?;
                let mut out = Context::new();
                out.set("hearth_path", pa);
                out.set::<RetainedTempDir>("hearth_path_handle", ha);
                out.set(HEARTH_B_KEY, pb);
                out.set::<RetainedTempDir>(HEARTH_B_HANDLE_KEY, hb);
                Ok(out)
            },
        ),
        step_def(
            "the primary hearth activity log is seeded with turns:",
            &[HEARTH_A],
            BOTH_HEARTHS,
            |ctx, params| seed_activity_step(ctx, params, "primary"),
        ),
        step_def(
            "the secondary hearth activity log is seeded with turns:",
            &[(HEARTH_B_KEY, "PathBuf")],
            BOTH_HEARTHS,
            |ctx, params| seed_activity_step(ctx, params, "secondary"),
        ),
        step_def(
            "a telemetry salt file containing {string} in the {string} hearth",
            &[HEARTH_A],
            BOTH_HEARTHS,
            |ctx, params| {
                let salt = params.get_string(0).unwrap_or_default().to_string();
                let which = params.get_string(1).unwrap_or_default().to_string();
                std::fs::write(hearth_of(&ctx, &which)?.join(SALT_FILENAME), salt)
                    .map_err(|e| format!("plant salt: {}", e))?;
                let mut out = Context::new();
                carry_hearths(&ctx, &mut out);
                Ok(out)
            },
        ),
        // ===== the request shapes =====
        call_step(
            "the JoinCoverage RPC is called",
            RESULT_KEY,
            |_ctx, _params| Ok(blank_request()),
        ),
        call_step(
            "the JoinCoverage RPC is called for the window {string} to {string}",
            RESULT_KEY,
            |_ctx, params| {
                Ok(JoinCoverageRequest {
                    window_start: params.get_string(0).unwrap_or_default().to_string(),
                    window_end: params.get_string(1).unwrap_or_default().to_string(),
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the JoinCoverage RPC is called naming both hearths in the hearth list",
            RESULT_KEY,
            |ctx, _params| {
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![
                        as_str(&hearth_of(ctx, "primary")?),
                        as_str(&hearth_of(ctx, "secondary")?),
                    ],
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the same JoinCoverage request is reissued with its hearth list reversed",
            RESULT_2_KEY,
            |ctx, _params| {
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![
                        as_str(&hearth_of(ctx, "secondary")?),
                        as_str(&hearth_of(ctx, "primary")?),
                    ],
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the JoinCoverage RPC is called naming the primary hearth in both request fields and the secondary hearth in the hearth list",
            RESULT_KEY,
            |ctx, _params| {
                let primary = hearth_of(ctx, "primary")?;
                // A SECOND SPELLING of the primary in the list, so a dedup that
                // compares request strings before canonicalizing reports the
                // primary twice and this scenario fails.
                Ok(JoinCoverageRequest {
                    hearth_path: as_str(&primary),
                    hearth_paths: vec![
                        other_spelling(&primary)?,
                        as_str(&hearth_of(ctx, "secondary")?),
                    ],
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the JoinCoverage RPC is called naming the primary hearth twice in two spellings",
            RESULT_KEY,
            |ctx, _params| {
                let primary = hearth_of(ctx, "primary")?;
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![as_str(&primary), other_spelling(&primary)?],
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the JoinCoverage RPC is called naming the secondary hearth beside an empty hearth list entry",
            RESULT_KEY,
            |ctx, _params| {
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![String::new(), as_str(&hearth_of(ctx, "secondary")?)],
                    ..blank_request()
                })
            },
        ),
        call_step(
            "the JoinCoverage RPC is called naming the primary hearth and the unpermitted hearth",
            RESULT_KEY,
            |ctx, _params| {
                // A SIBLING of the primary hearth, not a child: the permitted
                // root is the hearth directory itself, so a sibling is a real
                // hearth the policy refuses. Built here rather than in a Given
                // because brine retains only a step's declared `provides`, and
                // the engine-start step in between would drop its temp handle.
                let primary = hearth_of(ctx, "primary")?;
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![as_str(&primary), unpermitted_sibling(&primary)?],
                    ..blank_request()
                })
            },
        ),
        // ===== the same question, over /ws =====
        // The bridge is the surface a person reaches; these steps ask it the
        // same questions the RPC steps above ask, and store the answer under the
        // same key so the same assertions grade both.
        ws_call_step(
            "a join coverage request is sent over /ws naming the primary hearth and the unpermitted hearth",
            |ctx| {
                let primary = hearth_of(ctx, "primary")?;
                Ok(json!({
                    "hearth_paths": [as_str(&primary), unpermitted_sibling(&primary)?],
                }))
            },
        ),
        // The window is carried DELIBERATELY, and the gRPC twin below carries
        // the same one: a parity check over a request that exercises no
        // optional plumbing cannot catch an arm that drops it.
        ws_call_step(
            "a join coverage request is sent over /ws naming both hearths",
            |ctx| {
                Ok(json!({
                    "hearth_paths": [
                        as_str(&hearth_of(ctx, "primary")?),
                        as_str(&hearth_of(ctx, "secondary")?),
                    ],
                    "window_start": PARITY_WINDOW.0,
                    "window_end": PARITY_WINDOW.1,
                }))
            },
        ),
        ws_call_step(
            "a join coverage request is sent over /ws",
            |_ctx| Ok(json!({})),
        ),
        // The gRPC half of the non-divergence pair. It reuses `call_step`, so
        // the comparison is between the two SURFACES and not between two
        // differently-built client calls.
        call_step(
            "the same join coverage question is asked over gRPC",
            RESULT_2_KEY,
            |ctx, _params| {
                Ok(JoinCoverageRequest {
                    hearth_paths: vec![
                        as_str(&hearth_of(ctx, "primary")?),
                        as_str(&hearth_of(ctx, "secondary")?),
                    ],
                    window_start: PARITY_WINDOW.0.to_string(),
                    window_end: PARITY_WINDOW.1.to_string(),
                    ..blank_request()
                })
            },
        ),
        // ===== assertions =====
        check_def(
            "the JoinCoverage response has {int} per-hearth reports",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_int(0).unwrap_or(0) as usize;
                let actual = reports(&ctx)?;
                if actual.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected {} per-hearth reports, got {}: {:?}",
                        expected,
                        actual.len(),
                        actual.iter().map(|r| r["hearth_label"].clone()).collect::<Vec<_>>()
                    ))
                }
            },
        ),
        check_def(
            "the per-hearth report labels are {string} in any order",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let mut expected: Vec<String> = params
                    .get_string(0)
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                expected.sort();
                let mut actual: Vec<String> = reports(&ctx)?
                    .iter()
                    .map(|r| r["hearth_label"].as_str().unwrap_or("").to_string())
                    .collect();
                actual.sort();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("Expected labels {:?}, got {:?}", expected, actual))
                }
            },
        ),
        check_def(
            "the per-hearth report for {string} counts {string} as {int}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).unwrap_or_default().to_string();
                let field = params.get_string(1).unwrap_or_default().to_string();
                let expected = params.get_int(2).unwrap_or(0) as u64;
                let actual = number(&report_for(&ctx, &label)?, &field)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "{} / {}: expected {}, got {}",
                        label, field, expected, actual
                    ))
                }
            },
        ),
        // The end-to-end scenario drives the REAL binaries against a hearth
        // this module did not create, so its basename is a tempfile's and no
        // scenario can name it. Naming the report by its cardinality instead is
        // exact: the assertion still fails if a second report appears.
        check_def(
            "the only per-hearth report counts {string} as {int}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let field = params.get_string(0).unwrap_or_default().to_string();
                let expected = params.get_int(1).unwrap_or(0) as u64;
                let all = reports(&ctx)?;
                if all.len() != 1 {
                    return Err(format!(
                        "Expected exactly 1 per-hearth report, got {}: {:?}",
                        all.len(),
                        all.iter().map(|r| r["hearth_label"].clone()).collect::<Vec<_>>()
                    ));
                }
                let actual = number(&all[0], &field)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("{}: expected {}, got {}", field, expected, actual))
                }
            },
        ),
        check_def(
            "the per-hearth report for {string} declares {string} as {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).unwrap_or_default().to_string();
                let field = params.get_string(1).unwrap_or_default().to_string();
                let expected = params.get_string(2).unwrap_or_default().to_string();
                let actual = text(&report_for(&ctx, &label)?, &field)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "{} / {}: expected '{}', got '{}'",
                        label, field, expected, actual
                    ))
                }
            },
        ),
        check_def(
            "every per-hearth report declares {string} as {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let field = params.get_string(0).unwrap_or_default().to_string();
                let expected = params.get_string(1).unwrap_or_default().to_string();
                for report in reports(&ctx)? {
                    let actual = text(&report, &field)?;
                    if actual != expected {
                        return Err(format!(
                            "{} / {}: expected '{}', got '{}'",
                            report["hearth_label"], field, expected, actual
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "every per-hearth report declares a non-empty {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let field = params.get_string(0).unwrap_or_default().to_string();
                for report in reports(&ctx)? {
                    if text(&report, &field)?.is_empty() {
                        return Err(format!("{} / {} is empty", report["hearth_label"], field));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the per-hearth report for {string} buckets sum to its declared denominators",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let label = params.get_string(0).unwrap_or_default().to_string();
                let report = report_for(&ctx, &label)?;
                let joined = number(&report, "joined")?;
                let unjoin: u64 = ["no_conversation_key", "pre_migration_row",
                    "conversation_absent_from_begin_side", "no_begin_of_kind_in_conversation",
                    "superseded_by_later_delivery_of_kind"]
                    .iter()
                    .map(|f| number(&report, &format!("unjoin.{}", f)).unwrap_or(u64::MAX))
                    .sum();
                let begin_unjoin: u64 = ["no_conversation_key",
                    "conversation_absent_from_delivery_side", "no_prior_delivery_of_kind",
                    "no_unconsumed_prior_delivery_of_kind"]
                    .iter()
                    .map(|f| number(&report, &format!("begin_unjoin.{}", f)).unwrap_or(u64::MAX))
                    .sum();
                let episodes = number(&report, "episode_denominator")?;
                let begins = number(&report, "begin_denominator")?;
                if joined + unjoin != episodes {
                    return Err(format!(
                        "episode buckets {} + {} != denominator {}",
                        joined, unjoin, episodes
                    ));
                }
                if joined + begin_unjoin != begins {
                    return Err(format!(
                        "begin buckets {} + {} != denominator {}",
                        joined, begin_unjoin, begins
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "every per-hearth report carries the same key epoch",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                let all = reports(&ctx)?;
                let epochs: Vec<String> = all
                    .iter()
                    .map(|r| r["key_epoch"].as_str().unwrap_or("").to_string())
                    .collect();
                if all.len() < 2 {
                    return Err(format!(
                        "Only {} per-hearth report(s) — this assertion cannot bite",
                        all.len()
                    ));
                }
                if epochs.iter().all(|e| *e == epochs[0]) {
                    Ok(())
                } else {
                    Err(format!("Key epochs differ across hearths: {:?}", epochs))
                }
            },
        ),
        check_def(
            "every per-hearth report key epoch is 12 lowercase hex characters or the unknown sentinel",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                for report in reports(&ctx)? {
                    let epoch = text(&report, "key_epoch")?;
                    let hex = epoch.len() == 12
                        && epoch.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
                    if !hex && epoch != "unknown_key_epoch" {
                        return Err(format!("Key epoch '{}' is neither form", epoch));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the report filter version is the {string} constant",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let name = params.get_string(0).unwrap_or_default().to_string();
                let expected = match name.as_str() {
                    "JOIN_FILTER_VERSION" => JOIN_FILTER_VERSION,
                    other => return Err(format!("Unknown filter-version constant '{}'", other)),
                };
                let actual = result(&ctx, RESULT_KEY)?["filter_version"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "filter_version '{}' is not {} ('{}')",
                        actual, name, expected
                    ))
                }
            },
        ),
        check_def(
            "the serialized report exposes no pooled coverage key",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                let value = result(&ctx, RESULT_KEY)?;
                let keys: Vec<String> = value
                    .as_object()
                    .ok_or("Response is not an object")?
                    .keys()
                    .cloned()
                    .collect();
                let allowed = ["per_hearth", "filter_version"];
                for key in &keys {
                    if !allowed.contains(&key.as_str()) {
                        return Err(format!("Response carries an unlisted key '{}'", key));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "every key on the /ws join coverage payload is on the declared allowlist",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                let payload = result(&ctx, RESULT_KEY)?;
                // An allowlist over an empty key set allows everything. A
                // payload with no per-hearth report is not a clean payload, it
                // is an unexamined one.
                if payload["per_hearth"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(true)
                {
                    return Err(format!(
                        "The /ws payload carries no per-hearth report, so its key set proves \
                         nothing: {}",
                        payload
                    ));
                }
                fn walk(value: &Value, out: &mut Vec<String>) {
                    match value {
                        Value::Object(map) => {
                            for (key, child) in map {
                                out.push(key.clone());
                                walk(child, out);
                            }
                        }
                        Value::Array(items) => items.iter().for_each(|c| walk(c, out)),
                        _ => {}
                    }
                }
                let mut keys: Vec<String> = Vec::new();
                walk(&payload, &mut keys);
                let offenders: Vec<&String> = keys
                    .iter()
                    .filter(|k| !WS_PAYLOAD_ALLOWLIST.contains(&k.as_str()))
                    .collect();
                if offenders.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "The /ws join coverage payload carries unlisted key(s) {:?}, across {} \
                         keys examined",
                        offenders,
                        keys.len()
                    ))
                }
            },
        ),
        check_def(
            "the serialized JoinCoverage response does not contain raw text {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let needle = params.get_string(0).unwrap_or_default().to_string();
                let serialized = result(&ctx, RESULT_KEY)?.to_string();
                if serialized.contains(&needle) {
                    Err(format!("Response contains raw text '{}'", needle))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the serialized JoinCoverage response contains no hearth path",
            &[(RESULT_KEY, "Value"), HEARTH_A],
            |ctx, _params| {
                let serialized = result(&ctx, RESULT_KEY)?.to_string();
                let hearth = hearth_of(&ctx, "primary")?;
                for candidate in [as_str(&hearth), as_str(hearth.parent().unwrap_or(&hearth))] {
                    if serialized.contains(&candidate) {
                        return Err(format!("Response contains the path '{}'", candidate));
                    }
                }
                for report in reports(&ctx)? {
                    for (key, value) in report.as_object().ok_or("report is not an object")? {
                        if value.as_str().is_some_and(|s| s.contains('/')) {
                            return Err(format!("Field '{}' looks like a path: {}", key, value));
                        }
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the JoinCoverage RPC fails with {string}",
            &[(RESULT_KEY, "Value")],
            |ctx, params| {
                let expected = params.get_string(0).unwrap_or_default().to_string();
                let value = result(&ctx, RESULT_KEY)?;
                match value.get("error") {
                    Some(error) if error["code"] == json!(expected) => Ok(()),
                    Some(error) => Err(format!("Expected {}, got {}", expected, error)),
                    None => Err(format!(
                        "Expected {} and no report, got a report with {} hearth entries",
                        expected,
                        value["per_hearth"].as_array().map(|a| a.len()).unwrap_or(0)
                    )),
                }
            },
        ),
        check_def(
            "no JoinCoverage report was returned",
            &[(RESULT_KEY, "Value")],
            |ctx, _params| {
                let value = result(&ctx, RESULT_KEY)?;
                if value.get("per_hearth").is_some() {
                    Err(format!("A report was returned: {}", value))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the two JoinCoverage responses are identical",
            &[(RESULT_KEY, "Value"), (RESULT_2_KEY, "Value")],
            |ctx, _params| {
                let first = result(&ctx, RESULT_KEY)?;
                let second = result(&ctx, RESULT_2_KEY)?;
                if first == second {
                    Ok(())
                } else {
                    Err(format!("Responses differ:\n  {}\n  {}", first, second))
                }
            },
        ),
        check_def(
            "no telemetry salt file exists in the {string} hearth",
            &[HEARTH_A, (HEARTH_B_KEY, "PathBuf")],
            |ctx, params| {
                let which = params.get_string(0).unwrap_or_default().to_string();
                let path = hearth_of(&ctx, &which)?.join(SALT_FILENAME);
                if path.exists() {
                    Err(format!(
                        "A read path wrote {} — the generate-and-persist resolver was called",
                        path.display()
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}

/// A second, non-canonical spelling of the same directory. Canonicalizing
/// collapses it; comparing request strings does not.
fn other_spelling(hearth: &Path) -> Result<String, String> {
    let parent = hearth.parent().ok_or("hearth has no parent")?;
    let name = hearth.file_name().ok_or("hearth has no basename")?;
    Ok(as_str(&parent.join("..").join(
        parent.file_name().ok_or("parent has no basename")?,
    ))
    .to_string()
        + "/"
        + &name.to_string_lossy())
}
