//! Step module + fixtures for the playbook-builder (`playbook_generation`)
//! machine — the satisfaction-discriminated rewrite of
//! `anvil-hearth/workflows/20260528T2321_workflow_generation/machine.yaml`.
//!
//! Single source of truth: the builder machine.yaml is READ AT TEST TIME from
//! the real hearth file via a `CARGO_MANIFEST_DIR`-relative path, so editing the
//! hearth file automatically propagates to every brine fixture with no sync step
//! (plan M-A). Referenced hook bodies are read the same way.
//!
//! These fixtures mirror the knowledge_lifecycle hearth-copy pattern
//! (`query_port::seed_knowledge_lifecycle_hearth`) but add what the builder needs
//! that knowledge does not: physical hook files (the loader drops a machine
//! whose hook reference dangles), an ACTIVE parent track (the builder's
//! `parent_kind: track` makes begin enforce parent exists + active + kind
//! track), and the `workflow_generations/` directory + registry.

use crate::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core::domain::begin::{BeginCommandHandler, BeginError, BeginOutcome, BeginRequest};
use anvil_core::domain::events::Event;
use anvil_core::domain::playbook::composite_registry::CompositePlaybookRegistry;
use anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry;
use anvil_core::domain::playbook::registry::{PlaybookRegistry, SeedPlaybookRegistry};
use anvil_core_hearth::fs_query_adapter::FileSystemQueryAdapter;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::{Path, PathBuf};

/// The conventional playbook id directory for the builder machine. This is the
/// playbook DEFINITION id (the registry's `playbook_id_for(kind)`), distinct from
/// the per-run instance id that step_measurement records now carry in
/// `playbook_id` (H1).
pub const BUILDER_PLAYBOOK_ID: &str = "20260528T2321_workflow_generation";

/// The builder playbook KIND — the value step_measurement records carry in
/// `track_id` (the cross-instance Scorecard B aggregation key) after H1.
pub const BUILDER_KIND: &str = "playbook_generation";

/// Read the builder machine.yaml from the real hearth at test time. The hearth
/// lives as a sibling of the anvil repo; from the `anvil-test-support` crate root
/// (`CARGO_MANIFEST_DIR`) the path is `../../anvil-hearth/workflows/<id>/machine.yaml`.
/// In the worktree layout this resolves through the build-enabling sibling symlink
/// (`.claude/worktrees/anvil-hearth`), exactly like the `../../brine` path-deps.
pub fn builder_machine_yaml() -> String {
    let path = builder_hearth_dir().join("machine.yaml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read builder machine.yaml from real hearth at {}: {}",
            path.display(),
            e
        )
    })
}

/// Read the builder gathering hook body from the real hearth at test time.
pub fn builder_gathering_hook() -> String {
    builder_hook("gathering.md")
}

fn builder_hook(filename: &str) -> String {
    let path = builder_hearth_dir().join("hooks").join(filename);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read builder hook from real hearth at {}: {}",
            path.display(),
            e
        )
    })
}

fn builder_hearth_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut bases = vec![
        manifest.join("..").join("..").join("anvil-hearth"),
        manifest
            .join("..")
            .join("..")
            .join("..")
            .join("anvil-hearth"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        bases.push(PathBuf::from(home).join("Development").join("anvil-hearth"));
    }
    for base in bases {
        // Canonical first; the legacy workflows/ lookup is DATA tolerance for a
        // live hearth that has not yet been touched by the one-time migration.
        let canonical = base.join("playbooks").join(BUILDER_PLAYBOOK_ID);
        if canonical.exists() {
            return canonical;
        }
        let legacy = base.join("workflows").join(BUILDER_PLAYBOOK_ID);
        if legacy.exists() {
            return legacy;
        }
    }
    manifest
        .join("..")
        .join("..")
        .join("anvil-hearth")
        .join("playbooks")
        .join(BUILDER_PLAYBOOK_ID)
}

/// Seed a temp fs hearth that physically holds the builder machine.yaml +
/// referenced hooks under workflows/<id>/, plus the workflow_generations/
/// directory + registry. When `parent` is Some, also seed an ACTIVE parent
/// track under tracks/<parent>/status.yaml (kind: track, state: active) so a
/// builder begin-create passes the parent-active enforcement.
pub fn seed_builder_hearth(tmp: &Path, parent: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(tmp).map_err(|e| format!("Failed to create hearth: {}", e))?;

    // Structural hearth signature.
    std::fs::create_dir_all(tmp.join("tracks"))
        .map_err(|e| format!("Failed to create tracks dir: {}", e))?;
    std::fs::write(tmp.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("Failed to write tracks.md: {}", e))?;

    // The builder's own directory + registry so the first-complete registry
    // append (and create) lands.
    std::fs::create_dir_all(tmp.join("workflow_generations"))
        .map_err(|e| format!("Failed to create workflow_generations dir: {}", e))?;
    std::fs::write(
        tmp.join("workflow_generations.md"),
        "# Playbook Generations\n",
    )
    .map_err(|e| format!("Failed to write workflow_generations.md: {}", e))?;

    // The builder playbook on disk: machine.yaml + referenced hooks (the loader
    // validates referenced hook files exist, else it drops the machine).
    let wf_dir = tmp.join("playbooks").join(BUILDER_PLAYBOOK_ID);
    let hooks_dir = wf_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("Failed to create builder hooks dir: {}", e))?;
    let machine_yaml = builder_machine_yaml()
        .replacen("parent_kind: track\n", "parent_kind: track\nparent_required: false\n", 1)
        .replace(
            "  - name: parent_id\n    field_type: artifact_id\n    description: Parent track id (in any hearth — engine does not enforce hearth boundaries on parent_id)\n",
            "",
        );
    std::fs::write(wf_dir.join("machine.yaml"), machine_yaml)
        .map_err(|e| format!("Failed to write builder machine.yaml: {}", e))?;
    for hook in ["gathering.md", "modeling.md"] {
        std::fs::write(hooks_dir.join(hook), builder_hook(hook))
            .map_err(|e| format!("Failed to write builder hooks/{}: {}", hook, e))?;
    }

    if let Some(parent_id) = parent {
        let parent_dir = tmp.join("tracks").join(parent_id);
        std::fs::create_dir_all(&parent_dir)
            .map_err(|e| format!("Failed to create parent track dir: {}", e))?;
        // Realistic parent: a track's working state is `implementing` (it has no
        // `active` state). This makes the playbook_generation-under-track scenarios
        // exercise the real condition — and is the regression guard for the
        // kind-aware parent-eligibility fix (pre-fix: implementing → ParentNotActive).
        std::fs::write(
            parent_dir.join("status.yaml"),
            "version: 1\nkind: track\nstate: implementing\n",
        )
        .map_err(|e| format!("Failed to write parent track status.yaml: {}", e))?;
    }

    Ok(())
}

/// Write a builder artifact into `workflow_generations/<id>/status.yaml` in the
/// requested state, with a prior transition + a registered actor so the snapshot
/// read resolves (mirrors the knowledge artifact seed in complete.rs).
pub fn seed_builder_artifact(tmp: &Path, id: &str, state: &str) -> Result<(), String> {
    let art_dir = tmp.join("workflow_generations").join(id);
    std::fs::create_dir_all(&art_dir)
        .map_err(|e| format!("Failed to create builder artifact dir: {}", e))?;
    let status = format!(
        "version: 1\nkind: playbook_generation\nstate: {state}\nactors:\n  Seed-000000:\n    type: agent\n    configurations:\n      - at: \"2026-06-01T00:00:00Z\"\n        model: claude-opus-4-8\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: \"\"\n          entrypoint: claude-code\ntransitions:\n  - to: {state}\n    at: 2026-06-01T00:00:00Z\n    actor: Seed-000000\n    role: doer\n",
        state = state
    );
    std::fs::write(art_dir.join("status.yaml"), status)
        .map_err(|e| format!("Failed to write builder artifact status.yaml: {}", e))?;
    Ok(())
}

fn fresh_builder_hearth(tag: &str) -> Result<(RetainedTempDir, PathBuf), String> {
    retained_temp_dir(&format!("anvil-builder-{}-", tag))
}

/// Context key: the load-error count after registry construction.
const BUILDER_LOAD_ERROR_COUNT_KEY: &str = "builder_load_error_count";
/// Context key: whether the builder machine resolved.
const BUILDER_RESOLVED_KEY: &str = "builder_resolved";
/// Context key: the builder hearth path used by the AC1 registry steps.
const BUILDER_HEARTH_KEY: &str = "builder_hearth";
/// Context key: retained TempDir for the builder hearth path.
const BUILDER_HEARTH_HANDLE_KEY: &str = "builder_hearth_handle";

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== AC1: the rewritten builder machine loads + validates =====
        step_def(
            "a hearth seeded with the builder machine",
            &[],
            &[
                (BUILDER_HEARTH_KEY, "PathBuf"),
                (BUILDER_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, _params| {
                let (handle, tmp) = fresh_builder_hearth("load")?;
                seed_builder_hearth(&tmp, None)?;
                let mut out = Context::new();
                out.set(BUILDER_HEARTH_KEY, tmp);
                out.set(BUILDER_HEARTH_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "the builder hearth registry is constructed",
            &[(BUILDER_HEARTH_KEY, "PathBuf")],
            &[
                (BUILDER_HEARTH_KEY, "PathBuf"),
                (BUILDER_HEARTH_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (BUILDER_LOAD_ERROR_COUNT_KEY, "usize"),
                (BUILDER_RESOLVED_KEY, "bool"),
            ],
            |ctx, _params| {
                let hearth = ctx
                    .get::<PathBuf>(BUILDER_HEARTH_KEY)
                    .ok_or("No builder_hearth")?
                    .clone();
                let registry = HearthPlaybookRegistry::new(hearth.clone());
                let error_count = registry.invalid_artifacts().len();
                let resolved = registry.machine_for("playbook_generation").is_some();
                let mut out = Context::new();
                out.set(BUILDER_HEARTH_KEY, hearth);
                carry_retained_temp_dir(&ctx, &mut out, BUILDER_HEARTH_HANDLE_KEY);
                out.set(BUILDER_LOAD_ERROR_COUNT_KEY, error_count);
                out.set(BUILDER_RESOLVED_KEY, resolved);
                Ok(out)
            },
        ),
        check_def(
            "the builder hearth registry has no load errors",
            &[(BUILDER_LOAD_ERROR_COUNT_KEY, "usize")],
            |ctx, _params| {
                let count = ctx
                    .get::<usize>(BUILDER_LOAD_ERROR_COUNT_KEY)
                    .ok_or("No builder_load_error_count")?;
                if *count == 0 {
                    Ok(())
                } else {
                    Err(format!("Expected no load errors but found {}", count))
                }
            },
        ),
        check_def(
            "the builder hearth registry resolves {string} to a machine",
            &[(BUILDER_RESOLVED_KEY, "bool")],
            |ctx, _params| {
                let resolved = ctx
                    .get::<bool>(BUILDER_RESOLVED_KEY)
                    .ok_or("No builder_resolved")?;
                if *resolved {
                    Ok(())
                } else {
                    Err("Expected playbook_generation to resolve to a machine but it was dropped (silent-drop trap)".to_string())
                }
            },
        ),
        // ===== AC2/AC3: seed a complete fs hearth with a builder artifact in a
        // given state, so the shared `complete fs is executed with:` step drives
        // it through the real composite registry + fs adapters. =====
        step_def(
            "a complete fs hearth with a builder artifact {string} in state {string}",
            &[],
            &[
                ("fs_hearth", "PathBuf"),
                ("fs_hearth_handle", "Arc<Mutex<Option<TempDir>>>"),
            ],
            |_ctx, params| {
                let id = params.get_string(0).ok_or("Expected id")?.to_string();
                let state = params.get_string(1).ok_or("Expected state")?.to_string();
                let (handle, tmp) = fresh_builder_hearth("complete")?;
                seed_builder_hearth(&tmp, None)?;
                seed_builder_artifact(&tmp, &id, &state)?;
                let mut out = Context::new();
                out.set("fs_hearth", tmp);
                out.set("fs_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "a builder hearth with parent track {string}",
            &[],
            &[
                ("composite_begin_hearth", "PathBuf"),
                (
                    "composite_begin_hearth_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
            ],
            |_ctx, params| {
                let parent = params.get_string(0).ok_or("Expected parent")?.to_string();
                let (handle, tmp) = fresh_builder_hearth("begin")?;
                seed_builder_hearth(&tmp, Some(&parent))?;
                let mut out = Context::new();
                out.set("composite_begin_hearth", tmp);
                out.set("composite_begin_hearth_handle", handle);
                Ok(out)
            },
        ),
        step_def(
            "begin is called for playbook_generation {string} target_owner {string}",
            &[("composite_begin_hearth", "PathBuf")],
            &[
                ("composite_begin_hearth", "PathBuf"),
                (
                    "composite_begin_hearth_handle",
                    "Arc<Mutex<Option<TempDir>>>",
                ),
                ("begin_outcome", "Result<BeginOutcome, BeginError>"),
            ],
            |ctx, params| {
                let playbook_name = params
                    .get_string(0)
                    .ok_or("Expected playbook_name")?
                    .to_string();
                let target_owner = params
                    .get_string(1)
                    .ok_or("Expected target_owner")?
                    .to_string();
                let hearth = ctx
                    .get::<PathBuf>("composite_begin_hearth")
                    .ok_or("No composite_begin_hearth")?
                    .clone();
                let registry = CompositePlaybookRegistry::new(
                    HearthPlaybookRegistry::new(hearth.clone()),
                    SeedPlaybookRegistry,
                );
                let query = FileSystemQueryAdapter::new(hearth.clone());
                let request = BeginRequest {
                    artifact_type: BUILDER_KIND.to_string(),
                    parent_id: "20260419T1336_parent".to_string(),
                    track_name: playbook_name.clone(),
                    playbook_name: playbook_name.clone(),
                    target_owner,
                    fields: std::collections::BTreeMap::from([(
                        "playbook_name".to_string(),
                        playbook_name,
                    )]),
                    approver: "mark".to_string(),
                    actor_name: "Test-000000".to_string(),
                    actor_type: "agent".to_string(),
                    actor_model: "test-model".to_string(),
                    actor_provider: "test".to_string(),
                    ..Default::default()
                };
                let outcome = BeginCommandHandler::execute(&query, &registry, request);
                let mut out = Context::new();
                out.set("composite_begin_hearth", hearth);
                carry_retained_temp_dir(&ctx, &mut out, "composite_begin_hearth_handle");
                out.set("begin_outcome", outcome);
                Ok(out)
            },
        ),
        check_def(
            "the ArtifactCreation scaffold files include {string}",
            &[("begin_outcome", "Result<BeginOutcome, BeginError>")],
            |ctx, params| {
                let expected = params
                    .get_string(0)
                    .ok_or("Expected comma-separated filenames")?;
                let expected: Vec<&str> = expected.split(',').map(|s| s.trim()).collect();
                let outcome = ctx
                    .get::<Result<BeginOutcome, BeginError>>("begin_outcome")
                    .ok_or("No begin_outcome")?;
                match outcome {
                    Ok(o) => {
                        let files = o.events.iter().find_map(|event| {
                            if let Event::ArtifactCreation { scaffold_files, .. } = event {
                                Some(
                                    scaffold_files
                                        .iter()
                                        .map(|(name, _)| name.as_str())
                                        .collect::<Vec<_>>(),
                                )
                            } else {
                                None
                            }
                        });
                        match files {
                            Some(files) => {
                                let missing: Vec<&str> = expected
                                    .into_iter()
                                    .filter(|name| !files.contains(name))
                                    .collect();
                                if missing.is_empty() {
                                    Ok(())
                                } else {
                                    Err(format!(
                                        "Missing scaffold files {:?}; actual {:?}",
                                        missing, files
                                    ))
                                }
                            }
                            None => Err(format!("No ArtifactCreation event in {:?}", o.events)),
                        }
                    }
                    Err(e) => Err(format!("Expected success, got error: {}", e)),
                }
            },
        ),
    ]
}
