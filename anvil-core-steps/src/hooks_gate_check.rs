//! Step module for `hooks_gate_check.feature` (core seam).
//!
//! Drives the pure [`gate_check::decide`] function over an in-context fixture:
//! an optional resolved artifact (kind + state), the hard_enforce policy, and the
//! begin-status tri-state (open / closed / errored). No filesystem, no engine —
//! the decision is pure, so the runtime stdin/exit/engine plumbing is tested
//! separately in the engine seam.
//!
//! Each Given step declares the FULL carried key-set in its `provides` (not just
//! the key it sets) because the harness tracks available keys from declared
//! `provides`, and every step in the chain re-emits the whole fixture via `carry`.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core::domain::hooks::gate_check::{
    decide, decide_with_policy, gate_check, resolve_artifact, FailurePolicy, ResolvedArtifact,
    Verdict,
};
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use anvil_core_hearth::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::transition_event_write_port::{TransitionEventWritePort, TransitionRecord};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const KIND_KEY: &str = "gc_kind";
const STATE_KEY: &str = "gc_state";
const NO_ARTIFACT_KEY: &str = "gc_no_artifact";
const HARD_KEY: &str = "gc_hard_enforce";
// Begin status: "open" | "closed" | "error" — maps to Some(true)/Some(false)/None.
const BEGIN_KEY: &str = "gc_begin";
const VERDICT_KEY: &str = "gc_verdict";
const FS_HEARTH_KEY: &str = "gc_fs_hearth";
const FS_HEARTH_HANDLE_KEY: &str = "gc_fs_hearth_handle";
const FS_EDITED_PATH_KEY: &str = "gc_fs_edited_path";
const FS_RESOLVED_STATE_KEY: &str = "gc_fs_resolved_state";
const FS_TRACK_REL: &str = "tracks/20260702T2040_gate_folded_state";
const PROCESS_PAYLOAD_KEY: &str = "gc_process_payload";
const PROCESS_STATUS_KEY: &str = "gc_process_status";
const PROCESS_STDERR_KEY: &str = "gc_process_stderr";
const PROCESS_DEADLINE_MS_KEY: &str = "gc_process_deadline_ms";
const PROCESS_DELAY_MS_KEY: &str = "gc_process_delay_ms";
const PROCESS_INCLUDE_HEARTH_KEY: &str = "gc_process_include_hearth";

/// Every fixture key each Given step carries forward — declared as `provides` on
/// each so the harness's available-key contract is satisfied regardless of order.
const FIXTURE_PROVIDES: &[(&str, &str)] = &[
    (KIND_KEY, "String"),
    (STATE_KEY, "String"),
    (NO_ARTIFACT_KEY, "bool"),
    (HARD_KEY, "Vec<String>"),
    (BEGIN_KEY, "String"),
    (FS_HEARTH_KEY, "PathBuf"),
    (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
    (FS_EDITED_PATH_KEY, "PathBuf"),
    (PROCESS_PAYLOAD_KEY, "String"),
];

fn carry(ctx: &mut Context, out: &mut Context) {
    if let Some(v) = ctx.take::<String>(KIND_KEY) {
        out.set(KIND_KEY, v);
    }
    if let Some(v) = ctx.take::<String>(STATE_KEY) {
        out.set(STATE_KEY, v);
    }
    if let Some(v) = ctx.take::<bool>(NO_ARTIFACT_KEY) {
        out.set(NO_ARTIFACT_KEY, v);
    }
    if let Some(v) = ctx.take::<Vec<String>>(HARD_KEY) {
        out.set(HARD_KEY, v);
    }
    if let Some(v) = ctx.take::<String>(BEGIN_KEY) {
        out.set(BEGIN_KEY, v);
    }
    if let Some(v) = ctx.take::<PathBuf>(FS_HEARTH_KEY) {
        out.set(FS_HEARTH_KEY, v);
    }
    if let Some(v) = ctx.take::<RetainedTempDir>(FS_HEARTH_HANDLE_KEY) {
        out.set(FS_HEARTH_HANDLE_KEY, v);
    }
    if let Some(v) = ctx.take::<PathBuf>(FS_EDITED_PATH_KEY) {
        out.set(FS_EDITED_PATH_KEY, v);
    }
    if let Some(v) = ctx.take::<String>(PROCESS_PAYLOAD_KEY) {
        out.set(PROCESS_PAYLOAD_KEY, v);
    }
}

/// Seed a `tracks/…` artifact under a fresh temp hearth with the given top-level
/// `state:` and a caller-supplied `activity:` block, plus an editable `spec.md`.
/// Returns `(handle, hearth_root, edited_spec_path)`.
#[allow(clippy::type_complexity)]
fn seed_gate_track(
    state: &str,
    activity_yaml: &str,
) -> Result<(RetainedTempDir, PathBuf, PathBuf), String> {
    let (handle, hearth) = retained_temp_dir("anvil-gate-check-")?;
    let track_dir = hearth.join(FS_TRACK_REL);
    std::fs::create_dir_all(&track_dir).map_err(|e| format!("create track dir: {}", e))?;
    let status = format!("version: 1\nkind: track\nstate: {state}\nactors: {{}}\n{activity_yaml}");
    std::fs::write(track_dir.join("status.yaml"), status)
        .map_err(|e| format!("write status.yaml: {}", e))?;
    let edited_path = track_dir.join("spec.md");
    std::fs::write(&edited_path, "# Gate Degraded Log\n\nBody.\n")
        .map_err(|e| format!("write spec.md: {}", e))?;
    Ok((handle, hearth, edited_path))
}

fn carry_fs(ctx: &Context, out: &mut Context) {
    if let Some(v) = ctx.get::<PathBuf>(FS_HEARTH_KEY) {
        out.set(FS_HEARTH_KEY, v.clone());
    }
    if let Some(v) = ctx.get::<PathBuf>(FS_EDITED_PATH_KEY) {
        out.set(FS_EDITED_PATH_KEY, v.clone());
    }
    carry_retained_temp_dir(ctx, out, FS_HEARTH_HANDLE_KEY);
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a gate-check for an artifact of kind {string} in state {string}",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(KIND_KEY, kind);
                out.set(STATE_KEY, state);
                Ok(out)
            },
        ),
        step_def(
            "a gate-check for a path with no enclosing forge artifact",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, _params| {
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(NO_ARTIFACT_KEY, true);
                Ok(out)
            },
        ),
        step_def(
            "a gate-check that errored resolving the begin status",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, _params| {
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(BEGIN_KEY, "error".to_string());
                Ok(out)
            },
        ),
        step_def(
            "a Codex gate-check with a malformed governed mutation target",
            &[],
            FIXTURE_PROVIDES,
            |_ctx, _| {
                let mut out = Context::new();
                out.set(KIND_KEY, "track".to_string());
                out.set(STATE_KEY, "spec".to_string());
                out.set(HARD_KEY, vec!["track".to_string()]);
                out.set(BEGIN_KEY, "error".to_string());
                Ok(out)
            },
        ),
        step_def(
            "a Codex gate-check that timed out resolving a governed mutation",
            &[],
            FIXTURE_PROVIDES,
            |_ctx, _| {
                let mut out = Context::new();
                out.set(KIND_KEY, "track".to_string());
                out.set(STATE_KEY, "spec".to_string());
                out.set(HARD_KEY, vec!["track".to_string()]);
                out.set(BEGIN_KEY, "error".to_string());
                Ok(out)
            },
        ),
        step_def(
            "the artifact is of kind {string} in state {string}",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(KIND_KEY, kind);
                out.set(STATE_KEY, state);
                Ok(out)
            },
        ),
        step_def(
            "the hard_enforce policy includes {string}",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut hard: Vec<String> = ctx.take::<Vec<String>>(HARD_KEY).unwrap_or_default();
                hard.push(kind);
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(HARD_KEY, hard);
                Ok(out)
            },
        ),
        step_def(
            "there is no open begin session",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, _params| {
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(BEGIN_KEY, "closed".to_string());
                Ok(out)
            },
        ),
        step_def(
            "there is an open begin session",
            &[],
            FIXTURE_PROVIDES,
            |mut ctx, _params| {
                let mut out = Context::new();
                carry(&mut ctx, &mut out);
                out.set(BEGIN_KEY, "open".to_string());
                Ok(out)
            },
        ),
        step_def(
            "a filesystem gate-check track begun in {string} after seed state {string}",
            &[],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let begin_state = params.get_string(0).ok_or("Expected begin state")?;
                let seed_state = params.get_string(1).ok_or("Expected seed state")?;
                let (handle, hearth) = retained_temp_dir("anvil-gate-check-")?;
                let track_dir = hearth.join(FS_TRACK_REL);
                std::fs::create_dir_all(&track_dir)
                    .map_err(|e| format!("create track dir: {}", e))?;
                let status = format!(
                    "version: 1\nkind: track\nstate: {seed_state}\nactors: {{}}\nactivity:\n  - kind: begin\n    actor: Reviewer-500010\n    state: {begin_state}\n    at: 2026-07-02T20:40:00Z\n    conversation_id: conv-gate-folded\n",
                    seed_state = seed_state,
                    begin_state = begin_state,
                );
                std::fs::write(track_dir.join("status.yaml"), status)
                    .map_err(|e| format!("write status.yaml: {}", e))?;
                let edited_path = track_dir.join("spec.md");
                std::fs::write(&edited_path, "# Gate Folded State\n\nBody.\n")
                    .map_err(|e| format!("write spec.md: {}", e))?;

                let mut out = Context::new();
                out.set(FS_HEARTH_KEY, hearth);
                out.set(FS_HEARTH_HANDLE_KEY, handle);
                out.set(FS_EDITED_PATH_KEY, edited_path);
                Ok(out)
            },
        ),
        // Seed a hard-enforced track whose activity: log has NO surviving open
        // begin for the acting actor, but IS degraded (one entry dropped as
        // malformed). The conservative gate must fail OPEN (ALLOW) rather than
        // BLOCK — a dropped entry may have been the actor's open begin, and
        // blocking would nudge a DUPLICATE begin.
        step_def(
            "a filesystem gate-check hard-enforced track in state {string} whose only activity entry is malformed",
            &[],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                // One hand-authored begin marker MISSING the required `state:`
                // field → dropped per-entry → the log is degraded with zero
                // surviving entries (no open begin for anyone).
                let activity = "activity:\n  - kind: begin\n    actor: Reviewer-500010\n    at: 2026-07-02T20:40:00Z\n    note: hand authored, no state\n";
                let (handle, hearth, edited) = seed_gate_track(state, activity)?;
                let mut out = Context::new();
                out.set(FS_HEARTH_KEY, hearth);
                out.set(FS_HEARTH_HANDLE_KEY, handle);
                out.set(FS_EDITED_PATH_KEY, edited);
                Ok(out)
            },
        ),
        // Contrast: a CLEAN empty activity log (no markers, nothing dropped).
        // The gate is confident there is no open begin → BLOCK. This is what
        // proves the ALLOW above is caused by DEGRADATION, not mere absence.
        step_def(
            "a filesystem gate-check hard-enforced track in state {string} with a clean empty activity log",
            &[],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected state")?;
                let activity = "activity: []\n";
                let (handle, hearth, edited) = seed_gate_track(state, activity)?;
                let mut out = Context::new();
                out.set(FS_HEARTH_KEY, hearth);
                out.set(FS_HEARTH_HANDLE_KEY, handle);
                out.set(FS_EDITED_PATH_KEY, edited);
                Ok(out)
            },
        ),
        step_def(
            "the filesystem track has a transition event to {string}",
            &[(FS_HEARTH_KEY, "PathBuf")],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
            ],
            |ctx, params| {
                let to = params.get_string(0).ok_or("Expected to state")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(FS_HEARTH_KEY)
                    .ok_or("No fs hearth")?
                    .clone();
                let adapter = FileSystemTransitionEventAdapter::new(hearth.clone());
                let record = TransitionRecord {
                    to,
                    at: "2026-07-02T20:41:00Z".to_string(),
                    actor: "Author-500010".to_string(),
                    role: "reviewer".to_string(),
                    approver: None,
                    note: None,
                    satisfaction: None,
                    event_type: None,
                };
                adapter
                    .append_transition_event(FS_TRACK_REL, &record)
                    .map_err(|e| format!("append transition event: {}", e))?;
                let mut out = Context::new();
                carry_fs(&ctx, &mut out);
                Ok(out)
            },
        ),
        step_def(
            "the gate-check decision is computed",
            &[],
            &[(VERDICT_KEY, "String")],
            |mut ctx, _params| {
                let hard: Vec<String> = ctx.take::<Vec<String>>(HARD_KEY).unwrap_or_default();
                let no_artifact = ctx.take::<bool>(NO_ARTIFACT_KEY).unwrap_or(false);
                let kind = ctx.take::<String>(KIND_KEY);
                let state = ctx.take::<String>(STATE_KEY);
                let begin = ctx.take::<String>(BEGIN_KEY);

                let artifact = if no_artifact {
                    None
                } else {
                    match (kind, state) {
                        (Some(kind), Some(state)) => Some(ResolvedArtifact {
                            artifact_dir: PathBuf::from("/fixture/artifact"),
                            relative_path: "tracks/fixture".to_string(),
                            kind,
                            state,
                        }),
                        _ => None,
                    }
                };

                let begin_status = match begin.as_deref() {
                    Some("open") => Some(true),
                    Some("closed") => Some(false),
                    Some("error") => None,
                    // No begin step declared → treat as closed (no open begin).
                    _ => Some(false),
                };

                let verdict = decide(artifact.as_ref(), &hard, begin_status);
                let mut out = Context::new();
                out.set(VERDICT_KEY, verdict.as_str().to_string());
                Ok(out)
            },
        ),
        step_def(
            "the Codex gate-check decision is computed",
            &[],
            &[(VERDICT_KEY, "String")],
            |mut ctx, _| {
                let kind = ctx.take::<String>(KIND_KEY);
                let state = ctx.take::<String>(STATE_KEY);
                let hard = ctx.take::<Vec<String>>(HARD_KEY).unwrap_or_default();
                let begin = ctx.take::<String>(BEGIN_KEY);
                let artifact = kind.zip(state).map(|(kind, state)| ResolvedArtifact {
                    artifact_dir: PathBuf::from("/fixture/artifact"),
                    relative_path: "tracks/fixture".to_string(),
                    kind,
                    state,
                });
                let begin_status = match begin.as_deref() {
                    Some("open") => Some(true),
                    Some("closed") => Some(false),
                    _ => None,
                };
                let verdict = decide_with_policy(
                    artifact.as_ref(),
                    &hard,
                    begin_status,
                    FailurePolicy::CodexFailClosed,
                );
                let mut out = Context::new();
                out.set(VERDICT_KEY, verdict.as_str().to_string());
                Ok(out)
            },
        ),
        codex_process_fixture(
            "a Codex apply_patch mutation inside an Anvil-governed track with no open begin",
            "apply_patch",
            false,
            true,
        ),
        codex_deadline_fixture(),
        codex_malformed_resolver_fixture(),
        codex_process_fixture(
            "a Codex apply_patch mutation inside an Anvil-governed track with an open begin",
            "apply_patch",
            true,
            true,
        ),
        codex_process_fixture(
            "a Codex Bash mutation inside an Anvil-governed track with no open begin",
            "Bash",
            false,
            true,
        ),
        codex_bash_root_fixture(
            "a Codex Bash mutation launched from the hearth root targeting a governed track with no open begin",
            false,
            Some("touch tracks/20260702T2040_gate_folded_state/spec.md"),
        ),
        codex_bash_root_fixture(
            "a Codex Bash mutation launched from the hearth root targeting a governed track with an open begin",
            true,
            Some("touch tracks/20260702T2040_gate_folded_state/spec.md"),
        ),
        codex_bash_root_fixture(
            "a Codex Bash mutation launched from the hearth root without a trustworthy target",
            false,
            Some("touch"),
        ),
        codex_process_fixture(
            "a Codex Bash mutation outside an Anvil-governed artifact",
            "Bash",
            false,
            false,
        ),
        codex_process_fixture_options(
            "a Codex Bash mutation in a directory with no Anvil hearth",
            "Bash",
            false,
            false,
            false,
            None,
        ),
        codex_multi_target_fixture(
            "an outside-first Codex apply_patch mutation also targeting a governed track",
            "apply_patch",
        ),
        codex_multi_target_fixture(
            "an outside-first Codex Bash mutation also targeting a governed track",
            "Bash",
        ),
        codex_multi_target_fixture(
            "a Codex apply_patch move from an outside source into a governed track",
            "apply_patch_move",
        ),
        step_def(
            "the Codex gate-check process is executed",
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (PROCESS_PAYLOAD_KEY, "String"),
            ],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
                (PROCESS_PAYLOAD_KEY, "String"),
                (PROCESS_STATUS_KEY, "i32"),
                (PROCESS_STDERR_KEY, "String"),
            ],
            |ctx, _| {
                let workspace = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
                    .parent()
                    .ok_or("workspace root")?
                    .to_path_buf();
                let status = Command::new("cargo")
                    .args(["build", "-q", "-p", "anvil-engine", "--bin", "anvil-hooks"])
                    .current_dir(&workspace)
                    .status()
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("could not build anvil-hooks".to_string());
                }
                let hearth = ctx.get::<PathBuf>(FS_HEARTH_KEY).ok_or("missing hearth")?;
                let deadline = ctx.get::<u64>(PROCESS_DEADLINE_MS_KEY).copied();
                let delay = ctx.get::<u64>(PROCESS_DELAY_MS_KEY).copied();
                let mut args = vec![
                    "gate-check".to_string(),
                    "--source".to_string(),
                    "codex".to_string(),
                    "--hard-enforce".to_string(),
                    "track".to_string(),
                ];
                if ctx
                    .get::<bool>(PROCESS_INCLUDE_HEARTH_KEY)
                    .copied()
                    .unwrap_or(true)
                {
                    args.push("--hearth".to_string());
                    args.push(hearth.to_string_lossy().into_owned());
                }
                if let Some(deadline) = deadline {
                    args.push("--internal-deadline-ms".to_string());
                    args.push(deadline.to_string());
                }
                let mut command = Command::new(workspace.join("target/debug/anvil-hooks"));
                command.args(args);
                if let Some(delay) = delay {
                    command.env("ANVIL_HOOKS_TEST_RESOLVER_DELAY_MS", delay.to_string());
                }
                let mut child = command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                child
                    .stdin
                    .take()
                    .ok_or("missing stdin")?
                    .write_all(
                        ctx.get::<String>(PROCESS_PAYLOAD_KEY)
                            .ok_or("missing payload")?
                            .as_bytes(),
                    )
                    .map_err(|e| e.to_string())?;
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let code = output
                    .status
                    .code()
                    .unwrap_or(-1);
                let mut out = Context::new();
                carry_fs(&ctx, &mut out);
                out.set(PROCESS_PAYLOAD_KEY, ctx.get::<String>(PROCESS_PAYLOAD_KEY).unwrap().clone());
                out.set(PROCESS_STATUS_KEY, code);
                out.set(
                    PROCESS_STDERR_KEY,
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the gate-check process exits with the blocking status",
            &[(PROCESS_STATUS_KEY, "i32")],
            |ctx, _| {
                let code = *ctx.get::<i32>(PROCESS_STATUS_KEY).ok_or("missing status")?;
                (code == 2)
                    .then_some(())
                    .ok_or_else(|| format!("expected blocking exit 2, got {code}"))
            },
        ),
        check_def(
            "the gate-check process exits successfully",
            &[(PROCESS_STATUS_KEY, "i32")],
            |ctx, _| {
                let code = *ctx.get::<i32>(PROCESS_STATUS_KEY).ok_or("missing status")?;
                (code == 0)
                    .then_some(())
                    .ok_or_else(|| format!("expected exit 0, got {code}"))
            },
        ),
        check_def(
            "the gate-check process exits with the blocking status and a reason",
            &[(PROCESS_STATUS_KEY, "i32"), (PROCESS_STDERR_KEY, "String")],
            |ctx, _| {
                let code = *ctx.get::<i32>(PROCESS_STATUS_KEY).ok_or("missing status")?;
                let stderr = ctx
                    .get::<String>(PROCESS_STDERR_KEY)
                    .ok_or("missing stderr")?;
                (code == 2 && !stderr.trim().is_empty())
                    .then_some(())
                    .ok_or_else(|| {
                        format!("expected exit 2 with stderr, got {code}: {stderr:?}")
                })
            },
        ),
        check_def(
            "the gate-check process exits with the blocking status naming the governed target",
            &[
                (PROCESS_STATUS_KEY, "i32"),
                (PROCESS_STDERR_KEY, "String"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
            ],
            |ctx, _| {
                let code = *ctx.get::<i32>(PROCESS_STATUS_KEY).ok_or("missing status")?;
                let stderr = ctx
                    .get::<String>(PROCESS_STDERR_KEY)
                    .ok_or("missing stderr")?;
                let governed = ctx
                    .get::<PathBuf>(FS_EDITED_PATH_KEY)
                    .ok_or("missing governed target")?;
                (code == 2 && stderr.contains(&governed.to_string_lossy().into_owned()))
                    .then_some(())
                    .ok_or_else(|| {
                        format!(
                            "expected exit 2 naming governed target {}, got {code}: {stderr:?}",
                            governed.display()
                        )
                    })
            },
        ),
        step_def(
            "the filesystem gate-check decision is computed for actor {string}",
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
                (HARD_KEY, "Vec<String>"),
            ],
            &[
                (FS_HEARTH_KEY, "PathBuf"),
                (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (FS_EDITED_PATH_KEY, "PathBuf"),
                (FS_RESOLVED_STATE_KEY, "String"),
                (VERDICT_KEY, "String"),
            ],
            |ctx, params| {
                let actor = params.get_string(0).ok_or("Expected actor")?.to_string();
                let hearth = ctx
                    .get::<PathBuf>(FS_HEARTH_KEY)
                    .ok_or("No fs hearth")?
                    .clone();
                let edited_path = ctx
                    .get::<PathBuf>(FS_EDITED_PATH_KEY)
                    .ok_or("No edited path")?
                    .clone();
                let hard = ctx
                    .get::<Vec<String>>(HARD_KEY)
                    .cloned()
                    .unwrap_or_default();
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let resolved_state = resolve_artifact(&edited_path, &hearth)
                    .ok_or("gate-check did not resolve artifact")?
                    .state;
                let verdict = gate_check(&query, &edited_path, &hearth, &hard, Some(&actor));

                let mut out = Context::new();
                carry_fs(&ctx, &mut out);
                out.set(FS_RESOLVED_STATE_KEY, resolved_state);
                out.set(VERDICT_KEY, verdict.as_str().to_string());
                Ok(out)
            },
        ),
        check_def(
            "the gate-check verdict is {string}",
            &[(VERDICT_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected verdict")?;
                let got = ctx.get::<String>(VERDICT_KEY).ok_or("No verdict")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("verdict: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the filesystem gate-check resolved state is {string}",
            &[(FS_RESOLVED_STATE_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected state")?;
                let got = ctx
                    .get::<String>(FS_RESOLVED_STATE_KEY)
                    .ok_or("No resolved state")?;
                if got == want {
                    Ok(())
                } else {
                    Err(format!("resolved state: expected {}, got {}", want, got))
                }
            },
        ),
        check_def(
            "the gate-check verdict allow constant is wired",
            &[],
            |_ctx, _params| {
                if Verdict::Allow.as_str() == "allow" && Verdict::Block.as_str() == "block" {
                    Ok(())
                } else {
                    Err("verdict constants drifted".to_string())
                }
            },
        ),
    ]
}

fn codex_process_fixture(
    pattern: &'static str,
    tool: &'static str,
    open: bool,
    governed: bool,
) -> StepDef {
    codex_process_fixture_options(pattern, tool, open, governed, true, None)
}

fn codex_process_fixture_options(
    pattern: &'static str,
    tool: &'static str,
    open: bool,
    governed: bool,
    include_hearth: bool,
    timing: Option<(u64, u64)>,
) -> StepDef {
    step_def(
        pattern,
        &[],
        &[
            (FS_HEARTH_KEY, "PathBuf"),
            (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            (FS_EDITED_PATH_KEY, "PathBuf"),
            (PROCESS_PAYLOAD_KEY, "String"),
            (PROCESS_DEADLINE_MS_KEY, "u64"),
            (PROCESS_DELAY_MS_KEY, "u64"),
            (PROCESS_INCLUDE_HEARTH_KEY, "bool"),
        ],
        move |_ctx, _| {
            let (handle, hearth) = retained_temp_dir("anvil-codex-gate-process-")?;
            let target_dir = if governed {
                let dir = hearth.join(FS_TRACK_REL);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let activity = if open {
                    "activity:\n  - kind: begin\n    actor: Codex-500010\n    state: spec\n    at: 2026-07-02T20:40:00Z\n    conversation_id: codex-process\n"
                } else {
                    "activity: []\n"
                };
                std::fs::write(
                    dir.join("status.yaml"),
                    format!("version: 1\nkind: track\nstate: spec\nactors: {{}}\n{activity}"),
                )
                .map_err(|e| e.to_string())?;
                dir
            } else {
                let dir = hearth.join("outside");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                dir
            };
            let edited = target_dir.join("spec.md");
            std::fs::write(&edited, "# mutation\n").map_err(|e| e.to_string())?;
            let payload = if tool == "apply_patch" {
                serde_json::json!({
                    "tool_name": "apply_patch",
                    "cwd": target_dir,
                    "tool_input": {"command": format!("*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch\n", edited.display())}
                })
            } else {
                serde_json::json!({
                    "tool_name": "Bash",
                    "cwd": target_dir,
                    "tool_input": {"command": format!("touch {}", edited.display())}
                })
            };
            let mut out = Context::new();
            out.set(FS_HEARTH_KEY, hearth);
            out.set(FS_HEARTH_HANDLE_KEY, handle);
            out.set(FS_EDITED_PATH_KEY, edited);
            out.set(PROCESS_PAYLOAD_KEY, payload.to_string());
            out.set(PROCESS_INCLUDE_HEARTH_KEY, include_hearth);
            if let Some((deadline, delay)) = timing {
                out.set(PROCESS_DEADLINE_MS_KEY, deadline);
                out.set(PROCESS_DELAY_MS_KEY, delay);
            }
            Ok(out)
        },
    )
}

fn codex_deadline_fixture() -> StepDef {
    codex_process_fixture_options(
        "a Codex governed mutation with an open begin whose resolver exceeds the internal deadline",
        "apply_patch",
        true,
        true,
        true,
        Some((25, 100)),
    )
}

fn codex_malformed_resolver_fixture() -> StepDef {
    step_def(
        "a Codex governed mutation with malformed artifact state",
        &[],
        &[
            (FS_HEARTH_KEY, "PathBuf"),
            (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            (FS_EDITED_PATH_KEY, "PathBuf"),
            (PROCESS_PAYLOAD_KEY, "String"),
            (PROCESS_INCLUDE_HEARTH_KEY, "bool"),
        ],
        |_ctx, _| {
            let (handle, hearth) = retained_temp_dir("anvil-codex-malformed-state-")?;
            let track_dir = hearth.join(FS_TRACK_REL);
            std::fs::create_dir_all(&track_dir).map_err(|e| e.to_string())?;
            std::fs::write(track_dir.join("status.yaml"), "kind: [not-a-string\n")
                .map_err(|e| e.to_string())?;
            let edited = track_dir.join("spec.md");
            std::fs::write(&edited, "# governed\n").map_err(|e| e.to_string())?;
            let payload = serde_json::json!({
                "tool_name": "apply_patch",
                "cwd": track_dir,
                "tool_input": {"command": format!(
                    "*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch\n",
                    edited.display()
                )}
            });
            let mut out = Context::new();
            out.set(FS_HEARTH_KEY, hearth);
            out.set(FS_HEARTH_HANDLE_KEY, handle);
            out.set(FS_EDITED_PATH_KEY, edited);
            out.set(PROCESS_PAYLOAD_KEY, payload.to_string());
            out.set(PROCESS_INCLUDE_HEARTH_KEY, true);
            Ok(out)
        },
    )
}

fn codex_multi_target_fixture(pattern: &'static str, tool: &'static str) -> StepDef {
    step_def(
        pattern,
        &[],
        &[
            (FS_HEARTH_KEY, "PathBuf"),
            (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            (FS_EDITED_PATH_KEY, "PathBuf"),
            (PROCESS_PAYLOAD_KEY, "String"),
            (PROCESS_INCLUDE_HEARTH_KEY, "bool"),
        ],
        move |_ctx, _| {
            let (handle, root) = retained_temp_dir("anvil-codex-multi-target-")?;
            let hearth = root.join("hearth");
            let track_dir = hearth.join(FS_TRACK_REL);
            let outside_dir = root.join("outside");
            std::fs::create_dir_all(&track_dir).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&outside_dir).map_err(|e| e.to_string())?;
            std::fs::write(hearth.join(".hearth"), "path: .\n").map_err(|e| e.to_string())?;
            std::fs::write(
                track_dir.join("status.yaml"),
                "version: 1\nkind: track\nstate: spec\nactors: {}\nactivity: []\n",
            )
            .map_err(|e| e.to_string())?;
            let governed = track_dir.join("spec.md");
            let outside = outside_dir.join("notes.md");
            std::fs::write(&governed, "# governed\n").map_err(|e| e.to_string())?;
            std::fs::write(&outside, "# outside\n").map_err(|e| e.to_string())?;
            let payload = if tool == "apply_patch" {
                serde_json::json!({
                    "tool_name": "apply_patch",
                    "cwd": root,
                    "tool_input": {"command": format!(
                        "*** Begin Patch\n*** Update File: {}\n@@\n-old\n+new\n*** Update File: {}\n@@\n-old\n+new\n*** End Patch\n",
                        outside.display(),
                        governed.display()
                    )}
                })
            } else if tool == "apply_patch_move" {
                serde_json::json!({
                    "tool_name": "apply_patch",
                    "cwd": root,
                    "tool_input": {"command": format!(
                        "*** Begin Patch\n*** Update File: {}\n*** Move to: {}\n@@\n-old\n+new\n*** End Patch\n",
                        outside.display(),
                        governed.display()
                    )}
                })
            } else {
                serde_json::json!({
                    "tool_name": "Bash",
                    "cwd": root,
                    "tool_input": {"command": format!(
                        "touch {} {}",
                        outside.display(),
                        governed.display()
                    )}
                })
            };
            let mut out = Context::new();
            out.set(FS_HEARTH_KEY, hearth);
            out.set(FS_HEARTH_HANDLE_KEY, handle);
            out.set(FS_EDITED_PATH_KEY, governed);
            out.set(PROCESS_PAYLOAD_KEY, payload.to_string());
            out.set(PROCESS_INCLUDE_HEARTH_KEY, false);
            Ok(out)
        },
    )
}

fn codex_bash_root_fixture(
    pattern: &'static str,
    open: bool,
    command: Option<&'static str>,
) -> StepDef {
    step_def(
        pattern,
        &[],
        &[
            (FS_HEARTH_KEY, "PathBuf"),
            (FS_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            (FS_EDITED_PATH_KEY, "PathBuf"),
            (PROCESS_PAYLOAD_KEY, "String"),
        ],
        move |_ctx, _| {
            let (handle, hearth) = retained_temp_dir("anvil-codex-bash-root-")?;
            let track_dir = hearth.join(FS_TRACK_REL);
            std::fs::create_dir_all(&track_dir).map_err(|e| e.to_string())?;
            let activity = if open {
                "activity:\n  - kind: begin\n    actor: Codex-500010\n    state: spec\n    at: 2026-07-02T20:40:00Z\n    conversation_id: codex-bash-root\n"
            } else {
                "activity: []\n"
            };
            std::fs::write(
                track_dir.join("status.yaml"),
                format!("version: 1\nkind: track\nstate: spec\nactors: {{}}\n{activity}"),
            )
            .map_err(|e| e.to_string())?;
            let edited = track_dir.join("spec.md");
            std::fs::write(&edited, "# mutation\n").map_err(|e| e.to_string())?;
            let payload = serde_json::json!({
                "tool_name": "Bash",
                "cwd": hearth,
                "tool_input": {"command": command}
            });
            let mut out = Context::new();
            out.set(FS_HEARTH_KEY, hearth);
            out.set(FS_HEARTH_HANDLE_KEY, handle);
            out.set(FS_EDITED_PATH_KEY, edited);
            out.set(PROCESS_PAYLOAD_KEY, payload.to_string());
            Ok(out)
        },
    )
}
