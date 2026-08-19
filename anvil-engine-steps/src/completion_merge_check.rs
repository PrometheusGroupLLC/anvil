//! Real-binary steps for `completion_merge_check.feature`.
//!
//! The fixture is deliberately NOT a stub: it builds two REAL git repositories
//! in a temp root — the hearth, and a sibling `codeapp` carrying a real
//! `refs/remotes/origin/main` plus a real commit on a branch that never reached
//! it — and drives the REAL `anvil-hooks complete` binary against a REAL engine
//! process. A merge check proved against a fake git would prove nothing about
//! the one command the rule is written in.
//!
//! The sibling layout is the mechanism under test as much as the ancestry is:
//! the claim record lives in the hearth, the code lives elsewhere, and the
//! resolver has to cross that gap. `<repo>@<ref>` is how it is told to.

use anvil_core_hearth::fs_snapshot_adapter::FileSystemSnapshotAdapter;
use anvil_core::ports::snapshot_port::SnapshotPort;
use anvil_test_support::engine::EngineProcess;
use anvil_test_support::{retained_temp_dir, RetainedTempDir};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const KIND: &str = "merge_probe";
const PLAYBOOK_ID: &str = "merge_check_probe";
const ARTIFACT_PATH: &str = "merge_runs/mc-probe";
const ACTOR: &str = "MergeCheck-100000";
const CODE_REPO: &str = "codeapp";
const UNMERGED_BRANCH: &str = "example-org/never-landed";

/// The two real SHAs the scenarios refer to by token.
///
/// Persisted INTO the hearth rather than carried in the brine context: the
/// shared `the engine is started with that hearth` step carries only the keys
/// it declares, so a fixture key of ours is dropped between the Given and the
/// When. The file lives under the hearth, which every later step already holds.
#[derive(Clone, Debug)]
struct GitFixture {
    merged_sha: String,
    unmerged_sha: String,
}

const FIXTURE_FILE: &str = ".merge-check-fixture";

fn write_fixture(hearth: &Path, fixture: &GitFixture) -> Result<(), String> {
    std::fs::write(
        hearth.join(FIXTURE_FILE),
        format!("{}\n{}\n", fixture.merged_sha, fixture.unmerged_sha),
    )
    .map_err(|e| format!("write fixture shas: {}", e))
}

fn read_fixture(hearth: &Path) -> Result<GitFixture, String> {
    let raw = std::fs::read_to_string(hearth.join(FIXTURE_FILE))
        .map_err(|e| format!("read fixture shas: {}", e))?;
    let mut lines = raw.lines();
    let merged_sha = lines.next().ok_or("fixture file has no merged sha")?;
    let unmerged_sha = lines.next().ok_or("fixture file has no unmerged sha")?;
    Ok(GitFixture {
        merged_sha: merged_sha.to_string(),
        unmerged_sha: unmerged_sha.to_string(),
    })
}

#[derive(Clone, Debug)]
struct CompleteOutcome {
    fixture: GitFixture,
    exit: i32,
    output: String,
    resolved_state: String,
    artifact_after: BTreeMap<PathBuf, Vec<u8>>,
    artifact_before: BTreeMap<PathBuf, Vec<u8>>,
}

fn hooks_bin() -> PathBuf {
    anvil_test_support::harness::ensure_binary("anvil-hooks");
    anvil_test_support::harness::binary_path("anvil-hooks")
}

/// Run git with a hermetic identity so the fixture never depends on — or
/// touches — the developer's git config.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Merge Check Fixture",
            "-c",
            "user.email=fixture@anvil.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .map_err(|e| format!("git {:?}: {}", args, e))?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} in {} failed ({:?}): {}{}",
            args,
            dir.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_init(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {}", dir.display(), e))?;
    let output = Command::new("git")
        .arg("-c")
        .arg("init.defaultBranch=main")
        .arg("init")
        .arg(dir)
        .output()
        .map_err(|e| format!("git init {}: {}", dir.display(), e))?;
    if !output.status.success() {
        return Err(format!(
            "git init {} failed: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Build `<root>/codeapp`: one commit that IS `origin/main`, and one commit on
/// a branch that `origin/main` has never seen.
fn seed_code_repo(root: &Path) -> Result<GitFixture, String> {
    let repo = root.join(CODE_REPO);
    git_init(&repo)?;
    std::fs::create_dir_all(repo.join("src")).map_err(|e| format!("create src: {}", e))?;
    std::fs::write(repo.join("src/shipped.rs"), "// landed on main\n")
        .map_err(|e| format!("write shipped.rs: {}", e))?;
    git(&repo, &["add", "-A"])?;
    git(&repo, &["commit", "-m", "landed on main"])?;
    let merged_sha = git(&repo, &["rev-parse", "HEAD"])?;
    // A real remote-tracking ref, written the way a fetch writes it. No network.
    git(
        &repo,
        &["update-ref", "refs/remotes/origin/main", &merged_sha],
    )?;
    // The stranded work: a branch commit `origin/main` does not contain.
    git(&repo, &["checkout", "-b", UNMERGED_BRANCH])?;
    std::fs::write(repo.join("src/stranded.rs"), "// never merged\n")
        .map_err(|e| format!("write stranded.rs: {}", e))?;
    git(&repo, &["add", "-A"])?;
    git(&repo, &["commit", "-m", "stranded on a branch"])?;
    let unmerged_sha = git(&repo, &["rev-parse", "HEAD"])?;
    Ok(GitFixture {
        merged_sha,
        unmerged_sha,
    })
}

/// The probe playbook: `spec -> spec_review -> completed`, DRIVEN, no evidence
/// obligation. The merge check must not need an obligation to arm — it judges
/// what was CLAIMED, and the obligation lever is a separate one.
fn machine_yaml() -> String {
    r#"kind: merge_probe
directory: merge_runs
registry: merge_runs.md
description: "Completion merge-check probe."
register: driven
required_fields: []
roles:
  - doer
  - reviewer
states:
  - name: spec
    role_filters: []
    registry_section: spec
    projection_targets: []
    is_review_gate: false
    is_terminal: false
    measurement_by_role:
      doer:
        intent: "Produce the probe specification."
        expected_output: "A durable probe specification."
  - name: spec_review
    role_filters: []
    registry_section: spec_review
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    measurement_by_role:
      reviewer:
        intent: "Review the probe specification."
        expected_output: "A durable probe review."
  - name: completed
    role_filters: []
    registry_section: completed
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: spec
    to_state: spec_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: spec_review
    to_state: completed
    required_role: reviewer
    required_satisfaction:
      - satisfied
    requires_approver: false
outcome_predicate:
  terminal_state: completed
"#
    .to_string()
}

/// Build `<root>/hearth` as a REAL git repository so an unqualified claim has a
/// work tree to resolve against, and so the resolver's default search root is
/// `<root>` — where `codeapp` lives.
fn seed_hearth(root: &Path, state: &str) -> Result<PathBuf, String> {
    let hearth = root.join("hearth");
    git_init(&hearth)?;
    std::fs::create_dir_all(hearth.join("playbooks").join(PLAYBOOK_ID))
        .map_err(|e| format!("create playbook dir: {}", e))?;
    std::fs::write(
        hearth.join("playbooks").join(PLAYBOOK_ID).join("machine.yaml"),
        machine_yaml(),
    )
    .map_err(|e| format!("write machine.yaml: {}", e))?;
    std::fs::create_dir_all(hearth.join("merge_runs"))
        .map_err(|e| format!("create merge_runs: {}", e))?;
    std::fs::write(
        hearth.join("merge_runs.md"),
        "# Merge Runs\n\n## spec\n\n## spec_review\n\n## completed\n",
    )
    .map_err(|e| format!("write registry: {}", e))?;
    std::fs::create_dir_all(hearth.join("tracks")).map_err(|e| format!("create tracks: {}", e))?;
    std::fs::write(hearth.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("write tracks.md: {}", e))?;

    let artifact = hearth.join(ARTIFACT_PATH);
    std::fs::create_dir_all(&artifact).map_err(|e| format!("create artifact: {}", e))?;
    std::fs::write(
        artifact.join("status.yaml"),
        format!(
            "version: 1\nkind: {kind}\nstate: {state}\ntransitions:\n  - to: {state}\n    at: \"2026-08-09T00:00:00Z\"\n    actor: Seed-MC-000001\n    role: doer\n",
            kind = KIND,
            state = state,
        ),
    )
    .map_err(|e| format!("write status.yaml: {}", e))?;
    std::fs::write(artifact.join("spec.md"), "# Merge check probe\n")
        .map_err(|e| format!("write spec.md: {}", e))?;
    Ok(hearth)
}

/// Snapshot every file under the artifact directory, so "unchanged" is proved
/// by BYTES rather than by the state header alone.
fn snapshot_artifact(hearth: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let root = hearth.join(ARTIFACT_PATH);
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(rel) = path.strip_prefix(&root) {
                    out.insert(rel.to_path_buf(), bytes);
                }
            }
        }
    }
    out
}

/// `UNMERGED_SHA` must be substituted FIRST: `MERGED_SHA` is a substring of it,
/// and doing it the other way round silently produced `UN<merged-sha>` — a
/// rev-spec that simply does not exist, so the scenario refused for the wrong
/// reason and still looked like a refusal.
fn substitute(claim: &str, fixture: &GitFixture) -> String {
    claim
        .replace("UNMERGED_SHA", &fixture.unmerged_sha)
        .replace("MERGED_SHA", &fixture.merged_sha)
}

fn run_complete(mut ctx: Context, extra: &[String]) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let port = engine.port;
    let hearth = ctx
        .get::<PathBuf>("hearth_path")
        .cloned()
        .ok_or("No hearth_path")?;
    let fixture = read_fixture(&hearth)?;

    let artifact_before = snapshot_artifact(&hearth);

    let mut cmd = Command::new(hooks_bin());
    cmd.arg("complete")
        .arg("--artifact-path")
        .arg(ARTIFACT_PATH)
        .arg("--actor-name")
        .arg(ACTOR)
        .arg("--actor-type")
        .arg("agent")
        .arg("--actor-model")
        .arg("brine")
        .arg("--actor-provider")
        .arg("test")
        .arg("--hearth")
        .arg(hearth.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string());
    for arg in extra {
        cmd.arg(arg);
    }
    let output = cmd.output().map_err(|e| format!("run complete: {}", e))?;
    let exit = output.status.code().unwrap_or(-1);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let resolved_state = FileSystemSnapshotAdapter::new(hearth.clone())
        .read_artifact_state(ARTIFACT_PATH)
        .unwrap_or_default();
    let artifact_after = snapshot_artifact(&hearth);

    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    anvil_test_support::carry_retained_temp_dir(&ctx, &mut out, "hearth_path_handle");
    out.set(
        "merge_check_outcome",
        CompleteOutcome {
            fixture,
            exit,
            output: combined,
            resolved_state,
            artifact_after,
            artifact_before,
        },
    );
    Ok(out)
}

fn outcome(ctx: &Context) -> Result<&CompleteOutcome, String> {
    ctx.get::<CompleteOutcome>("merge_check_outcome")
        .ok_or_else(|| "No merge_check_outcome".to_string())
}

fn claim_args(ctx: &Context, claim: &str) -> Result<Vec<String>, String> {
    let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
    let fixture = read_fixture(hearth)?;
    Ok(vec![
        "--claimed-evidence".to_string(),
        substitute(claim, &fixture),
    ])
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a completion merge-check hearth with the probe in state {string}",
            &[],
            &[
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected probe state")?;
                let (handle, root) = retained_temp_dir("anvil-merge-check-")?;
                let fixture = seed_code_repo(&root)?;
                let hearth = seed_hearth(&root, &state)?;
                write_fixture(&hearth, &fixture)?;
                let mut out = Context::new();
                out.set("hearth_path", hearth);
                out.set::<RetainedTempDir>("hearth_path_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "the reviewer completes the probe satisfied claiming {string}",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("merge_check_outcome", "CompleteOutcome"),
            ],
            |ctx, params| {
                let claim = params.get_string(0).ok_or("Expected claim")?;
                let mut args = vec!["--satisfaction".to_string(), "satisfied".to_string()];
                args.extend(claim_args(&ctx, &claim)?);
                run_complete(ctx, &args)
            },
        ),
        step_def(
            "the reviewer completes the probe satisfied presenting no claims",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("merge_check_outcome", "CompleteOutcome"),
            ],
            |ctx, _params| {
                run_complete(
                    ctx,
                    &["--satisfaction".to_string(), "satisfied".to_string()],
                )
            },
        ),
        step_def(
            "the doer completes the probe claiming {string}",
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                ("engine_process", "EngineProcess"),
                ("hearth_path", "PathBuf"),
                ("hearth_path_handle", "Arc<Mutex<Option<TempDir>>>"),
                ("merge_check_outcome", "CompleteOutcome"),
            ],
            |ctx, params| {
                let claim = params.get_string(0).ok_or("Expected claim")?;
                let args = claim_args(&ctx, &claim)?;
                run_complete(ctx, &args)
            },
        ),
        check_def(
            "the completion is refused",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.exit == 0 {
                    return Err(format!(
                        "expected the completion to be REFUSED, but anvil-hooks exited 0. \
                         Output: {}",
                        o.output
                    ));
                }
                if !o.output.contains("claimed_evidence_not_on_origin_main") {
                    return Err(format!(
                        "refused, but not by the merge check — no \
                         `claimed_evidence_not_on_origin_main` token in: {}",
                        o.output
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the completion succeeds",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.exit != 0 {
                    return Err(format!(
                        "expected the completion to SUCCEED, exit was {}. Output: {}",
                        o.exit, o.output
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal names the unmerged commit, the repository, and the branch it is on",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                let fixture = &o.fixture;
                let mut missing = Vec::new();
                if !o.output.contains(&fixture.unmerged_sha) {
                    missing.push("the unmerged SHA");
                }
                if !o.output.contains(CODE_REPO) {
                    missing.push("the repository");
                }
                if !o.output.contains(UNMERGED_BRANCH) {
                    missing.push("the branch it IS on");
                }
                if !o.output.contains(&fixture.merged_sha) {
                    missing.push("the origin/main SHA it compared against");
                }
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "refusal message omitted {}. Message: {}",
                        missing.join(", "),
                        o.output
                    ))
                }
            },
        ),
        check_def(
            "the refusal names the path it could not find",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.output.contains("never_written.rs") && o.output.contains("does not exist") {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the refusal to name the missing path; got: {}",
                        o.output
                    ))
                }
            },
        ),
        check_def(
            "the refusal names the repository it could not locate",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.output.contains("nosuchrepo") && o.output.contains("could not be located") {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the refusal to name the unlocatable repository; got: {}",
                        o.output
                    ))
                }
            },
        ),
        check_def(
            "the probe artifact rests in state {string}",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state")?;
                let o = outcome(&ctx)?;
                if o.resolved_state == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected the probe to rest in '{}', found '{}'. Output: {}",
                        expected, o.resolved_state, o.output
                    ))
                }
            },
        ),
        check_def(
            "the probe artifact directory is byte-for-byte unchanged",
            &[("merge_check_outcome", "CompleteOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.artifact_before == o.artifact_after {
                    return Ok(());
                }
                let mut diffs = Vec::new();
                for (path, before) in &o.artifact_before {
                    match o.artifact_after.get(path) {
                        None => diffs.push(format!("{} was DELETED", path.display())),
                        Some(after) if after != before => {
                            diffs.push(format!("{} was REWRITTEN", path.display()))
                        }
                        Some(_) => {}
                    }
                }
                for path in o.artifact_after.keys() {
                    if !o.artifact_before.contains_key(path) {
                        diffs.push(format!("{} was CREATED", path.display()));
                    }
                }
                Err(format!(
                    "a refused completion mutated the artifact: {}",
                    diffs.join("; ")
                ))
            },
        ),
    ]
}
