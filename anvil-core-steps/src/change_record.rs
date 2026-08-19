//! Step definitions for the change record at the anvil-core seam — the
//! engine is anvil-core's user, so baseline import, journal recovery, the
//! divergence fold, commit-metadata rendering and ref compare-and-swap are all
//! proven in-process against a real temporary git repository: no subprocess, no
//! port, no tonic.
//!
//! The assertions here are deliberately made against **real git**, not against
//! a returned struct. `the baseline tree contains hearth path` runs `ls-tree`
//! over the ref; `HEAD, the index and the working tree are unchanged` diffs a
//! captured `rev-parse HEAD` / `ls-files -s` / `status --porcelain` triple. An
//! import that reported success while writing nothing would pass an assertion
//! over its own return value and fail every assertion here.

use anvil_core::domain::change_record::message::{
    parse_trailers, trailer_value, CommitMetadata, DECLARED_TRAILER_KEYS, REQUIRED_TRAILER_COUNT,
};
use anvil_core::domain::telemetry_salt::actor_hash;
use anvil_core_hearth::change_record_baseline::{
    baseline_tree_paths, import_baseline, recorded_paths, replay_baseline, BaselineImport,
    BaselineImportError,
};
use anvil_core_hearth::change_record_commit::{cat_file, ChangeRecordError};
use anvil_core_hearth::change_record_journal::{
    cas_barrier_waiting_marker, leg_dirs, recover, record_leg, Recovery, CAS_BARRIER_ENV,
};
use anvil_core_hearth::git_plumbing::{BASELINE_REF, CHANGE_RECORD_REF};
use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The three observables that together say "the human's checkout is where they
/// left it": the commit HEAD names, the staged index, and the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeState {
    head: String,
    index: String,
    status: String,
    files: Vec<(String, Vec<u8>)>,
}

type ImportResults = Vec<Result<BaselineImport, BaselineImportError>>;

/// Run git in a fixture. Test-side only: production git goes through the closed
/// subcommand set in `anvil_core_hearth::git_plumbing`.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git {:?}: {}", args, e))?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} in {} failed ({}): {}",
            args,
            dir.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("create {}: {}", to.display(), e))?;
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("read_dir {}: {}", from.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry under {}: {}", from.display(), e))?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|e| format!("copy {}: {}", entry.path().display(), e))?;
        }
    }
    Ok(())
}

fn walk_files(root: &Path, prefix: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(root).map_err(|e| format!("read_dir {}: {}", root.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry under {}: {}", root.display(), e))?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let rel = prefix.join(&name);
        if entry.path().is_dir() {
            walk_files(&entry.path(), &rel, out)?;
        } else {
            let bytes = std::fs::read(entry.path())
                .map_err(|e| format!("read {}: {}", entry.path().display(), e))?;
            out.push((rel.to_string_lossy().replace('\\', "/"), bytes));
        }
    }
    Ok(())
}

fn capture_worktree_state(repo: &Path) -> Result<WorktreeState, String> {
    let mut files = Vec::new();
    walk_files(repo, Path::new(""), &mut files)?;
    files.sort();
    Ok(WorktreeState {
        head: git(repo, &["rev-parse", "HEAD"])?,
        index: git(repo, &["ls-files", "-s"])?,
        status: git(repo, &["status", "--porcelain"])?,
        files,
    })
}

fn hearth_of(ctx: &Context) -> Result<PathBuf, String> {
    Ok(ctx
        .get::<PathBuf>("hearth_path")
        .ok_or("No hearth_path")?
        .clone())
}

fn carry_fixture(ctx: &Context, out: &mut Context) {
    carry_retained_temp_dir(ctx, out, "hearth_path_handle");
    carry_retained_temp_dir(ctx, out, "cr_outer_handle");
    if let Some(state) = ctx.get::<WorktreeState>("cr_pre_state") {
        out.set("cr_pre_state", state.clone());
    }
    if let Some(outer) = ctx.get::<PathBuf>("cr_outer_repo") {
        out.set("cr_outer_repo", outer.clone());
    }
}

fn last_import(ctx: &Context) -> Result<&Result<BaselineImport, BaselineImportError>, String> {
    ctx.get::<ImportResults>("cr_imports")
        .ok_or("No cr_imports — baseline import was never run")?
        .last()
        .ok_or_else(|| "cr_imports is empty".to_string())
}

// ── the transaction fixture ─────────────────────────────────────────────────
//
// Brine REPLACES the context with each step's declared `provides`, so every
// step below carries the whole change-record fixture forward. One key list,
// used as every step's `provides`, is what keeps a later step from silently
// dropping an earlier one's state.

const CR_KEYS: &[(&str, &str)] = &[
    ("hearth_path", "PathBuf"),
    ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
    ("cr_outer_repo", "PathBuf"),
    ("cr_outer_handle", "Arc<Mutex<Option<TempDir>>>"),
    ("cr_outside_handle", "Arc<Mutex<Option<TempDir>>>"),
    ("cr_pre_state", "WorktreeState"),
    ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>"),
    ("cr_meta", "CommitMetadata"),
    ("cr_paths", "Vec<String>"),
    ("cr_salt", "String"),
    ("cr_actor", "String"),
    ("cr_conversation", "String"),
    ("cr_commit", "String"),
    ("cr_object", "String"),
    ("cr_result", "Result<String, ChangeRecordError>"),
    ("cr_recovery", "Recovery"),
    ("cr_tip_before", "String"),
    // The second writer of a ref race, and what the race observed.
    ("cr_meta_second", "CommitMetadata"),
    ("cr_paths_second", "Vec<String>"),
    ("cr_second_hearth", "PathBuf"),
    ("cr_second_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
    ("cr_pre_race_tip", "String"),
    ("cr_free_commit", "String"),
    ("cr_held_result", "Result<String, ChangeRecordError>"),
    ("cr_held_blocked", "bool"),
];

type LegResult = Result<String, ChangeRecordError>;

fn copy<T: Clone + Send + Sync + 'static>(ctx: &Context, out: &mut Context, key: &str) {
    if let Some(value) = ctx.get::<T>(key) {
        out.set(key, value.clone());
    }
}

fn carry(ctx: &Context) -> Context {
    let mut out = Context::new();
    carry_fixture(ctx, &mut out);
    carry_retained_temp_dir(ctx, &mut out, "cr_outside_handle");
    copy::<PathBuf>(ctx, &mut out, "hearth_path");
    copy::<ImportResults>(ctx, &mut out, "cr_imports");
    copy::<CommitMetadata>(ctx, &mut out, "cr_meta");
    copy::<Vec<String>>(ctx, &mut out, "cr_paths");
    copy::<String>(ctx, &mut out, "cr_salt");
    copy::<String>(ctx, &mut out, "cr_actor");
    copy::<String>(ctx, &mut out, "cr_conversation");
    copy::<String>(ctx, &mut out, "cr_commit");
    copy::<String>(ctx, &mut out, "cr_object");
    copy::<LegResult>(ctx, &mut out, "cr_result");
    copy::<Recovery>(ctx, &mut out, "cr_recovery");
    copy::<String>(ctx, &mut out, "cr_tip_before");
    carry_retained_temp_dir(ctx, &mut out, "cr_second_hearth_handle");
    copy::<CommitMetadata>(ctx, &mut out, "cr_meta_second");
    copy::<Vec<String>>(ctx, &mut out, "cr_paths_second");
    copy::<PathBuf>(ctx, &mut out, "cr_second_hearth");
    copy::<String>(ctx, &mut out, "cr_pre_race_tip");
    copy::<String>(ctx, &mut out, "cr_free_commit");
    copy::<LegResult>(ctx, &mut out, "cr_held_result");
    copy::<bool>(ctx, &mut out, "cr_held_blocked");
    out
}

/// The configured salt, or `None`. The empty string is "no salt configured",
/// which is what makes the fail-safe (hash ABSENT, never raw) reachable.
fn salt_of(ctx: &Context) -> Option<String> {
    ctx.get::<String>("cr_salt")
        .filter(|salt| !salt.is_empty())
        .cloned()
}

/// Hashed HERE rather than at declaration time, so a scenario that sets the
/// salt after the transaction still gets the salt it declared.
fn apply_hashes(ctx: &Context, mut meta: CommitMetadata) -> CommitMetadata {
    let salt = salt_of(ctx);
    meta.actor_hash = actor_hash(salt.as_deref(), ctx.get::<String>("cr_actor").map_or("", |a| a));
    meta.conversation_hash = actor_hash(
        salt.as_deref(),
        ctx.get::<String>("cr_conversation").map_or("", |c| c),
    );
    meta
}

fn meta_of(ctx: &Context) -> Result<CommitMetadata, String> {
    let meta = ctx
        .get::<CommitMetadata>("cr_meta")
        .ok_or("No cr_meta — no change-record transaction was declared")?
        .clone();
    Ok(apply_hashes(ctx, meta))
}

/// The second writer of a ref race.
fn second_meta_of(ctx: &Context) -> Result<CommitMetadata, String> {
    let meta = ctx
        .get::<CommitMetadata>("cr_meta_second")
        .ok_or("No cr_meta_second — no second change-record transaction was declared")?
        .clone();
    Ok(apply_hashes(ctx, meta))
}

/// One copy of the transaction table's grammar, read by both the first and the
/// second writer's declaration. Two copies of a fixture parser is how the two
/// writers of a race end up meaning subtly different things by `path`.
fn transaction_from_rows(
    hearth: &Path,
    rows: &[(String, String)],
    default_operation_id: &str,
) -> (CommitMetadata, Vec<String>, String, String) {
    let first = |key: &str| -> String {
        rows.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let optional = |key: &str| -> Option<String> {
        let value = first(key);
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    };
    let paths: Vec<String> = rows
        .iter()
        .filter(|(k, _)| k == "path")
        .map(|(_, v)| v.clone())
        .collect();
    let event_kinds = first("event_kinds");
    let meta = CommitMetadata {
        // Deterministic where it can be: `at` is fixed so a rolled-forward
        // commit is byte-comparable to the commit an uninterrupted run would
        // have produced.
        operation_id: optional("operation_id").unwrap_or_else(|| default_operation_id.to_string()),
        command: first("command"),
        artifact_kind: first("artifact_kind"),
        event_kinds: if event_kinds.is_empty() {
            Vec::new()
        } else {
            event_kinds.split(',').map(str::to_string).collect()
        },
        repository_label: hearth
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        paths_recorded: paths.len(),
        at: optional("at").unwrap_or_else(|| "2026-08-14T12:00:00Z".to_string()),
        actor_hash: None,
        conversation_hash: None,
        project_label: optional("project_label"),
        playbook_run_id: optional("playbook_run_id"),
    };
    (meta, paths, first("actor"), first("conversation_id"))
}

/// A transaction table's `key | value` rows, in file order.
fn transaction_rows(params: &Params) -> Result<Vec<(String, String)>, String> {
    let table = params.data_table().ok_or("Expected a data table")?;
    let mut rows: Vec<(String, String)> = Vec::new();
    for row in &table.rows {
        if row.len() < 2 {
            return Err(format!("a transaction row needs key and value: {:?}", row));
        }
        rows.push((row[0].trim().to_string(), row[1].trim().to_string()));
    }
    Ok(rows)
}

fn paths_of(ctx: &Context) -> Vec<String> {
    ctx.get::<Vec<String>>("cr_paths").cloned().unwrap_or_default()
}

fn record(ctx: &Context, crash: Option<&str>, test_mode: bool) -> Result<Context, String> {
    let hearth = hearth_of(ctx)?;
    // Establish the lineage the way a hearth actually gets one, and record the
    // import where the shipped ref assertions look for it.
    let mut imports: ImportResults = ctx.get::<ImportResults>("cr_imports").cloned().unwrap_or_default();
    let import = import_baseline(&hearth);
    if let Err(e) = &import {
        return Err(format!("baseline import failed: {}", e));
    }
    imports.push(import);
    let meta = meta_of(ctx)?;
    let paths = paths_of(ctx);
    if let Some(token) = crash {
        if test_mode {
            std::env::set_var("ANVIL_TEST_MODE", "1");
        } else {
            std::env::remove_var("ANVIL_TEST_MODE");
        }
        std::env::set_var("ANVIL_TEST_CHANGE_RECORD_CRASH_AFTER", token);
    }
    let result = record_leg(&hearth, &hearth, &meta, &paths);
    if crash.is_some() {
        std::env::remove_var("ANVIL_TEST_CHANGE_RECORD_CRASH_AFTER");
        std::env::remove_var("ANVIL_TEST_MODE");
    }
    let mut out = carry(ctx);
    if let Ok(commit) = &result {
        let object = cat_file(&hearth, commit)
            .map_err(|e| format!("could not read the commit back: {}", e))?;
        out.set("cr_commit", commit.clone());
        out.set("cr_object", object);
    }
    out.set("cr_imports", imports);
    out.set::<LegResult>("cr_result", result);
    Ok(out)
}

fn object_of(ctx: &Context) -> Result<String, String> {
    ctx.get::<String>("cr_object")
        .cloned()
        .ok_or_else(|| match ctx.get::<LegResult>("cr_result") {
            Some(Err(e)) => format!("no commit was written: {}", e),
            _ => "no commit was written and no attempt was recorded".to_string(),
        })
}

/// Raw bytes out of git. The trimmed [`git`] helper cannot answer a
/// byte-for-byte question, and a trailing newline is exactly the difference a
/// byte-for-byte assertion exists to catch.
fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git {:?}: {}", args, e))?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} in {} failed: {}",
            args,
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Every commit reachable from the change-record tip, tip first.
fn lineage(hearth: &Path) -> Result<Vec<String>, String> {
    let tip = match git(hearth, &["rev-parse", "--verify", "--quiet", CHANGE_RECORD_REF]) {
        Ok(sha) if !sha.trim().is_empty() => sha.trim().to_string(),
        _ => return Ok(Vec::new()),
    };
    let listed = git(hearth, &["rev-list", &tip])?;
    Ok(listed.lines().map(str::to_string).collect())
}

fn transaction_steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the hearth repository git config sets user.name {string} and user.email {string}",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected a name")?;
                let email = params.get_string(1).ok_or("Expected an email")?;
                let hearth = hearth_of(&ctx)?;
                if !hearth.join(".git").exists() {
                    git(&hearth, &["init", "--quiet"])?;
                }
                git(&hearth, &["config", "user.name", name])?;
                git(&hearth, &["config", "user.email", email])?;
                // A human's checkout, so the never-touch-HEAD assertion covers
                // the PER-TRANSACTION writer and not only baseline import. Two
                // modifications, exactly ONE of them staged: with everything
                // staged, an engine that wrote the repository's own index would
                // write the bytes already there and the assertion would be
                // green while the property was broken.
                git(&hearth, &["add", "."])?;
                git(&hearth, &["commit", "--quiet", "-m", "human commit"])?;
                std::fs::write(hearth.join("tracks.md"), "# Tracks\n\n- edited by the human\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;
                git(&hearth, &["add", "tracks.md"])?;
                std::fs::write(
                    hearth.join("tracks/t-record/status.yaml"),
                    "version: 1\nstate: spec\n# human edit\n",
                )
                .map_err(|e| format!("write status.yaml: {}", e))?;
                let pre = capture_worktree_state(&hearth)?;
                let mut out = carry(&ctx);
                out.set("cr_pre_state", pre);
                Ok(out)
            },
        ),
        step_def(
            "the change-record telemetry salt is {string}",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, params| {
                let salt = params.get_string(0).ok_or("Expected a salt")?.to_string();
                let mut out = carry(&ctx);
                out.set("cr_salt", salt);
                Ok(out)
            },
        ),
        step_def(
            "the change-record hearth has no telemetry salt",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, _params| {
                let mut out = carry(&ctx);
                out.set("cr_salt", String::new());
                Ok(out)
            },
        ),
        step_def(
            "a change-record transaction with:",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, params| {
                let hearth = hearth_of(&ctx)?;
                let rows = transaction_rows(params)?;
                let (meta, paths, actor, conversation) = transaction_from_rows(
                    &hearth,
                    &rows,
                    &format!("cr-op-{}", std::process::id()),
                );
                let mut out = carry(&ctx);
                out.set("cr_meta", meta);
                out.set("cr_paths", paths);
                out.set("cr_actor", actor);
                out.set("cr_conversation", conversation);
                Ok(out)
            },
        ),
        step_def(
            "the transaction carries reflection notes {string} and approver {string}",
            &[("hearth_path", "PathBuf"), ("cr_paths", "Vec<String>")],
            CR_KEYS,
            |ctx, params| {
                let notes = params.get_string(0).ok_or("Expected reflection notes")?;
                let approver = params.get_string(1).ok_or("Expected an approver")?;
                let hearth = hearth_of(&ctx)?;
                // The prose goes into the TREE, which is where prose legitimately
                // lives. The assertion is that it never reaches the metadata — so
                // this step has to put it somewhere real, or the scenario proves
                // nothing.
                let relative = "tracks/t-record/spec_reflection/notes.md";
                let full = hearth.join(relative);
                std::fs::create_dir_all(full.parent().ok_or("no parent")?)
                    .map_err(|e| format!("create the reflection dir: {}", e))?;
                std::fs::write(&full, format!("{}\n\napprover: {}\n", notes, approver))
                    .map_err(|e| format!("write {}: {}", relative, e))?;
                let mut paths = paths_of(&ctx);
                paths.push(relative.to_string());
                let mut out = carry(&ctx);
                out.set("cr_paths", paths);
                Ok(out)
            },
        ),
        step_def(
            "the change-record commit is written",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            CR_KEYS,
            |ctx, _params| {
                let out = record(&ctx, None, false)?;
                match out.get::<LegResult>("cr_result") {
                    Some(Err(e)) => Err(format!("the change-record write refused: {}", e)),
                    _ => Ok(out),
                }
            },
        ),
        step_def(
            "a change-record transaction is recorded for that hearth",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            CR_KEYS,
            |ctx, _params| record(&ctx, None, false),
        ),
        step_def(
            "a change-record transaction is interrupted after {string}",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            CR_KEYS,
            |ctx, params| {
                let token = params.get_string(0).ok_or("Expected a crash token")?;
                record(&ctx, Some(token), true)
            },
        ),
        step_def(
            "a change-record transaction is interrupted after {string} with test mode unset",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            CR_KEYS,
            |ctx, params| {
                let token = params.get_string(0).ok_or("Expected a crash token")?;
                record(&ctx, Some(token), false)
            },
        ),
        step_def(
            "change-record recovery is run for that hearth",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let before = git(&hearth, &["rev-parse", "--verify", "--quiet", CHANGE_RECORD_REF])
                    .unwrap_or_default();
                let recovery =
                    recover(&hearth).map_err(|e| format!("recovery refused outright: {}", e))?;
                let mut out = carry(&ctx);
                out.set("cr_tip_before", before.trim().to_string());
                out.set("cr_recovery", recovery);
                Ok(out)
            },
        ),
        step_def(
            "the hearth file {string} is changed to {string}",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, params| {
                let relative = params.get_string(0).ok_or("Expected a path")?;
                let content = params.get_string(1).ok_or("Expected content")?;
                let hearth = hearth_of(&ctx)?;
                std::fs::write(hearth.join(relative), content)
                    .map_err(|e| format!("write {}: {}", relative, e))?;
                Ok(carry(&ctx))
            },
        ),
        step_def(
            "the hearth is copied outside the platform temporary directory",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                // `target/` is disposable, is inside this repository, and is
                // emphatically NOT under the platform temp dir — which is the
                // one property the crash-point gate is being asked about.
                let base = Path::new(anvil_test_support::TEST_SUPPORT_DIR)
                    .parent()
                    .ok_or("no workspace root")?
                    .join("target");
                std::fs::create_dir_all(&base)
                    .map_err(|e| format!("create {}: {}", base.display(), e))?;
                let dir = tempfile::Builder::new()
                    .prefix("anvil-cr-outside-")
                    .tempdir_in(&base)
                    .map_err(|e| format!("scratch dir under {}: {}", base.display(), e))?;
                let outside = dir.path().join("hearth");
                copy_tree(&hearth, &outside)?;
                // Its OWN repository. `target/` sits inside this repository, and
                // baseline import refuses a hearth it does not own — a nested
                // `.git` makes the copy its own toplevel, so the scenario reaches
                // the crash gate instead of stopping at the foreign-repo refusal.
                git(&outside, &["init", "--quiet"])?;
                let mut out = carry(&ctx);
                out.set("hearth_path", outside);
                out.set::<anvil_test_support::RetainedTempDir>(
                    "cr_outside_handle",
                    std::sync::Arc::new(std::sync::Mutex::new(Some(dir))),
                );
                Ok(out)
            },
        ),
    ]
}

fn assertion_steps() -> Vec<StepDef> {
    vec![
        check_def(
            "the commit author and committer are {string} and {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected a name")?;
                let email = params.get_string(1).ok_or("Expected an email")?;
                let object = object_of(&ctx)?;
                let wanted = format!("{} <{}>", name, email);
                for header in ["author", "committer"] {
                    let line = object
                        .lines()
                        .find(|l| l.starts_with(&format!("{} ", header)))
                        .ok_or_else(|| format!("the commit carries no {} header", header))?;
                    if !line.starts_with(&format!("{} {}", header, wanted)) {
                        return Err(format!(
                            "the {} is not the fixed engine identity '{}': {}",
                            header, wanted, line
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            // Reproducibility, asserted rather than documented: the author and
            // committer timestamps come from the transaction's declared `at`,
            // so a rolled-forward commit hashes to the SAME object id as the
            // one an uninterrupted run would have written. Left to "now", a
            // recovered commit would be a different object carrying the same
            // operation id, and exactly-once would rest on the id check alone.
            "the commit is dated the declared at",
            &[("cr_object", "String"), ("cr_meta", "CommitMetadata")],
            |ctx, _params| {
                let object = object_of(&ctx)?;
                let at = ctx.get::<CommitMetadata>("cr_meta").ok_or("No cr_meta")?.at.clone();
                let epoch = chrono::DateTime::parse_from_rfc3339(&at)
                    .map_err(|e| format!("'{}' is not an RFC 3339 timestamp: {}", at, e))?
                    .timestamp()
                    .to_string();
                for header in ["author", "committer"] {
                    let line = object
                        .lines()
                        .find(|l| l.starts_with(&format!("{} ", header)))
                        .ok_or_else(|| format!("the commit carries no {} header", header))?;
                    if !line.split_whitespace().any(|token| token == epoch) {
                        return Err(format!(
                            "the {} is not dated the declared at ({} = epoch {}): {}",
                            header, at, epoch, line
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the commit metadata does not contain {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let forbidden = params.get_string(0).ok_or("Expected a value")?;
                let object = object_of(&ctx)?;
                if object.contains(forbidden) {
                    return Err(format!(
                        "the commit metadata carries '{}':\n{}",
                        forbidden, object
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the commit metadata contains no absolute path",
            &[("cr_object", "String"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let object = object_of(&ctx)?;
                let hearth = hearth_of(&ctx)?;
                if object.contains(&hearth.display().to_string()) {
                    return Err(format!(
                        "the commit metadata names the hearth's absolute path {}",
                        hearth.display()
                    ));
                }
                // The message body only: the `tree`/`parent` headers hold object
                // ids and the identity headers hold the fixed engine identity,
                // neither of which can be a path.
                let body = object.split_once("\n\n").map(|(_, b)| b).unwrap_or("");
                for token in body.split_whitespace() {
                    if token.starts_with('/') {
                        return Err(format!(
                            "the commit metadata carries the absolute path '{}'",
                            token
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the commit has a trailer named {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected a trailer key")?;
                let object = object_of(&ctx)?;
                match trailer_value(&object, key) {
                    Some(value) if !value.is_empty() => Ok(()),
                    Some(_) => Err(format!("the trailer '{}' is present but empty", key)),
                    None => Err(format!(
                        "the commit carries no trailer '{}'. It carries: {}",
                        key,
                        trailer_keys(&object).join(", ")
                    )),
                }
            },
        ),
        check_def(
            "the commit has no trailer named {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected a trailer key")?;
                let object = object_of(&ctx)?;
                match trailer_value(&object, key) {
                    None => Ok(()),
                    Some(value) => Err(format!(
                        "the trailer '{}' is present as '{}'. With no salt the key must be \
                         ABSENT, never empty and never raw.",
                        key, value
                    )),
                }
            },
        ),
        check_def(
            "the commit trailer {string} is {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected a trailer key")?;
                let wanted = params.get_string(1).ok_or("Expected a value")?;
                let object = object_of(&ctx)?;
                match trailer_value(&object, key).as_deref() {
                    Some(value) if value == wanted => Ok(()),
                    other => Err(format!(
                        "the trailer '{}' is {:?}, expected '{}'",
                        key, other, wanted
                    )),
                }
            },
        ),
        check_def(
            "the commit trailer {string} is the salted hash of {string}",
            &[("cr_object", "String"), ("cr_salt", "String")],
            |ctx, params| {
                let key = params.get_string(0).ok_or("Expected a trailer key")?;
                let raw = params.get_string(1).ok_or("Expected a raw value")?;
                let object = object_of(&ctx)?;
                let expected = actor_hash(salt_of(&ctx).as_deref(), raw)
                    .ok_or("no salt is configured, so there is no hash to compare against")?;
                match trailer_value(&object, key).as_deref() {
                    Some(value) if value == expected => Ok(()),
                    other => Err(format!(
                        "the trailer '{}' is {:?}, expected the salted hash {}",
                        key, other, expected
                    )),
                }
            },
        ),
        check_def(
            "the commit trailer key set is exactly the declared allowlist",
            &[("cr_object", "String")],
            |ctx, _params| {
                let object = object_of(&ctx)?;
                let keys = trailer_keys(&object);
                if keys.is_empty() {
                    return Err("the commit carries no trailers at all".to_string());
                }
                for key in &keys {
                    if !DECLARED_TRAILER_KEYS.contains(&key.as_str()) {
                        return Err(format!(
                            "'{}' is not on the declared allowlist {:?}",
                            key, DECLARED_TRAILER_KEYS
                        ));
                    }
                }
                for required in &DECLARED_TRAILER_KEYS[..REQUIRED_TRAILER_COUNT] {
                    if !keys.iter().any(|k| k == required) {
                        return Err(format!("the mandatory trailer '{}' is absent", required));
                    }
                }
                let mut seen = keys.clone();
                seen.sort();
                seen.dedup();
                if seen.len() != keys.len() {
                    return Err(format!("a trailer key is emitted twice: {:?}", keys));
                }
                Ok(())
            },
        ),
        check_def(
            "the commit subject is {string}",
            &[("cr_object", "String")],
            |ctx, params| {
                let wanted = params.get_string(0).ok_or("Expected a subject")?;
                let object = object_of(&ctx)?;
                let subject = object
                    .split_once("\n\n")
                    .map(|(_, body)| body.lines().next().unwrap_or(""))
                    .unwrap_or("");
                if subject != wanted {
                    return Err(format!("the subject is '{}', expected '{}'", subject, wanted));
                }
                Ok(())
            },
        ),
        check_def(
            "the change-record journal holds {int} leg",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")?;
                let hearth = hearth_of(&ctx)?;
                let legs = leg_dirs(&hearth).map_err(|e| format!("read the journal: {}", e))?;
                if legs.len() as i64 != expected {
                    return Err(format!(
                        "expected {} journalled leg(s), found {}: {:?}",
                        expected,
                        legs.len(),
                        legs
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "exactly {int} commit carries that operation id",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")?;
                let hearth = hearth_of(&ctx)?;
                let id = ctx
                    .get::<CommitMetadata>("cr_meta")
                    .ok_or("No cr_meta")?
                    .operation_id
                    .clone();
                let mut found = Vec::new();
                for sha in lineage(&hearth)? {
                    let object = cat_file(&hearth, &sha)
                        .map_err(|e| format!("read commit {}: {}", sha, e))?;
                    if trailer_value(&object, DECLARED_TRAILER_KEYS[0]).as_deref() == Some(&id) {
                        found.push(sha);
                    }
                }
                if found.len() as i64 != expected {
                    return Err(format!(
                        "expected {} commit(s) carrying operation id '{}', found {}: {:?}",
                        expected,
                        id,
                        found.len(),
                        found
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the recorded commit tree matches the hearth content byte-for-byte",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_paths", "Vec<String>"),
                ("cr_meta", "CommitMetadata"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let paths = paths_of(&ctx);
                if paths.is_empty() {
                    return Err(
                        "the transaction declared no paths, so this assertion is vacuous"
                            .to_string(),
                    );
                }
                // The tip has to BE this operation's commit before its tree
                // means anything: the baseline tree already equals disk, so a
                // byte comparison against whatever the tip happens to be is
                // green whether the roll-forward ran or not.
                let id = ctx
                    .get::<CommitMetadata>("cr_meta")
                    .ok_or("No cr_meta")?
                    .operation_id
                    .clone();
                let tip = git(&hearth, &["rev-parse", "--verify", "--quiet", CHANGE_RECORD_REF])
                    .map_err(|e| format!("{} does not resolve: {}", CHANGE_RECORD_REF, e))?;
                let object = cat_file(&hearth, tip.trim())
                    .map_err(|e| format!("read the tip commit: {}", e))?;
                if trailer_value(&object, DECLARED_TRAILER_KEYS[0]).as_deref() != Some(&id) {
                    return Err(format!(
                        "the tip does not carry operation id '{}', so no commit was recovered \
                         for this transaction:\n{}",
                        id, object
                    ));
                }
                for relative in &paths {
                    let recorded = git_bytes(
                        &hearth,
                        &["cat-file", "-p", &format!("{}:{}", CHANGE_RECORD_REF, relative)],
                    )
                    .map_err(|e| format!("{} is not in the recorded tree: {}", relative, e))?;
                    let on_disk = std::fs::read(hearth.join(relative))
                        .map_err(|e| format!("read {} from the hearth: {}", relative, e))?;
                    if recorded != on_disk {
                        return Err(format!(
                            "'{}' differs.\n  on disk: {:?}\n  recorded: {:?}",
                            relative,
                            String::from_utf8_lossy(&on_disk),
                            String::from_utf8_lossy(&recorded)
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "change-record recovery reports a conflict naming hearth path {string}",
            &[("cr_recovery", "Recovery")],
            |ctx, params| {
                let wanted = params.get_string(0).ok_or("Expected a path")?;
                let recovery = ctx.get::<Recovery>("cr_recovery").ok_or("No cr_recovery")?;
                if recovery.conflicts.iter().any(|c| c.contains(wanted)) {
                    return Ok(());
                }
                Err(format!(
                    "no reported conflict names '{}'. Recovery reported {:?}, rolled forward {:?}",
                    wanted, recovery.conflicts, recovery.rolled_forward
                ))
            },
        ),
        check_def(
            "the file at hearth path {string} still has content {string}",
            &[("hearth_path", "PathBuf")],
            |ctx, params| {
                let relative = params.get_string(0).ok_or("Expected a path")?;
                let wanted = params.get_string(1).ok_or("Expected content")?;
                let hearth = hearth_of(&ctx)?;
                let live = std::fs::read_to_string(hearth.join(relative))
                    .map_err(|e| format!("read {}: {}", relative, e))?;
                if live != wanted {
                    return Err(format!(
                        "'{}' was rewritten. Recovery never rewinds disk.\n  expected: {:?}\n  \
                         found: {:?}",
                        relative, wanted, live
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the change-record ref tip is unchanged",
            &[("hearth_path", "PathBuf"), ("cr_tip_before", "String")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let before = ctx.get::<String>("cr_tip_before").ok_or("No cr_tip_before")?;
                let after = git(&hearth, &["rev-parse", "--verify", "--quiet", CHANGE_RECORD_REF])
                    .unwrap_or_default();
                if before != after.trim() {
                    return Err(format!(
                        "the tip moved: {} -> {}",
                        before,
                        after.trim()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the change-record transaction refuses naming {string}",
            &[("cr_result", "Result<String, ChangeRecordError>")],
            |ctx, params| {
                let wanted = params.get_string(0).ok_or("Expected a phrase")?;
                match ctx.get::<LegResult>("cr_result").ok_or("No cr_result")? {
                    Ok(commit) => Err(format!(
                        "expected a refusal naming '{}', but the transaction committed {}",
                        wanted, commit
                    )),
                    Err(e) => {
                        let text = e.to_string();
                        if text.contains(wanted) {
                            Ok(())
                        } else {
                            Err(format!("the refusal does not name '{}': {}", wanted, text))
                        }
                    }
                }
            },
        ),
        check_def(
            "the change-record transaction completed without interruption",
            &[("cr_result", "Result<String, ChangeRecordError>")],
            |ctx, _params| match ctx.get::<LegResult>("cr_result").ok_or("No cr_result")? {
                Ok(_) => Ok(()),
                Err(e) => Err(format!(
                    "the transaction was interrupted, so the crash point was ARMED when it must \
                     not have been: {}",
                    e
                )),
            },
        ),
    ]
}

fn trailer_keys(object: &str) -> Vec<String> {
    parse_trailers(object).into_iter().map(|(k, _)| k).collect()
}

pub fn steps() -> Vec<StepDef> {
    let mut steps = baseline_steps();
    steps.extend(transaction_steps());
    steps.extend(assertion_steps());
    steps.extend(race_steps());
    steps
}

// ── the deterministic ref race ──────────────────────────────────────────────
//
// The ordering comes from the production barrier, never from a timing hope.
// The held writer signals that it is held by dropping a marker file, and the
// free writer is not started until that marker exists — so the held writer's
// observed-old is stale by construction. A bounded poll on a file that MUST
// appear is a wait on a condition; `sleep(200)` hoping a thread got somewhere
// is the thing this file does not do.

/// How long to wait for the held writer to arm the barrier before calling it a
/// defect. Reached when the production hold is not there at all, so the
/// failure says exactly that rather than timing out anonymously.
const BARRIER_ARM_DEADLINE: Duration = Duration::from_secs(10);

/// What one race observed. `held` is a `Result` because the losing writer
/// refusing IS the interesting failure — a scenario that unwrapped it would
/// report a panic instead of naming the lost update.
struct Race {
    free: String,
    held: LegResult,
    held_blocked_when_free_landed: bool,
    pre_tip: String,
}

fn wait_for_file(path: &Path, deadline: Duration, what: &str) -> Result<(), String> {
    let start = Instant::now();
    while !path.exists() {
        if start.elapsed() > deadline {
            return Err(format!(
                "{}: {} never appeared within {:?}",
                what,
                path.display(),
                deadline
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn tip_of(repo: &Path) -> String {
    git(repo, &["rev-parse", "--verify", "--quiet", CHANGE_RECORD_REF])
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Run the held writer in a thread, hold it at the ref update, land the free
/// writer underneath it, then release.
fn run_race(
    held_hearth: &Path,
    held_meta: &CommitMetadata,
    held_paths: &[String],
    free_hearth: &Path,
    free_meta: &CommitMetadata,
    free_paths: &[String],
) -> Result<Race, String> {
    let pre_tip = tip_of(held_hearth);
    // Under `.git/`, so the barrier is not itself a recorded path.
    let barrier = held_hearth.join(".git").join("anvil").join("cas-barrier");
    let waiting = cas_barrier_waiting_marker(&barrier);
    std::fs::create_dir_all(barrier.parent().ok_or("the barrier has no parent")?)
        .map_err(|e| format!("create the barrier directory: {}", e))?;
    let _ = std::fs::remove_file(&barrier);
    let _ = std::fs::remove_file(&waiting);

    std::env::set_var("ANVIL_TEST_MODE", "1");
    std::env::set_var(CAS_BARRIER_ENV, &barrier);

    let (thread_hearth, thread_meta, thread_paths) = (
        held_hearth.to_path_buf(),
        held_meta.clone(),
        held_paths.to_vec(),
    );
    let handle = std::thread::spawn(move || {
        record_leg(&thread_hearth, &thread_hearth, &thread_meta, &thread_paths)
    });

    let armed = wait_for_file(
        &waiting,
        BARRIER_ARM_DEADLINE,
        "the compare-and-swap barrier never armed, so no writer was held and the race never \
         happened",
    );
    let landed = if armed.is_ok() {
        let free = record_leg(free_hearth, free_hearth, free_meta, free_paths);
        // Observed BEFORE the release, which is the only moment the answer is
        // meaningful.
        let blocked = !handle.is_finished();
        Some((free, blocked))
    } else {
        None
    };
    // Released unconditionally: an unarmed race must not leave a parked thread
    // behind for the next scenario to inherit.
    let released = std::fs::write(&barrier, b"released")
        .map_err(|e| format!("release the barrier: {}", e));
    let joined = handle.join();
    std::env::remove_var(CAS_BARRIER_ENV);
    std::env::remove_var("ANVIL_TEST_MODE");

    armed?;
    released?;
    let held = joined.map_err(|_| "the held writer panicked".to_string())?;
    let (free, held_blocked_when_free_landed) = landed.expect("an armed race records an outcome");
    let free = free.map_err(|e| format!("the free writer refused: {}", e))?;
    Ok(Race {
        free,
        held,
        held_blocked_when_free_landed,
        pre_tip,
    })
}

fn second_paths_of(ctx: &Context) -> Vec<String> {
    ctx.get::<Vec<String>>("cr_paths_second")
        .cloned()
        .unwrap_or_default()
}

/// Establish the lineage the way a hearth actually gets one, recording the
/// import where the shipped ref assertions look for it.
fn imported(hearth: &Path, mut imports: ImportResults) -> Result<ImportResults, String> {
    let import = import_baseline(hearth);
    if let Err(e) = &import {
        return Err(format!("baseline import failed for {}: {}", hearth.display(), e));
    }
    imports.push(import);
    Ok(imports)
}

fn record_race(ctx: &Context, free_hearth: Option<PathBuf>) -> Result<Context, String> {
    let hearth = hearth_of(ctx)?;
    let free_hearth = free_hearth.unwrap_or_else(|| hearth.clone());
    let mut imports = imported(
        &hearth,
        ctx.get::<ImportResults>("cr_imports").cloned().unwrap_or_default(),
    )?;
    if free_hearth != hearth {
        imports = imported(&free_hearth, imports)?;
    }
    let held_meta = meta_of(ctx)?;
    let free_meta = second_meta_of(ctx)?;
    let race = run_race(
        &hearth,
        &held_meta,
        &paths_of(ctx),
        &free_hearth,
        &free_meta,
        &second_paths_of(ctx),
    )?;
    let mut out = carry(ctx);
    out.set("cr_imports", imports);
    out.set("cr_pre_race_tip", race.pre_tip);
    out.set("cr_free_commit", race.free);
    out.set("cr_held_blocked", race.held_blocked_when_free_landed);
    out.set::<LegResult>("cr_held_result", race.held);
    Ok(out)
}

fn held_commit(ctx: &Context) -> Result<String, String> {
    match ctx.get::<LegResult>("cr_held_result").ok_or("No cr_held_result")? {
        Ok(commit) => Ok(commit.clone()),
        Err(e) => Err(format!(
            "the writer that lost the ref race recorded nothing: {}",
            e
        )),
    }
}

/// The parent a commit names, read off the object rather than off a struct.
fn parent_of(repo: &Path, commit: &str) -> Result<String, String> {
    let object = cat_file(repo, commit).map_err(|e| format!("read commit {}: {}", commit, e))?;
    object
        .lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix("parent ").map(|rest| rest.trim().to_string()))
        .ok_or_else(|| format!("commit {} names no parent", commit))
}

fn race_steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a second change-record transaction with:",
            &[("hearth_path", "PathBuf"), ("cr_meta", "CommitMetadata")],
            CR_KEYS,
            |ctx, params| {
                let hearth = hearth_of(&ctx)?;
                let rows = transaction_rows(params)?;
                let (meta, paths, _, _) = transaction_from_rows(
                    &hearth,
                    &rows,
                    &format!("cr-op-{}-second", std::process::id()),
                );
                let mut out = carry(&ctx);
                out.set("cr_meta_second", meta);
                out.set("cr_paths_second", paths);
                Ok(out)
            },
        ),
        step_def(
            "a second change-record hearth with its own recorded governance file",
            &[("hearth_path", "PathBuf")],
            CR_KEYS,
            |ctx, _params| {
                let (handle, dir) = retained_temp_dir("anvil-cr-second-hearth-")?;
                std::fs::create_dir_all(dir.join("tracks").join("t-second"))
                    .map_err(|e| format!("create the second hearth: {}", e))?;
                for (relative, content) in [
                    ("tracks.md", "# Tracks\n\n- t-second\n"),
                    ("tracks/t-second/status.yaml", "version: 1\nstate: spec\n"),
                    ("tracks/t-second/spec.md", "# Second hearth spec\n"),
                ] {
                    std::fs::write(dir.join(relative), content)
                        .map_err(|e| format!("write {}: {}", relative, e))?;
                }
                let rows: Vec<(String, String)> = [
                    ("operation_id", "cr-op-second-hearth"),
                    ("command", "complete"),
                    ("artifact_kind", "track"),
                    ("event_kinds", "StateChanged"),
                    ("path", "tracks/t-second/spec.md"),
                ]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
                let (meta, paths, _, _) =
                    transaction_from_rows(&dir, &rows, "cr-op-second-hearth");
                let mut out = carry(&ctx);
                out.set("cr_second_hearth", dir);
                out.set::<anvil_test_support::RetainedTempDir>("cr_second_hearth_handle", handle);
                out.set("cr_meta_second", meta);
                out.set("cr_paths_second", paths);
                Ok(out)
            },
        ),
        step_def(
            "both change-record writers race for the ref",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_meta", "CommitMetadata"),
                ("cr_meta_second", "CommitMetadata"),
            ],
            CR_KEYS,
            |ctx, _params| record_race(&ctx, None),
        ),
        step_def(
            "a writer held at the barrier on the first hearth races a writer on the second hearth",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_meta", "CommitMetadata"),
                ("cr_second_hearth", "PathBuf"),
            ],
            CR_KEYS,
            |ctx, _params| {
                let second = ctx
                    .get::<PathBuf>("cr_second_hearth")
                    .ok_or("No cr_second_hearth")?
                    .clone();
                record_race(&ctx, Some(second))
            },
        ),
        check_def(
            "both recorded commits are reachable from the change-record tip",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_free_commit", "String"),
                ("cr_held_result", "Result<String, ChangeRecordError>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let free = ctx.get::<String>("cr_free_commit").ok_or("No cr_free_commit")?;
                let held = held_commit(&ctx)?;
                if &held == free {
                    return Err(format!(
                        "both writers report the same commit {}, so 'both landed' is vacuous",
                        held
                    ));
                }
                let reachable = lineage(&hearth)?;
                for (who, commit) in [("the winning", free.as_str()), ("the losing", held.as_str())] {
                    if !reachable.iter().any(|sha| sha == commit) {
                        return Err(format!(
                            "{} writer's commit {} is NOT reachable from {} — it was overwritten \
                             rather than compared-and-swapped. Reachable: {:?}",
                            who, commit, CHANGE_RECORD_REF, reachable
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the held writer's commit names the winning commit as its parent",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_free_commit", "String"),
                ("cr_held_result", "Result<String, ChangeRecordError>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let free = ctx.get::<String>("cr_free_commit").ok_or("No cr_free_commit")?;
                let held = held_commit(&ctx)?;
                let parent = parent_of(&hearth, &held)?;
                if &parent != free {
                    return Err(format!(
                        "the losing writer's commit {} names {} as its parent, not the winning \
                         commit {} — it did not rebuild against the new tip",
                        held, parent, free
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the held writer's commit does not name the tip observed before the race as its parent",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_pre_race_tip", "String"),
                ("cr_held_result", "Result<String, ChangeRecordError>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let stale = ctx
                    .get::<String>("cr_pre_race_tip")
                    .ok_or("No cr_pre_race_tip")?;
                if stale.is_empty() {
                    return Err(
                        "there was no tip before the race, so 'not the stale tip' asserts nothing"
                            .to_string(),
                    );
                }
                let held = held_commit(&ctx)?;
                let parent = parent_of(&hearth, &held)?;
                if &parent == stale {
                    return Err(format!(
                        "the losing writer's commit {} still names the pre-race tip {} as its \
                         parent, which is what a FORCED ref update looks like",
                        held, stale
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the second hearth's writer landed while the first was still held",
            &[
                ("cr_second_hearth", "PathBuf"),
                ("cr_free_commit", "String"),
                ("cr_held_blocked", "bool"),
            ],
            |ctx, _params| {
                let blocked = *ctx.get::<bool>("cr_held_blocked").ok_or("No cr_held_blocked")?;
                if !blocked {
                    return Err(
                        "the first hearth's writer was no longer held when the second hearth's \
                         writer returned, so this proves nothing about contention"
                            .to_string(),
                    );
                }
                let second = ctx
                    .get::<PathBuf>("cr_second_hearth")
                    .ok_or("No cr_second_hearth")?;
                let free = ctx.get::<String>("cr_free_commit").ok_or("No cr_free_commit")?;
                if tip_of(second) != *free {
                    return Err(format!(
                        "the second hearth's tip is {}, not the commit {} its writer reported",
                        tip_of(second),
                        free
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "each hearth's change-record ref has exactly {int} commit",
            &[("hearth_path", "PathBuf"), ("cr_second_hearth", "PathBuf")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")?;
                let first = hearth_of(&ctx)?;
                let second = ctx
                    .get::<PathBuf>("cr_second_hearth")
                    .ok_or("No cr_second_hearth")?
                    .clone();
                for hearth in [first, second] {
                    let listed = git(&hearth, &["rev-list", "--count", CHANGE_RECORD_REF])
                        .unwrap_or_else(|_| "0".to_string());
                    let count: i64 = listed.trim().parse().unwrap_or(0);
                    if count != expected {
                        return Err(format!(
                            "expected {} commit(s) on {} in {}, found {}",
                            expected,
                            CHANGE_RECORD_REF,
                            hearth.display(),
                            count
                        ));
                    }
                }
                Ok(())
            },
        ),
    ]
}

fn baseline_steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the hearth is an existing git repository with a committed file and two uncommitted modifications",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("cr_pre_state", "WorktreeState"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                git(&hearth, &["init", "--quiet"])?;
                git(&hearth, &["add", "."])?;
                // A distinctive ambient identity: the fixed engine identity must
                // never inherit it, which file 2 asserts directly.
                git(
                    &hearth,
                    &[
                        "-c",
                        "user.name=Hearth Owner",
                        "-c",
                        "user.email=owner@example.invalid",
                        "commit",
                        "--quiet",
                        "-m",
                        "human commit",
                    ],
                )?;
                // Two uncommitted modifications, ONE of them staged, so the
                // assertion covers the index and the working tree separately.
                std::fs::write(hearth.join("tracks.md"), "# Tracks\n\n- edited by the human\n")
                    .map_err(|e| format!("write tracks.md: {}", e))?;
                git(&hearth, &["add", "tracks.md"])?;
                let status = hearth.join("tracks/t-import/status.yaml");
                std::fs::write(&status, "version: 1\nstate: spec\n# human edit\n")
                    .map_err(|e| format!("write status.yaml: {}", e))?;

                let pre = capture_worktree_state(&hearth)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("cr_pre_state", pre);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "the hearth is a subdirectory of an outer git repository",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("cr_outer_repo", "PathBuf"),
                ("cr_outer_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let (outer_handle, outer) = retained_temp_dir("anvil-outer-repo-")?;
                git(&outer, &["init", "--quiet"])?;
                std::fs::write(outer.join("README.md"), "the human's own repository\n")
                    .map_err(|e| format!("write README.md: {}", e))?;
                git(&outer, &["add", "README.md"])?;
                git(
                    &outer,
                    &[
                        "-c",
                        "user.name=Hearth Owner",
                        "-c",
                        "user.email=owner@example.invalid",
                        "commit",
                        "--quiet",
                        "-m",
                        "outer",
                    ],
                )?;
                let nested = outer.join("hearth");
                copy_tree(&hearth, &nested)?;

                let mut out = Context::new();
                out.set("hearth_path", nested);
                out.set("cr_outer_repo", outer);
                out.set("cr_outer_handle", outer_handle);
                carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
                Ok(out)
            },
        ),
        step_def(
            "baseline import is run for that hearth",
            &[("hearth_path", "PathBuf")],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>"),
                ("cr_pre_state", "WorktreeState"),
                ("cr_outer_repo", "PathBuf"),
                ("cr_outer_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let mut imports: ImportResults = ctx
                    .get::<ImportResults>("cr_imports")
                    .cloned()
                    .unwrap_or_default();
                imports.push(import_baseline(&hearth));

                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set("cr_imports", imports);
                carry_fixture(&ctx, &mut out);
                Ok(out)
            },
        ),
        check_def(
            "the baseline tree contains hearth path {string}",
            &[("hearth_path", "PathBuf"), ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>")],
            |ctx, params| {
                let wanted = params.get_string(0).ok_or("Expected a path")?;
                let hearth = hearth_of(&ctx)?;
                let recorded = baseline_tree_paths(&hearth)
                    .map_err(|e| format!("could not read the baseline tree: {}", e))?;
                if recorded.iter().any(|p| p == wanted) {
                    return Ok(());
                }
                Err(format!(
                    "the baseline tree does not hold '{}'. It holds {} path(s): {}",
                    wanted,
                    recorded.len(),
                    recorded.join(", ")
                ))
            },
        ),
        check_def(
            "the baseline tree does not contain hearth path {string}",
            &[("hearth_path", "PathBuf"), ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>")],
            |ctx, params| {
                let forbidden = params.get_string(0).ok_or("Expected a path")?;
                let hearth = hearth_of(&ctx)?;
                let recorded = baseline_tree_paths(&hearth)
                    .map_err(|e| format!("could not read the baseline tree: {}", e))?;
                if recorded.iter().any(|p| p == forbidden) {
                    return Err(format!(
                        "the baseline tree holds '{}', which the path declaration excludes",
                        forbidden
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "replay from the baseline reproduces every recorded path byte-for-byte",
            &[("hearth_path", "PathBuf"), ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let (_handle, scratch) = retained_temp_dir("anvil-replay-")?;
                let replayed = replay_baseline(&hearth, &scratch)
                    .map_err(|e| format!("replay failed: {}", e))?;
                let mut expected = recorded_paths(&hearth)
                    .map_err(|e| format!("could not enumerate the recorded set: {}", e))?;
                expected.sort();
                let mut got = replayed.clone();
                got.sort();
                if expected != got {
                    return Err(format!(
                        "replay covered a different path set.\n  on disk: {:?}\n  replayed: {:?}",
                        expected, got
                    ));
                }
                if expected.is_empty() {
                    return Err(
                        "the recorded set is empty, so a byte-for-byte replay is vacuously green"
                            .to_string(),
                    );
                }
                for rel in &expected {
                    let on_disk = std::fs::read(hearth.join(rel))
                        .map_err(|e| format!("read {} from the hearth: {}", rel, e))?;
                    let from_baseline = std::fs::read(scratch.join(rel))
                        .map_err(|e| format!("read {} from the replay: {}", rel, e))?;
                    if on_disk != from_baseline {
                        return Err(format!(
                            "'{}' differs after replay.\n  on disk ({} bytes): {:?}\n  replayed \
                             ({} bytes): {:?}",
                            rel,
                            on_disk.len(),
                            String::from_utf8_lossy(&on_disk),
                            from_baseline.len(),
                            String::from_utf8_lossy(&from_baseline)
                        ));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "the repository has exactly {int} baseline ref",
            &[("hearth_path", "PathBuf"), ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")?;
                let hearth = hearth_of(&ctx)?;
                let listed = git(&hearth, &["for-each-ref", "--format=%(refname)", "refs/anvil/"])
                    .unwrap_or_default();
                let found: Vec<&str> = listed
                    .lines()
                    .map(str::trim)
                    .filter(|line| *line == BASELINE_REF)
                    .collect();
                if found.len() as i64 != expected {
                    return Err(format!(
                        "expected {} baseline ref(s), found {}. All anvil refs: [{}]",
                        expected,
                        found.len(),
                        listed.lines().collect::<Vec<_>>().join(", ")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the change-record ref has exactly {int} commit",
            &[("hearth_path", "PathBuf"), ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected a count")?;
                let hearth = hearth_of(&ctx)?;
                let listed = git(&hearth, &["rev-list", "--count", CHANGE_RECORD_REF])
                    .unwrap_or_else(|_| "0".to_string());
                let count: i64 = listed.trim().parse().unwrap_or(0);
                if count != expected {
                    return Err(format!(
                        "expected {} commit(s) on {}, found {}",
                        expected, CHANGE_RECORD_REF, count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            // Counting refs cannot catch an UNCONDITIONAL ref update: replacing
            // the baseline leaves exactly one ref and exactly one reachable
            // commit while the original becomes unreachable. This is the
            // assertion that fails when the create-only compare-and-swap is
            // dropped.
            "the baseline ref names the commit the first import established",
            &[
                ("hearth_path", "PathBuf"),
                ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>"),
            ],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let imports = ctx
                    .get::<ImportResults>("cr_imports")
                    .ok_or("No cr_imports")?;
                if imports.len() < 2 {
                    return Err(format!(
                        "this assertion needs at least two imports, saw {}",
                        imports.len()
                    ));
                }
                let first = imports[0]
                    .as_ref()
                    .map_err(|e| format!("the first import refused: {}", e))?;
                let second = imports[1]
                    .as_ref()
                    .map_err(|e| format!("the second import refused: {}", e))?;
                if !second.already_imported {
                    return Err(
                        "the second import reported that it established a baseline".to_string()
                    );
                }
                let live = git(&hearth, &["rev-parse", BASELINE_REF])?;
                if live.trim() != first.baseline {
                    return Err(format!(
                        "the baseline was replaced: {} established {}, but the ref now names {}",
                        BASELINE_REF,
                        first.baseline,
                        live.trim()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "HEAD, the index and the working tree are unchanged",
            &[("hearth_path", "PathBuf"), ("cr_pre_state", "WorktreeState")],
            |ctx, _params| {
                let hearth = hearth_of(&ctx)?;
                let before = ctx
                    .get::<WorktreeState>("cr_pre_state")
                    .ok_or("No cr_pre_state")?
                    .clone();
                let after = capture_worktree_state(&hearth)?;
                if before.head != after.head {
                    return Err(format!(
                        "HEAD moved: {} -> {}",
                        before.head, after.head
                    ));
                }
                if before.index != after.index {
                    return Err(format!(
                        "the index changed.\n  before:\n{}\n  after:\n{}",
                        before.index, after.index
                    ));
                }
                if before.status != after.status {
                    return Err(format!(
                        "the working tree status changed.\n  before:\n{}\n  after:\n{}",
                        before.status, after.status
                    ));
                }
                if before.files != after.files {
                    let before_names: Vec<&String> = before.files.iter().map(|(p, _)| p).collect();
                    let after_names: Vec<&String> = after.files.iter().map(|(p, _)| p).collect();
                    return Err(format!(
                        "the working tree bytes changed.\n  before: {:?}\n  after: {:?}",
                        before_names, after_names
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "baseline import refuses naming the enclosing repository",
            &[
                ("cr_imports", "Vec<Result<BaselineImport, BaselineImportError>>"),
                ("cr_outer_repo", "PathBuf"),
                ("hearth_path", "PathBuf"),
            ],
            |ctx, _params| {
                // Canonicalized because git answers `/private/var/…` where the
                // fixture holds `/var/…`; comparing the two raw would make the
                // assertion fail for a reason that is not the behavior.
                let outer_raw = ctx
                    .get::<PathBuf>("cr_outer_repo")
                    .ok_or("No cr_outer_repo")?
                    .clone();
                let outer = std::fs::canonicalize(&outer_raw)
                    .map_err(|e| format!("canonicalize {}: {}", outer_raw.display(), e))?;
                let result = last_import(&ctx)?;
                let refusal = match result {
                    Ok(import) => {
                        return Err(format!(
                            "expected a refusal, got a baseline at {} over {} path(s)",
                            import.baseline, import.paths_recorded
                        ))
                    }
                    Err(e) => e,
                };
                match refusal {
                    BaselineImportError::ForeignRepository { enclosing, .. } => {
                        let text = refusal.to_string();
                        if !text.contains(&outer.display().to_string())
                            || enclosing != &outer
                        {
                            return Err(format!(
                                "the refusal does not name the enclosing repository {}: {}",
                                outer.display(),
                                text
                            ));
                        }
                    }
                    other => {
                        return Err(format!(
                            "expected a foreign-repository refusal, got: {}",
                            other
                        ))
                    }
                }
                // A refusal that still committed into the human's repository is
                // the failure this scenario exists to catch.
                let anvil_refs =
                    git(&outer_raw, &["for-each-ref", "--format=%(refname)", "refs/anvil/"])
                        .unwrap_or_default();
                if !anvil_refs.trim().is_empty() {
                    return Err(format!(
                        "the enclosing repository gained anvil refs despite the refusal: {}",
                        anvil_refs.trim()
                    ));
                }
                let head_commits = git(&outer_raw, &["rev-list", "--count", "HEAD"])
                    .unwrap_or_else(|_| "0".to_string());
                if head_commits.trim() != "1" {
                    return Err(format!(
                        "the enclosing repository's HEAD moved: expected 1 commit, found {}",
                        head_commits.trim()
                    ));
                }
                Ok(())
            },
        ),
    ]
}
