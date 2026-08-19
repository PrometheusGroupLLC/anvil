//! Step module for `hearth_discovery.feature`.
//!
//! Exercises `anvil_core_hearth::hearth_discovery::discover_hearths` over a
//! real on-disk fixture: a permitted root containing several sub-hearths (each
//! seeded with an `activity-log.jsonl` sink) plus an ordinary non-hearth
//! subdirectory. Asserts the discovered set, dedup behavior, explicit-hearth
//! inclusion, and non-hearth exclusion.

use anvil_test_support::{carry_retained_temp_dir, retained_temp_dir, RetainedTempDir};
use anvil_core_hearth::hearth_discovery::discover_hearths;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const ROOT_KEY: &str = "hd_root";
const ROOT_HANDLE_KEY: &str = "hd_root_handle";
const EXPLICIT_KEY: &str = "hd_explicit";
const HAS_EXPLICIT_KEY: &str = "hd_has_explicit";
const DISCOVERED_KEY: &str = "hd_discovered";

/// Mark a directory as a GENUINE hearth: it must hold the artifact store
/// directly — a real `tracks/` dir AND a `tracks.md` registry — the same
/// predicate `resolve_hearth` gates on. (A durable sink alone no longer
/// qualifies a dir as a hearth.)
fn make_hearth(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("tracks"))
        .map_err(|e| format!("create hearth tracks/ {}: {}", dir.display(), e))?;
    std::fs::write(dir.join("tracks.md"), "# Tracks\n")
        .map_err(|e| format!("seed tracks.md in {}: {}", dir.display(), e))?;
    // A hearth may also have recorded activity; seed the sink too so the fixture
    // resembles a live hearth, but the sink alone is NOT what qualifies it.
    std::fs::write(
        dir.join(anvil_core_hearth::fs_activity_log_adapter::SINK_FILENAME),
        "{}\n",
    )
    .map_err(|e| format!("seed sink in {}: {}", dir.display(), e))
}

/// Make a dir that LOOKS like a code/project repo, not a hearth: it carries a
/// `.hearth` POINTER file (the hearth path lives inside) and a `forge/` SYMLINK
/// to its hearth — but NO real `tracks/` dir. Discovery must NOT treat it as a
/// hearth (and must not follow the symlink to double-fold the pointed-at hearth).
fn make_code_repo(dir: &std::path::Path, hearth_target: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create code repo {}: {}", dir.display(), e))?;
    std::fs::write(
        dir.join(".hearth"),
        format!("{}\n", hearth_target.display()),
    )
    .map_err(|e| format!("seed .hearth pointer in {}: {}", dir.display(), e))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(hearth_target, dir.join("forge"))
        .map_err(|e| format!("symlink forge -> hearth in {}: {}", dir.display(), e))?;
    Ok(())
}

/// Make a dir holding ONLY a bare durable sink (no `tracks/`, no `tracks.md`).
/// A sink alone must NOT qualify a dir as a hearth.
fn make_bare_sink(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create bare-sink {}: {}", dir.display(), e))?;
    std::fs::write(
        dir.join(anvil_core_hearth::fs_activity_log_adapter::SINK_FILENAME),
        "{}\n",
    )
    .map_err(|e| format!("seed bare sink in {}: {}", dir.display(), e))
}

fn discovered_names(ctx: &Context) -> Result<Vec<String>, String> {
    let discovered = ctx
        .get::<Vec<PathBuf>>(DISCOVERED_KEY)
        .ok_or("No discovered hearths in context")?;
    Ok(discovered
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect())
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a permitted root containing sub-hearths {string} and a non-hearth dir {string}",
            &[],
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            |_ctx, params| {
                let hearth_list = params.get_string(0).ok_or("Expected sub-hearth list")?;
                let non_hearth = params.get_string(1).ok_or("Expected non-hearth dir name")?;
                let (handle, root) = retained_temp_dir("hd_root")?;
                for name in hearth_list
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    make_hearth(&root.join(name))?;
                }
                // Ordinary subdirectory with no hearth markers.
                std::fs::create_dir_all(root.join(non_hearth.trim()))
                    .map_err(|e| format!("create non-hearth dir: {}", e))?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set::<RetainedTempDir>(ROOT_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a permitted root with a genuine hearth {string}, a code-repo dir {string} pointing at it, and a bare-sink dir {string}",
            &[],
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            |_ctx, params| {
                let hearth_name = params.get_string(0).ok_or("Expected hearth name")?;
                let repo_name = params.get_string(1).ok_or("Expected code-repo name")?;
                let sink_name = params.get_string(2).ok_or("Expected bare-sink name")?;
                let (handle, root) = retained_temp_dir("hd_root")?;
                // The genuine hearth holds tracks/ + tracks.md directly.
                let hearth = root.join(hearth_name.trim());
                make_hearth(&hearth)?;
                // The code repo points at the SAME hearth via .hearth + a forge
                // symlink, but has no real tracks/ of its own.
                make_code_repo(&root.join(repo_name.trim()), &hearth)?;
                // A dir with only a bare sink, no artifact store.
                make_bare_sink(&root.join(sink_name.trim()))?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set::<RetainedTempDir>(ROOT_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "a permitted root that is itself a hearth with no sub-hearths",
            &[],
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            |_ctx, _params| {
                let (handle, root) = retained_temp_dir("hd_root")?;
                make_hearth(&root)?;
                let mut out = Context::new();
                out.set(ROOT_KEY, root);
                out.set::<RetainedTempDir>(ROOT_HANDLE_KEY, handle);
                Ok(out)
            },
        ),
        step_def(
            "an explicit default hearth {string}",
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (EXPLICIT_KEY, "PathBuf"),
                (EXPLICIT_HANDLE_KEY, "RetainedTempDir"),
                (HAS_EXPLICIT_KEY, "bool"),
            ],
            |ctx, params| {
                let name = params
                    .get_string(0)
                    .ok_or("Expected explicit hearth name")?;
                // The explicit hearth lives OUTSIDE the permitted root in its own
                // temp dir, so it must be folded in addition to the discovered set.
                let (handle, explicit_parent) = retained_temp_dir("hd_explicit")?;
                let explicit = explicit_parent.join(name.trim());
                make_hearth(&explicit)?;
                let mut out = Context::new();
                carry_root(&ctx, &mut out);
                out.set(EXPLICIT_KEY, explicit);
                out.set::<RetainedTempDir>(EXPLICIT_HANDLE_KEY, handle);
                out.set(HAS_EXPLICIT_KEY, true);
                Ok(out)
            },
        ),
        step_def(
            "the explicit default hearth is the sub-hearth {string} under that root",
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (EXPLICIT_KEY, "PathBuf"),
                (HAS_EXPLICIT_KEY, "bool"),
            ],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected sub-hearth name")?;
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No root")?.clone();
                let explicit = root.join(name.trim());
                let mut out = Context::new();
                carry_root(&ctx, &mut out);
                out.set(EXPLICIT_KEY, explicit);
                out.set(HAS_EXPLICIT_KEY, true);
                Ok(out)
            },
        ),
        step_def(
            "no explicit default hearth",
            &[(ROOT_KEY, "PathBuf"), (ROOT_HANDLE_KEY, "RetainedTempDir")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (HAS_EXPLICIT_KEY, "bool"),
            ],
            |ctx, _params| {
                let mut out = Context::new();
                carry_root(&ctx, &mut out);
                out.set(HAS_EXPLICIT_KEY, false);
                Ok(out)
            },
        ),
        step_def(
            "hearths are discovered for that root and explicit hearth",
            &[(ROOT_KEY, "PathBuf"), (HAS_EXPLICIT_KEY, "bool")],
            &[
                (ROOT_KEY, "PathBuf"),
                (ROOT_HANDLE_KEY, "RetainedTempDir"),
                (EXPLICIT_HANDLE_KEY, "RetainedTempDir"),
                (DISCOVERED_KEY, "Vec<PathBuf>"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No root")?.clone();
                let has_explicit = ctx.get::<bool>(HAS_EXPLICIT_KEY).copied().unwrap_or(false);
                let explicit = if has_explicit {
                    ctx.get::<PathBuf>(EXPLICIT_KEY).cloned()
                } else {
                    None
                };
                let discovered = discover_hearths(&[root], explicit.as_deref());
                let mut out = Context::new();
                carry_root(&ctx, &mut out);
                carry_retained_temp_dir(&ctx, &mut out, EXPLICIT_HANDLE_KEY);
                out.set(DISCOVERED_KEY, discovered);
                Ok(out)
            },
        ),
        check_def(
            "the discovered hearths include {string}",
            &[(DISCOVERED_KEY, "Vec<PathBuf>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected hearth name")?;
                let names = discovered_names(&ctx)?;
                if names.iter().any(|n| n == name.trim()) {
                    Ok(())
                } else {
                    Err(format!("expected '{}' among discovered {:?}", name, names))
                }
            },
        ),
        check_def(
            "the discovered hearths exclude {string}",
            &[(DISCOVERED_KEY, "Vec<PathBuf>")],
            |ctx, params| {
                let name = params.get_string(0).ok_or("Expected dir name")?;
                let names = discovered_names(&ctx)?;
                if names.iter().any(|n| n == name.trim()) {
                    Err(format!(
                        "expected '{}' to be EXCLUDED from {:?}",
                        name, names
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def(
            "the root itself is among the discovered hearths",
            &[(ROOT_KEY, "PathBuf"), (DISCOVERED_KEY, "Vec<PathBuf>")],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(ROOT_KEY).ok_or("No root")?;
                let discovered = ctx
                    .get::<Vec<PathBuf>>(DISCOVERED_KEY)
                    .ok_or("No discovered")?;
                if discovered.iter().any(|p| p == root) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected root {} among discovered {:?}",
                        root.display(),
                        discovered
                    ))
                }
            },
        ),
        check_def(
            "{int} hearths are discovered",
            &[(DISCOVERED_KEY, "Vec<PathBuf>")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let discovered = ctx
                    .get::<Vec<PathBuf>>(DISCOVERED_KEY)
                    .ok_or("No discovered")?;
                if discovered.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} discovered, got {}: {:?}",
                        expected,
                        discovered.len(),
                        discovered
                    ))
                }
            },
        ),
    ]
}

const EXPLICIT_HANDLE_KEY: &str = "hd_explicit_handle";

fn carry_root(ctx: &Context, out: &mut Context) {
    if let Some(root) = ctx.get::<PathBuf>(ROOT_KEY) {
        out.set(ROOT_KEY, root.clone());
    }
    carry_retained_temp_dir(ctx, out, ROOT_HANDLE_KEY);
}
