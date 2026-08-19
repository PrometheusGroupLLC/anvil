//! Step definitions for `anvil-core/features/kit_playbook_bijection.feature`.
//!
//! C1's packaging gate (`spec.md:1017-1027`). The seam drives the SAME three
//! scripts `scripts/build-kit.sh` invokes, in the same order:
//!
//!   1. `scripts/stage-kit-content.sh`             — owns "packaged iff machine.yaml"
//!   2. `scripts/declare-kit-playbooks.sh`         — owns the manifest definitions block
//!   3. `scripts/assert-kit-playbook-bijection.py` — owns the four-set bijection
//!
//! Running the whole of `build-kit.sh` here is not viable (three cross-compiles
//! plus `npm ci` against `kit/app/frontend`, mutating shared build state on every
//! suite run) — the same reasoning `stage-kit-content.sh`'s header already
//! records for `agent_hook_paths_resolve.feature`. The rules this seam depends on
//! live in the delegated scripts precisely so the seam cannot drift from the
//! published kit.
//!
//! NOTHING HERE ASSERTS A COUNT. `spec.md:1026-1027` forbids a numeric constant
//! or the checked-in template population as an oracle, so every set is derived
//! from the build under test. The only floor is non-emptiness: four empty sets
//! are equal, and a vacuously green gate is the failure mode this file exists to
//! prevent.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{step_def, StepDef};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

const KIT_ROOT_KEY: &str = "kpb_staged_kit_root";
const KIT_HANDLE_KEY: &str = "kpb_staged_kit_handle";

fn workspace_root() -> PathBuf {
    let start = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR);
    let mut current: &Path = &start;
    loop {
        let manifest = current.join("Cargo.toml");
        if manifest.exists() {
            if let Ok(contents) = std::fs::read_to_string(&manifest) {
                if contents.contains("[workspace]") {
                    return current.to_path_buf();
                }
            }
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return start,
        }
    }
}

/// Read the staged manifest's `playbooks.definitions` as `(id, path, anvil_kind)`.
fn staged_definitions(kit_root: &Path) -> Result<Vec<(String, String, String)>, String> {
    let manifest_path = kit_root.join("foundry-manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let doc: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("parse staged manifest: {e}"))?;
    let defs = doc
        .get("playbooks")
        .and_then(|p| p.get("definitions"))
        .and_then(|d| d.as_array())
        .ok_or_else(|| "staged manifest has no playbooks.definitions array".to_string())?;
    Ok(defs
        .iter()
        .map(|d| {
            (
                d.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                d.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                d.get("anvil_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect())
}

fn definition_basename(path: &str) -> String {
    path.trim_end_matches('/')
        .trim_start_matches("playbooks/")
        .to_string()
}

fn source_machine_dirs(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("playbooks");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        if entry.path().join("machine.yaml").is_file() {
            out.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn source_dirs_without_machine(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("playbooks");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let p = entry.path();
        if p.is_dir() && !p.join("machine.yaml").is_file() {
            out.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    out.sort();
    Ok(out)
}

/// Carry the staged kit root and its TempDir handle forward.
///
/// Brine retains only the keys a step declares as `provides`, so every Then step
/// that reads the staged kit must re-declare and re-emit both, or the next step
/// loses the tree out from under itself.
fn carry(ctx: &mut Context) -> Result<(Context, PathBuf), String> {
    let root_str = ctx.require::<String>(KIT_ROOT_KEY)?.clone();
    let handle = ctx.take::<Arc<TempDir>>(KIT_HANDLE_KEY);
    let mut out = Context::new();
    out.set(KIT_ROOT_KEY, root_str.clone());
    if let Some(h) = handle {
        out.set(KIT_HANDLE_KEY, h);
    }
    Ok((out, PathBuf::from(root_str)))
}

const CARRIED: &[(&str, &str)] = &[(KIT_ROOT_KEY, "string"), (KIT_HANDLE_KEY, "handle")];

pub fn steps() -> Vec<StepDef> {
    vec![
        // Given: stage + declare through the production scripts into a TempDir.
        step_def(
            "an anvil-kit staged through the production packaging scripts",
            &[],
            CARRIED,
            |_ctx, _params| {
                let root = workspace_root();
                let stage = root.join("scripts/stage-kit-content.sh");
                let declare = root.join("scripts/declare-kit-playbooks.sh");
                for s in [&stage, &declare] {
                    if !s.is_file() {
                        return Err(format!("packaging script not found: {}", s.display()));
                    }
                }

                let temp = TempDir::new().map_err(|e| format!("temp dir: {e}"))?;
                let kit_root = temp.path().to_path_buf();

                // The manifest TEMPLATE is the declaration step's INPUT. It is
                // deliberately not trusted as the answer: the checked-in block
                // and the derived block differ in this repo today, and the
                // bijection is what catches a build that shipped the template.
                std::fs::copy(
                    root.join("kit/foundry-manifest.json"),
                    kit_root.join("foundry-manifest.json"),
                )
                .map_err(|e| format!("seed manifest template: {e}"))?;

                let staged = Command::new(&stage)
                    .arg(&kit_root)
                    .current_dir(&root)
                    .env("ANVIL_REPO", &root)
                    .output()
                    .map_err(|e| format!("exec {}: {e}", stage.display()))?;
                if !staged.status.success() {
                    return Err(format!(
                        "staging failed ({:?}): {}",
                        staged.status.code(),
                        String::from_utf8_lossy(&staged.stderr)
                    ));
                }
                let ids: Vec<String> = String::from_utf8_lossy(&staged.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if ids.is_empty() {
                    return Err("staging produced no playbook ids".to_string());
                }

                let declared = Command::new(&declare)
                    .arg(&kit_root)
                    .args(&ids)
                    .current_dir(&root)
                    .env("ANVIL_REPO", &root)
                    .output()
                    .map_err(|e| format!("exec {}: {e}", declare.display()))?;
                if !declared.status.success() {
                    return Err(format!(
                        "manifest declaration failed ({:?}): {}",
                        declared.status.code(),
                        String::from_utf8_lossy(&declared.stderr)
                    ));
                }

                let mut out = Context::new();
                out.set(KIT_ROOT_KEY, kit_root.display().to_string());
                out.set(KIT_HANDLE_KEY, Arc::new(temp));
                Ok(out)
            },
        ),
        // Then: A = B, each declared exactly once, id agreeing with path.
        step_def(
            "every machine-backed source is declared exactly once in the staged manifest",
            CARRIED,
            CARRIED,
            |mut ctx, _params| {
                let (out, kit_root) = carry(&mut ctx)?;
                let root = workspace_root();
                let sources = source_machine_dirs(&root)?;
                if sources.is_empty() {
                    return Err(
                        "no source playbook carries a machine.yaml — set equality over empty sets \
                         is vacuously true, so this is a STOP, not a pass"
                            .to_string(),
                    );
                }
                let defs = staged_definitions(&kit_root)?;
                let mut declared: Vec<String> =
                    defs.iter().map(|(_, p, _)| definition_basename(p)).collect();
                let before = declared.len();
                declared.sort();
                let mut deduped = declared.clone();
                deduped.dedup();
                if deduped.len() != before {
                    return Err(format!("a source is declared more than once: {declared:?}"));
                }
                if declared != sources {
                    return Err(format!(
                        "declared set != machine-backed source set\n  sources : {sources:?}\n  \
                         declared: {declared:?}"
                    ));
                }
                for (id, path, kind) in &defs {
                    if *id != definition_basename(path) {
                        return Err(format!("declared id {id:?} disagrees with path {path:?}"));
                    }
                    if kind.is_empty() {
                        return Err(format!("declared definition {id:?} has an empty anvil_kind"));
                    }
                }
                Ok(out)
            },
        ),
        // Then: the production gate itself — B = D, canonical AND legal.
        step_def(
            "every declared staged definition has a canonical legal playbook status",
            CARRIED,
            CARRIED,
            |mut ctx, _params| {
                let (out, kit_root) = carry(&mut ctx)?;
                let root = workspace_root();
                let script = root.join("scripts/assert-kit-playbook-bijection.py");
                let res = Command::new("python3")
                    .arg(&script)
                    .arg(&kit_root)
                    .current_dir(&root)
                    .env("ANVIL_REPO", &root)
                    .output()
                    .map_err(|e| format!("exec {}: {e}", script.display()))?;
                if !res.status.success() {
                    return Err(format!(
                        "the packaging bijection does not hold ({:?}):\n{}{}",
                        res.status.code(),
                        String::from_utf8_lossy(&res.stdout),
                        String::from_utf8_lossy(&res.stderr)
                    ));
                }
                Ok(out)
            },
        ),
        // Then: B = C — every declared path is actually packaged.
        step_def(
            "no declared path is missing from the staged package",
            CARRIED,
            CARRIED,
            |mut ctx, _params| {
                let (out, kit_root) = carry(&mut ctx)?;
                let defs = staged_definitions(&kit_root)?;
                let mut missing = Vec::new();
                for (id, path, _) in &defs {
                    let dir = kit_root.join(path.trim_end_matches('/'));
                    if !dir.join("machine.yaml").is_file() {
                        missing.push(format!("{id} ({} has no machine.yaml)", dir.display()));
                    }
                }
                if !missing.is_empty() {
                    return Err(format!("declared but not packaged: {missing:?}"));
                }
                Ok(out)
            },
        ),
        // Then: what the packaging predicate EXCLUDES, asserted not assumed.
        step_def(
            "a source playbook directory with no machine is absent from the staged manifest",
            CARRIED,
            CARRIED,
            |mut ctx, _params| {
                let (out, kit_root) = carry(&mut ctx)?;
                let root = workspace_root();
                let excluded = source_dirs_without_machine(&root)?;
                if excluded.is_empty() {
                    return Err(
                        "no source playbook directory lacks a machine.yaml, so the exclusion this \
                         scenario measures cannot be observed. Do not weaken it — construct one, or \
                         the packaging predicate's boundary is untested."
                            .to_string(),
                    );
                }
                let declared: Vec<String> = staged_definitions(&kit_root)?
                    .iter()
                    .map(|(id, _, _)| id.clone())
                    .collect();
                for name in &excluded {
                    if declared.contains(name) {
                        return Err(format!(
                            "{name:?} has no machine.yaml yet is declared in the staged manifest"
                        ));
                    }
                    if kit_root.join("playbooks").join(name).exists() {
                        return Err(format!("{name:?} has no machine.yaml yet was staged"));
                    }
                }
                Ok(out)
            },
        ),
        step_def(
            "the excluded directory is still present in the source tree",
            CARRIED,
            CARRIED,
            |mut ctx, _params| {
                let (out, _kit_root) = carry(&mut ctx)?;
                let root = workspace_root();
                let excluded = source_dirs_without_machine(&root)?;
                if excluded.is_empty() {
                    return Err("no excluded source directory to observe".to_string());
                }
                for name in &excluded {
                    if !root.join("playbooks").join(name).is_dir() {
                        return Err(format!("{name:?} is not present in the source tree"));
                    }
                }
                Ok(out)
            },
        ),
        // Then: the gate's one non-derived datum is pinned to the compiled seed.
        step_def(
            "the packaging gate's legal playbook states equal the playbook seed's states",
            &[],
            &[],
            |_ctx, _params| {
                let root = workspace_root();
                let script = root.join("scripts/assert-kit-playbook-bijection.py");
                let res = Command::new("python3")
                    .arg(&script)
                    .arg("--print-legal-states")
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("exec {}: {e}", script.display()))?;
                if !res.status.success() {
                    return Err(format!(
                        "could not read the gate's legal state list: {}",
                        String::from_utf8_lossy(&res.stderr)
                    ));
                }
                let mut from_gate: Vec<String> = String::from_utf8_lossy(&res.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                let mut from_seed: Vec<String> =
                    anvil_core::domain::playbook::seeds::playbook_seed()
                        .states
                        .iter()
                        .map(|s| s.name.clone())
                        .collect();
                if from_seed.is_empty() {
                    return Err("the playbook seed declares no states".to_string());
                }
                from_gate.sort();
                from_seed.sort();
                if from_gate != from_seed {
                    return Err(format!(
                        "the packaging gate's legal states have drifted from the playbook seed\n  \
                         gate: {from_gate:?}\n  seed: {from_seed:?}"
                    ));
                }
                Ok(Context::new())
            },
        ),
    ]
}
