//! Real-binary steps for `anvil_hooks_complete_claimed_evidence.feature`
//! (T-ACT-2, phases P0/P2/P4 — CLI leg).
//!
//! Drives the REAL `anvil-hooks complete` binary against a running engine over
//! a `track_lifecycle` hearth that carries NO `playbooks/` directory, so the
//! engine resolves the machine via `SeedPlaybookRegistry` fallback and assesses
//! against T-ACT-1's live `spec`/doer → `[artifact_of_consequence]` and
//! `plan`/doer → `[verifiable_citation, artifact_of_consequence]` obligations.
//! The `--claimed-evidence <class>:<reference>` affordance is the only lever the
//! caller pulls; the emitted row on `<hearth>/step-measurement.jsonl` is read
//! back with deadline polling (the durable write is dispatched asynchronously).

use anvil_test_support::engine::EngineProcess;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use anvil_core::domain::playbook::seeds::track_seed;
use anvil_core::domain::playbook_version::machine_content_version;
use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const TRACK_ID: &str = "20260721T2000_claimed_evidence_cli";
const ARTIFACT_PATH: &str = "tracks/20260721T2000_claimed_evidence_cli";
const PARENT_PROPOSAL: &str = "proposals/20260411T2021_anvil_workflow_engine";
const ACTOR: &str = "Doer-CE-100000";

#[derive(Clone)]
struct CeOutcome {
    exit: i32,
    output: String,
    rows: Vec<Value>,
    resolved_state: String,
}

fn bin() -> PathBuf {
    anvil_test_support::harness::ensure_binary("anvil-hooks");
    anvil_test_support::harness::binary_path("anvil-hooks")
}

/// Build a minimal `track_lifecycle` hearth with the track in `state` and NO
/// `playbooks/` directory (so the seed machine — carrying T-ACT-1's obligations
/// — resolves via fallback). Generous registry/projection sections keep every
/// exercised transition's bookkeeping write from erroring.
fn seed_track_hearth(hearth: &Path, state: &str) -> Result<(), String> {
    std::fs::create_dir_all(hearth.join("tracks"))
        .map_err(|e| format!("create tracks/: {}", e))?;
    // Parent proposal (active) — the track's strategic parent.
    let proposal_dir = hearth.join(PARENT_PROPOSAL);
    std::fs::create_dir_all(&proposal_dir)
        .map_err(|e| format!("create proposal dir: {}", e))?;
    std::fs::write(
        proposal_dir.join("status.yaml"),
        "version: 1\nkind: proposal\nstate: active\ntransitions:\n",
    )
    .map_err(|e| format!("write proposal status: {}", e))?;
    std::fs::write(proposal_dir.join("proposal.md"), "# Parent proposal\n")
        .map_err(|e| format!("write proposal.md: {}", e))?;

    // The track artifact in the requested state, with a matching transition
    // history line so the fold resolves the current state cleanly.
    let track_dir = hearth.join(ARTIFACT_PATH);
    std::fs::create_dir_all(&track_dir).map_err(|e| format!("create track dir: {}", e))?;
    std::fs::write(
        track_dir.join("status.yaml"),
        format!(
            "version: 1\nkind: track\nstate: {state}\nproposal: 20260411T2021_anvil_workflow_engine\ntransitions:\n  - to: {state}\n    at: \"2026-07-21T20:00:00Z\"\n    actor: Seed-CE-000001\n    role: doer\n",
            state = state,
        ),
    )
    .map_err(|e| format!("write track status: {}", e))?;
    std::fs::write(track_dir.join("spec.md"), "# Claimed evidence CLI track\n")
        .map_err(|e| format!("write spec.md: {}", e))?;
    std::fs::write(
        track_dir.join("spec.review.md"),
        "# Spec Review\n\n- [x] Reviewer satisfied\n",
    )
    .map_err(|e| format!("write spec.review.md: {}", e))?;
    std::fs::write(track_dir.join("plan.md"), "# Plan\n")
        .map_err(|e| format!("write plan.md: {}", e))?;

    // Registry with every section the exercised transitions project into.
    std::fs::write(
        hearth.join("tracks.md"),
        "# Tracks\n\n## spec\n\n- [Claimed evidence CLI track](tracks/20260721T2000_claimed_evidence_cli/) — claimed evidence CLI — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## spec_revision\n\n## plan\n\n## plan_review\n\n## implementing\n\n## completed\n",
    )
    .map_err(|e| format!("write tracks.md: {}", e))?;

    // Execution projection with the same generous header set.
    let proj = hearth.join("projections");
    std::fs::create_dir_all(&proj).map_err(|e| format!("create projections/: {}", e))?;
    std::fs::write(
        proj.join("execution.md"),
        "---\nincremental_count: 0\nbase_snapshot: 2026-07-21T20:00:00Z\nlast_updated: 2026-07-21T20:00:00Z\nafter_event: \"\"\n---\n\n# Anvil — State of Execution\n\n## Spec (1)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Plan Review (0)\n\n## Implementing (0)\n",
    )
    .map_err(|e| format!("write execution.md: {}", e))?;
    Ok(())
}

/// Run `anvil-hooks complete` against the started engine, poll the durable
/// evidence sink, fold the resulting artifact state, and store the outcome.
fn run_complete(
    mut ctx: Context,
    extra_args: &[String],
) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let port = engine.port;
    let hearth = ctx.get::<PathBuf>("hearth_path").cloned().ok_or("No hearth_path")?;

    let mut cmd = Command::new(bin());
    cmd.arg("complete")
        .arg("--artifact-path")
        .arg(ARTIFACT_PATH)
        .arg("--actor-name")
        .arg(ACTOR)
        .arg("--actor-type")
        .arg("agent")
        .arg("--actor-model")
        .arg("test-model")
        .arg("--actor-provider")
        .arg("test-provider")
        .arg("--hearth")
        .arg(hearth.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string());
    for a in extra_args {
        cmd.arg(a);
    }
    let output = cmd.output().map_err(|e| format!("run complete: {}", e))?;
    let exit = output.status.code().unwrap_or(-1);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rows = poll_sink(&hearth);
    let resolved_state = FileSystemSnapshotAdapter::new(hearth.clone())
        .read_artifact_state(ARTIFACT_PATH)
        .unwrap_or_default();

    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    out.set(
        "ce_outcome",
        CeOutcome {
            exit,
            output: combined,
            rows,
            resolved_state,
        },
    );
    Ok(out)
}

/// Deadline-poll `<hearth>/step-measurement.jsonl` for at least one parsed row.
fn poll_sink(hearth: &Path) -> Vec<Value> {
    let path = hearth.join("step-measurement.jsonl");
    for _ in 0..150 {
        if let Ok(bytes) = std::fs::read(&path) {
            if !bytes.is_empty() {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let rows: Vec<Value> = text
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                        .collect();
                    if !rows.is_empty() {
                        return rows;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Vec::new()
}

fn outcome(ctx: &Context) -> Result<&CeOutcome, String> {
    ctx.get::<CeOutcome>("ce_outcome")
        .ok_or_else(|| "No ce_outcome".to_string())
}

fn evidence_rows(o: &CeOutcome) -> Vec<&Value> {
    o.rows
        .iter()
        .filter(|r| r.get("evidence_status").is_some())
        .collect()
}

fn one_evidence_row(o: &CeOutcome) -> Result<&Value, String> {
    let rows = evidence_rows(o);
    if rows.len() == 1 {
        Ok(rows[0])
    } else {
        Err(format!(
            "expected exactly one evidence row, got {} (all rows: {})",
            rows.len(),
            render(&o.rows)
        ))
    }
}

fn render(rows: &[Value]) -> String {
    rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")
}

fn parse_pairs(params: &Params) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if let Some(table) = params.data_table() {
        // Header row is the first data pair for these two-column class|reference
        // tables (matching the existing complete-tools/call convention).
        if table.headers.len() >= 2
            && table.headers[0].trim() != "class"
        {
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
    }
    pairs
}

fn csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a track_lifecycle hearth with a track in state {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected track state")?;
                let (handle, hearth) = retained_temp_dir("anvil-ce-cli-")?;
                seed_track_hearth(&hearth, &state)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        // ---- When: doer completes presenting claims from a class|reference table ----
        step_def(
            "anvil-hooks completes the track as doer presenting claims:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("ce_outcome", "CeOutcome"),
            ],
            |ctx, params| {
                let mut args = Vec::new();
                for (class, reference) in parse_pairs(&params) {
                    args.push("--claimed-evidence".to_string());
                    args.push(format!("{}:{}", class, reference));
                }
                run_complete(ctx, &args)
            },
        ),
        step_def(
            "anvil-hooks completes the track as doer presenting no claims",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("ce_outcome", "CeOutcome"),
            ],
            |ctx, _params| run_complete(ctx, &[]),
        ),
        step_def(
            "anvil-hooks completes the track as doer with note {string} presenting claims:",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("ce_outcome", "CeOutcome"),
            ],
            |ctx, params| {
                let note = params.get_string(0).ok_or("Expected note")?;
                let mut args = vec!["--note".to_string(), note.to_string()];
                for (class, reference) in parse_pairs(&params) {
                    args.push("--claimed-evidence".to_string());
                    args.push(format!("{}:{}", class, reference));
                }
                run_complete(ctx, &args)
            },
        ),
        step_def(
            "anvil-hooks completes the track as reviewer satisfied presenting no claims",
            &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("ce_outcome", "CeOutcome"),
            ],
            |ctx, _params| {
                run_complete(ctx, &["--satisfaction".to_string(), "satisfied".to_string()])
            },
        ),
        // ---- Then ----
        check_def(
            "the anvil-hooks complete command exits 0",
            &[("ce_outcome", "CeOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.exit == 0 {
                    Ok(())
                } else {
                    Err(format!("expected exit 0, got {} — output: {}", o.exit, o.output))
                }
            },
        ),
        check_def(
            "the emitted evidence row records status {string}",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected status")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual = row.get("evidence_status").and_then(Value::as_str).unwrap_or("");
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected status '{}', got '{}' in {}", expected, actual, row))
                }
            },
        ),
        check_def(
            "the emitted evidence row lists claims in order:",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let expected = parse_pairs(&params);
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let claims = row
                    .get("claimed_evidence")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("no claimed_evidence array in {}", row))?;
                if claims.len() != expected.len() {
                    return Err(format!(
                        "expected {} claims, got {} in {}",
                        expected.len(),
                        claims.len(),
                        row
                    ));
                }
                for (i, (class, reference)) in expected.iter().enumerate() {
                    let ac = claims[i].get("class").and_then(Value::as_str).unwrap_or("");
                    let ar = claims[i].get("reference").and_then(Value::as_str).unwrap_or("");
                    if ac != class || ar != reference {
                        return Err(format!(
                            "claim[{}] expected {}:{}, got {}:{}",
                            i, class, reference, ac, ar
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the emitted evidence row names missing classes {string}",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let expected = csv(&params.get_string(0).ok_or("Expected missing classes")?);
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual: Vec<String> = row
                    .get("missing_evidence_classes")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("no missing_evidence_classes in {}", row))?
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect();
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected missing {:?}, got {:?} in {}", expected, actual, row))
                }
            },
        ),
        check_def(
            "the emitted evidence row carries the track_lifecycle seed content version",
            &[("ce_outcome", "CeOutcome")],
            |ctx, _params| {
                let expected = machine_content_version(track_seed())
                    .ok_or("track seed had no content version")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let actual = row.get("playbook_version").and_then(Value::as_str).unwrap_or("");
                if actual.is_empty() {
                    return Err(format!("empty playbook_version in {}", row));
                }
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected seed version '{}', got '{}' in {}",
                        expected, actual, row
                    ))
                }
            },
        ),
        check_def(
            "no emitted step measurement row carries any evidence key",
            &[("ce_outcome", "CeOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                let keys = [
                    "evidence_status",
                    "missing_evidence_classes",
                    "claimed_evidence",
                    "playbook_version",
                ];
                let offending: Vec<&Value> = o
                    .rows
                    .iter()
                    .filter(|r| keys.iter().any(|k| r.get(*k).is_some()))
                    .collect();
                if offending.is_empty() {
                    Ok(())
                } else {
                    Err(format!("rows carry evidence keys: {}", render(&o.rows)))
                }
            },
        ),
        check_def(
            "the emitted evidence row reference contains {string}",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let o = outcome(&ctx)?;
                let row = one_evidence_row(o)?;
                let found = row
                    .get("claimed_evidence")
                    .and_then(Value::as_array)
                    .map(|claims| {
                        claims.iter().any(|c| {
                            c.get("reference").and_then(Value::as_str) == Some(needle.as_ref())
                        })
                    })
                    .unwrap_or(false);
                if found {
                    Ok(())
                } else {
                    Err(format!("no claim reference equal to '{}' in {}", needle, row))
                }
            },
        ),
        check_def(
            "no emitted step measurement row contains the text {string}",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?;
                let o = outcome(&ctx)?;
                let raw = render(&o.rows);
                if raw.contains(needle.as_ref() as &str) {
                    Err(format!("raw text '{}' leaked into a measurement row: {}", needle, raw))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the track resolved state is {string}",
            &[("ce_outcome", "CeOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let o = outcome(&ctx)?;
                if o.resolved_state == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected resolved state '{}', got '{}'",
                        expected, o.resolved_state
                    ))
                }
            },
        ),
    ]
}
