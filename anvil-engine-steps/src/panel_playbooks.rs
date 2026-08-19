//! Steps for `anvil-engine/features/panel_playbooks.feature`.
//!
//! Drives the REAL engine over a REAL loopback socket and reads the REAL
//! `GET /panel/playbooks` route. The hearth, the playbooks and the recorded
//! calls are all real: the scenarios reuse `ws_bridge.rs`'s own
//! `a playbook activity engine hearth with playbooks:` and
//! `the engine is started with that hearth`, so the fixture that feeds this
//! column is the same one that feeds the `/ws` methods and the gRPC RPCs. Only
//! the reading is new.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, step_def, StepDef};
use serde_json::Value;

use anvil_core::ports::activity_log_port::{ActivityLogRecord, ActivityLogWritePort};
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_test_support::engine::EngineProcess;

const PANEL_STATUS_KEY: &str = "panel_playbooks_status";
const PANEL_BODY_KEY: &str = "panel_playbooks_body";

/// Raw HTTP/1.1 GET returning `(status, body)`. Same shape as
/// `ws_bridge::http_get_status`, which returns only the status — a panel row
/// cannot be asserted from a status code, so this one keeps the body.
async fn http_get(port: u16, path: &str) -> Result<(u16, String), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("connect to engine port {port} failed: {e}"))?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("write HTTP request failed: {e}"))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("read HTTP response failed: {e}"))?;
    let text = String::from_utf8_lossy(&buf).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("no status line in response: {}", &text[..text.len().min(120)]))?;
    // The body is everything after the blank line that ends the headers.
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

fn document(ctx: &Context) -> Result<Value, String> {
    let raw: String = ctx
        .get::<String>(PANEL_BODY_KEY)
        .cloned()
        .ok_or("no /panel/playbooks response in this scenario")?;
    serde_json::from_str(&raw).map_err(|e| format!("the body is not JSON ({e}): {raw}"))
}

fn rows(ctx: &Context) -> Result<Vec<Value>, String> {
    let doc = document(&ctx)?;
    doc.get("playbooks")
        .and_then(|p| p.as_array())
        .cloned()
        .ok_or_else(|| {
            format!(
                "the document carries no `playbooks` ARRAY. The host refuses a body whose array is \
                 missing rather than publishing it, so the column would stay blank with the host \
                 blaming the kit: {doc}"
            )
        })
}

/// One row by TITLE — the thing a reader sees. Looking it up by id would let a
/// scenario pass while the column rendered a name nobody recognises, which is
/// exactly what the reverted route did.
fn row_titled(ctx: &Context, title: &str) -> Result<Value, String> {
    let all = rows(&ctx)?;
    all.iter()
        .find(|r| r.get("title").and_then(|t| t.as_str()) == Some(title))
        .cloned()
        .ok_or_else(|| {
            let titles: Vec<&str> = all
                .iter()
                .filter_map(|r| r.get("title").and_then(|t| t.as_str()))
                .collect();
            format!("no playbook titled {title:?}; the column carries {titles:?}")
        })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "an HTTP GET /panel/playbooks is sent to the engine port",
            &[("engine_process", "EngineProcess")],
            &[
                ("engine_process", "EngineProcess"),
                (PANEL_STATUS_KEY, "u16"),
                (PANEL_BODY_KEY, "String"),
            ],
            |mut ctx, _params| async move {
                let engine = ctx
                    .take::<EngineProcess>("engine_process")
                    .ok_or("No engine_process")?;
                let (status, body) = http_get(engine.port, "/panel/playbooks").await?;
                let mut out = Context::new();
                out.set("engine_process", engine);
                out.set::<u16>(PANEL_STATUS_KEY, status);
                out.set::<String>(PANEL_BODY_KEY, body);
                Ok(out)
            },
        ),
        // The fixture's `owner` column writes `contributed_by:`, which feeds the
        // ACTIVITY roll-up. The atlas reads a DIFFERENT field — status.yaml's
        // `owner_kit:` — and that is the one the column's grouping is derived
        // from. My first draft asserted "Yours" against a fixture that never
        // wrote owner_kit, so every row came back "Shared with you" and the
        // scenario was asserting something the fixture could not produce. This
        // step writes the field the atlas actually reads.
        step_def(
            "the playbook {string} declares owner_kit {string}",
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("no playbook kind")?;
                let owner = params.get_string(1).ok_or("no owner_kit")?;
                let hearth = ctx
                    .get::<std::path::PathBuf>("hearth_path")
                    .ok_or("No hearth_path")?
                    .clone();
                let status = hearth
                    .join("playbooks")
                    .join(format!("{kind}_dir"))
                    .join("status.yaml");
                let existing = std::fs::read_to_string(&status)
                    .map_err(|e| format!("read {}: {e}", status.display()))?;
                if existing.contains("owner_kit:") {
                    return Err(format!("{} already declares owner_kit", status.display()));
                }
                std::fs::write(&status, format!("{existing}owner_kit: {owner}\n"))
                    .map_err(|e| format!("write {}: {e}", status.display()))?;
                Ok(ctx)
            },
        ),
        // The cross-hearth call. `PlaybookRegistryView.tsx` reads activity with
        // `all_hearths: true` and live instances with `false`, and the asymmetry
        // is load-bearing: a call recorded against ANOTHER hearth still means the
        // playbook HAS run. Without this step the route could scope activity to
        // one hearth and every scenario would still pass, which is exactly the
        // defect that shipped — 5 of anvil's 32 production rows derived `hollow`
        // that the frontend derives as having run.
        step_def(
            "an activity log record for kind {string} is appended to the SECOND sub-hearth",
            &[("wa_parent_root", "PathBuf")],
            // Passes the hearth slots THROUGH: the engine-start step that
            // follows requires them, and a step that provides only what it
            // touched drops the rest of the scene on the floor.
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("wa_parent_root", "PathBuf"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("no kind")?.to_string();
                let parent = ctx
                    .get::<std::path::PathBuf>("wa_parent_root")
                    .ok_or("No wa_parent_root — this step needs the two-sub-hearth fixture")?
                    .clone();
                let beta = parent.join("beta-hearth");
                FileSystemActivityLogAdapter::new(&beta)
                    .append_activity_log(&ActivityLogRecord {
                        command: "route".to_string(),
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
                    .map_err(|e| format!("append activity to beta hearth: {e:?}"))?;
                Ok(ctx)
            },
        ),
        check_def(
            "the /panel/playbooks response status is {int}",
            &[(PANEL_STATUS_KEY, "u16")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("no expected status")? as u16;
                let got = ctx.get::<u16>(PANEL_STATUS_KEY).copied().unwrap_or(0);
                if got == want {
                    Ok(())
                } else {
                    Err(format!("status was {got}, expected {want}"))
                }
            },
        ),
        check_def(
            "the playbooks document declares version {int}",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("no expected version")? as u64;
                let doc = document(&ctx)?;
                match doc.get("version").and_then(|v| v.as_u64()) {
                    Some(v) if v == want => Ok(()),
                    other => Err(format!(
                        "the document declares version {other:?}; the host reads version {want} and \
                         a document that does not say so is one it cannot place"
                    )),
                }
            },
        ),
        check_def(
            "the playbooks document carries {int} playbooks",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let want = params.get_int(0).ok_or("no expected count")? as usize;
                let all = rows(&ctx)?;
                if all.len() == want {
                    Ok(())
                } else {
                    Err(format!("expected {want} playbooks, the document carries {}", all.len()))
                }
            },
        ),
        check_def(
            "the playbook {string} is in state {string}",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let title = params.get_string(0).ok_or("no playbook title")?;
                let want = params.get_string(1).ok_or("no expected state")?;
                let row = row_titled(&ctx, title)?;
                let got = row.get("state").and_then(|s| s.as_str()).unwrap_or("");
                if got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "{title:?} is in state {got:?}, expected {want:?}. These are different facts \
                         with different fixes — a definition that CANNOT be read is not a playbook \
                         nobody has called."
                    ))
                }
            },
        ),
        check_def(
            "the playbook {string} is grouped under {string}",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let title = params.get_string(0).ok_or("no playbook title")?;
                let want = params.get_string(1).ok_or("no expected group")?;
                let row = row_titled(&ctx, title)?;
                let got = row.get("group").and_then(|g| g.as_str()).unwrap_or("");
                if got == want {
                    Ok(())
                } else {
                    Err(format!("{title:?} is grouped under {got:?}, expected {want:?}"))
                }
            },
        ),
        check_def(
            "the playbook {string} carries the meta {string}",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let title = params.get_string(0).ok_or("no playbook title")?;
                let want = params.get_string(1).ok_or("no expected meta")?;
                let row = row_titled(&ctx, title)?;
                let got = row.get("meta").and_then(|m| m.as_str()).unwrap_or("");
                if got == want {
                    Ok(())
                } else {
                    Err(format!("{title:?} carries meta {got:?}, expected {want:?}"))
                }
            },
        ),
        check_def(
            "the playbook {string} carries no meta",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let title = params.get_string(0).ok_or("no playbook title")?;
                let row = row_titled(&ctx, title)?;
                match row.get("meta").and_then(|m| m.as_str()) {
                    None => Ok(()),
                    Some(m) => Err(format!(
                        "{title:?} carries the meta {m:?}. The engine records THAT a playbook ran, \
                         never whether the run was clean, so a phrase here would be an invented \
                         outcome."
                    )),
                }
            },
        ),
        // The two guards the reverted route would have failed. They are written
        // over EVERY row rather than a named one, because the defect was not
        // specific to a playbook — it was the slot being fed the wrong field.
        check_def(
            "no playbook title contains an underscore",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, _params| {
                for row in rows(&ctx)? {
                    let t = row.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    if t.contains('_') {
                        return Err(format!(
                            "the title {t:?} is a raw engine identifier. Engine state names and \
                             lifecycle enums appear nowhere on a customer surface."
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "no playbook title is longer than {int} characters",
            &[(PANEL_BODY_KEY, "String")],
            |ctx, params| {
                let max = params.get_int(0).ok_or("no maximum")? as usize;
                for row in rows(&ctx)? {
                    let t = row.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    if t.chars().count() > max {
                        return Err(format!(
                            "a title is {} characters long, which is a DESCRIPTION in the title \
                             slot — the exact defect that got the first version of this route \
                             reverted: {:?}…",
                            t.chars().count(),
                            t.chars().take(60).collect::<String>()
                        ));
                    }
                }
                Ok(())
            },
        ),
    ]
}
