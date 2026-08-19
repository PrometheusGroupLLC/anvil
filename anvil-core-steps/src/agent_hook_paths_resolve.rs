//! Step definitions for `anvil-core/features/agent_hook_paths_resolve.feature`.
//!
//! The point of this seam: agent-facing instructions (shipped skills, playbook
//! hook bodies) name hook files by path. Those paths must resolve against what
//! the kit actually SHIPS, not against the source tree — a `playbooks/<id>/`
//! directory without a `machine.yaml` is never packaged, installed, or
//! registered, so a hook path inside one is dead on a user's machine even
//! though it exists in this repo.
//!
//! So the `Given` stages real kit content into an isolated temp directory by
//! driving `scripts/stage-kit-content.sh` — the same script `scripts/build-kit.sh`
//! invokes to assemble `dist/anvil-kit/`. Every documented hook path is then
//! resolved beneath that staged root.
//!
//! The scan has an explicit population floor: zero documented hook paths is an
//! error, not a pass, so the check can never be vacuously green.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

/// Context key: absolute path of the isolated staged kit root.
const STAGED_ROOT_KEY: &str = "ahpr_staged_kit_root";
/// Context key: the TempDir handle, kept alive so the staged tree survives the scenario.
const STAGED_HANDLE_KEY: &str = "ahpr_staged_kit_handle";
/// Context key: every documented hook path reference found in staged content.
const REFERENCES_KEY: &str = "ahpr_documented_hook_paths";
/// Context key: hook path references found in the repository slash-command tree.
const COMMAND_REFERENCES_KEY: &str = "ahpr_command_hook_paths";

/// One documented hook path, with the staged file that documents it.
#[derive(Clone, Debug)]
pub struct HookPathReference {
    /// Path of the documenting file, relative to the staged kit root.
    pub source: String,
    /// The path reference exactly as written in that file.
    pub reference: String,
}

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

/// Characters that may appear inside an unquoted path reference in markdown.
///
/// `*` is admitted deliberately. The GLOB form (`workflows/*/hooks/`,
/// `playbooks/*/hooks/`) was 21 of the 40 legacy occurrences this seam's
/// migration repaired; excluding `*` made every one of them invisible to the
/// scan and left the check's glob branch unreachable.
///
/// Template placeholders (`<playbook-dir>/hooks/<filename>`) still drop out of
/// EXTRACTION, because `<` and `>` are not path characters — but they do NOT
/// escape the seam. An independent reviewer proved it: replacing a real path
/// with a placeholder reds the population floor below
/// (`Only 39 ... the floor is 40`) and reds the residual-token classifier.
///
/// DO NOT "FIX" the placeholder case by loosening `is_pathish` further. The
/// `>= MIN_REFERENCES` floor — not the character class — is what makes this
/// seam robust against every form of loss, and widening extraction to swallow
/// placeholders would inflate the population and weaken exactly that.
fn is_pathish(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '*')
}

/// Pull every `<root>/.../hooks[/<file>]` reference out of a markdown body,
/// where `<root>` is the canonical `playbooks` or the legacy `workflows`.
///
/// Template placeholders (`<playbook-dir>/hooks/<filename>`) drop out naturally:
/// `<` and `>` are not path characters, so the run breaks before `/hooks`.
pub fn extract_hook_references(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !is_pathish(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_pathish(chars[i]) {
            i += 1;
        }
        let run: String = chars[start..i].iter().collect();
        // The char class swallows sentence-final periods; a real reference never
        // ends in one.
        let run = run.trim_end_matches('.');
        if !run.contains("/hooks") {
            continue;
        }
        for root in ["workflows/", "playbooks/"] {
            if let Some(pos) = run.find(root) {
                let candidate = &run[pos..];
                if candidate.contains("/hooks") {
                    found.push(candidate.to_string());
                }
                break;
            }
        }
    }
    found
}

fn markdown_files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Source playbook directory ids that carry a `machine.yaml` (the ones the kit packages).
fn source_playbook_ids_with_machine(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(root.join("playbooks")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("machine.yaml").is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

fn staged_playbook_ids(staged_root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(staged_root.join("playbooks")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // Given: collect every documented hook path from the repository's Claude
        // Code slash-command prompts. These are not kit content — they are the
        // instructions an agent working in this repo (or in brine, which
        // symlinks the same tree) is served — so they resolve against the
        // SOURCE playbooks tree.
        step_def(
            "the repository's Claude Code slash-command prompts",
            &[],
            &[(COMMAND_REFERENCES_KEY, "list")],
            |_ctx, _params| {
                let root = workspace_root();
                let commands = root.join(".claude/commands/forge");
                if !commands.is_dir() {
                    return Err(format!(
                        "slash-command prompt tree not found at {}",
                        commands.display()
                    ));
                }
                let mut references: Vec<HookPathReference> = Vec::new();
                for file in markdown_files_under(&commands) {
                    let body = std::fs::read_to_string(&file)
                        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
                    let relative = file
                        .strip_prefix(&root)
                        .unwrap_or(&file)
                        .display()
                        .to_string();
                    for reference in extract_hook_references(&body) {
                        references.push(HookPathReference {
                            source: relative.clone(),
                            reference,
                        });
                    }
                }
                // Population floor. Not "non-zero": the migration that created
                // this seam repaired 40 legacy occurrences across this tree, of
                // which 21 were the glob form. A scan returning fewer than that
                // has silently lost sites — which is exactly how the glob form
                // went unguarded in the first place.
                const MIN_REFERENCES: usize = 40;
                if references.len() < MIN_REFERENCES {
                    return Err(format!(
                        "Only {} documented hook path(s) found under {}; the floor is {}. \
                         A scan that has lost sites cannot pass this seam.",
                        references.len(),
                        commands.display(),
                        MIN_REFERENCES
                    ));
                }
                let mut out = Context::new();
                out.set(COMMAND_REFERENCES_KEY, references);
                Ok(out)
            },
        ),
        // Then: every slash-command hook path resolves below source playbooks/.
        check_def(
            "each documented slash-command hook path exists below the source playbooks directory",
            &[(COMMAND_REFERENCES_KEY, "list")],
            |ctx, _params| {
                let root = workspace_root();
                let references = ctx.require::<Vec<HookPathReference>>(COMMAND_REFERENCES_KEY)?;
                if references.is_empty() {
                    return Err("No slash-command hook paths were collected; refusing to pass."
                        .to_string());
                }
                let playbooks_root = root.join("playbooks");
                let mut failures: Vec<String> = Vec::new();
                for entry in references {
                    let relative = entry.reference.trim_end_matches('/');
                    // A glob reference (`playbooks/*/hooks/`) names the tree, not
                    // one file: require the canonical root and a real playbooks
                    // directory rather than a literal `*` path.
                    if relative.contains('*') {
                        if !relative.starts_with("playbooks/") {
                            failures.push(format!(
                                "hook path glob `{}` documented in {} does not name the canonical \
                                 playbooks tree",
                                entry.reference, entry.source
                            ));
                        }
                        continue;
                    }
                    let resolved = root.join(relative);
                    if !resolved.starts_with(&playbooks_root) {
                        failures.push(format!(
                            "hook path `{}` documented in {} is not below the source playbooks \
                             directory (resolved: {})",
                            entry.reference,
                            entry.source,
                            resolved.display()
                        ));
                        continue;
                    }
                    if !resolved.exists() {
                        failures.push(format!(
                            "hook path `{}` documented in {} does not exist (resolved: {})",
                            entry.reference,
                            entry.source,
                            resolved.display()
                        ));
                    }
                }
                if failures.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} of {} slash-command hook path(s) do not resolve below the source \
                         playbooks directory:\n  - {}",
                        failures.len(),
                        references.len(),
                        failures.join("\n  - ")
                    ))
                }
            },
        ),
        // Given: stage real kit content (playbooks + skills) into an isolated dir
        // by driving the same script scripts/build-kit.sh uses.
        step_def(
            "the shipped skills and source playbooks",
            &[],
            // BOTH keys must be declared: brine retains only the keys a step
            // declares as `provides`. Dropping the TempDir handle here would drop
            // the TempDir itself and delete the staged tree before the next step.
            &[(STAGED_ROOT_KEY, "string"), (STAGED_HANDLE_KEY, "handle")],
            |_ctx, _params| {
                let root = workspace_root();
                let script = root.join("scripts/stage-kit-content.sh");
                if !script.is_file() {
                    return Err(format!(
                        "kit content staging script not found: {}",
                        script.display()
                    ));
                }
                let temp = TempDir::new()
                    .map_err(|e| format!("Failed to create staging temp dir: {e}"))?;
                let staged_root = temp.path().to_path_buf();

                let output = Command::new(&script)
                    .arg(&staged_root)
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("Failed to execute {}: {e}", script.display()))?;
                if !output.status.success() {
                    return Err(format!(
                        "Kit content staging failed with {:?}\nstdout: {}\nstderr: {}",
                        output.status.code(),
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                let staged_skills = staged_root.join("skills");
                let staged_playbooks = staged_root.join("playbooks");
                if !staged_skills.is_dir() {
                    return Err(format!(
                        "Staged kit has no skills/ tree at {}",
                        staged_skills.display()
                    ));
                }
                if !staged_playbooks.is_dir() {
                    return Err(format!(
                        "Staged kit has no playbooks/ tree at {}",
                        staged_playbooks.display()
                    ));
                }

                let mut out = Context::new();
                out.set(STAGED_ROOT_KEY, staged_root.display().to_string());
                out.set(STAGED_HANDLE_KEY, Arc::new(temp));
                Ok(out)
            },
        ),
        // When: collect every documented hook path from the staged content.
        step_def(
            "every documented hook path is resolved",
            &[(STAGED_ROOT_KEY, "string")],
            // Brine retains only the keys a step declares as `provides`, so the
            // staged root and its TempDir handle must be re-declared and
            // re-emitted here or the Then step loses both.
            &[
                (STAGED_ROOT_KEY, "string"),
                (STAGED_HANDLE_KEY, "handle"),
                (REFERENCES_KEY, "list"),
            ],
            |mut ctx, _params| {
                let staged_root_str = ctx.require::<String>(STAGED_ROOT_KEY)?.clone();
                let handle = ctx.take::<Arc<TempDir>>(STAGED_HANDLE_KEY);
                let staged_root = PathBuf::from(&staged_root_str);
                let mut references: Vec<HookPathReference> = Vec::new();
                for file in markdown_files_under(&staged_root) {
                    let body = std::fs::read_to_string(&file)
                        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;
                    let relative = file
                        .strip_prefix(&staged_root)
                        .unwrap_or(&file)
                        .display()
                        .to_string();
                    for reference in extract_hook_references(&body) {
                        references.push(HookPathReference {
                            source: relative.clone(),
                            reference,
                        });
                    }
                }
                // Population floor: a scan that finds nothing cannot be a pass.
                if references.is_empty() {
                    return Err(format!(
                        "No documented hook path was found anywhere in the staged kit at {}. \
                         A zero-population scan cannot pass this seam.",
                        staged_root.display()
                    ));
                }
                let mut out = Context::new();
                out.set(STAGED_ROOT_KEY, staged_root_str);
                if let Some(handle) = handle {
                    out.set(STAGED_HANDLE_KEY, handle);
                }
                out.set(REFERENCES_KEY, references);
                Ok(out)
            },
        ),
        // Then: each documented path resolves to something that exists below the
        // staged kit's playbooks/ directory.
        check_def(
            "each current path exists below the playbooks directory",
            &[(STAGED_ROOT_KEY, "string"), (REFERENCES_KEY, "list")],
            |ctx, _params| {
                let staged_root = PathBuf::from(ctx.require::<String>(STAGED_ROOT_KEY)?);
                let references = ctx.require::<Vec<HookPathReference>>(REFERENCES_KEY)?;
                if references.is_empty() {
                    return Err("No documented hook paths were collected; refusing to pass."
                        .to_string());
                }
                let playbooks_root = staged_root.join("playbooks");
                let mut failures: Vec<String> = Vec::new();
                for entry in references {
                    let relative = entry.reference.trim_end_matches('/');
                    let resolved = staged_root.join(relative);
                    if !resolved.starts_with(&playbooks_root) {
                        failures.push(format!(
                            "hook path `{}` documented in {} is not below the playbooks directory \
                             (resolved: {})",
                            entry.reference,
                            entry.source,
                            resolved.display()
                        ));
                        continue;
                    }
                    if !resolved.exists() {
                        failures.push(format!(
                            "hook path `{}` documented in {} does not exist in the staged kit \
                             (resolved: {})",
                            entry.reference,
                            entry.source,
                            resolved.display()
                        ));
                    }
                }
                if failures.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "{} of {} documented hook path(s) do not resolve below the staged kit's \
                         playbooks directory:\n  - {}",
                        failures.len(),
                        references.len(),
                        failures.join("\n  - ")
                    ))
                }
            },
        ),
        // Then: the staged playbook set is exactly the machine-bearing source set.
        // This is what makes "staged, not source" falsifiable: if staging ever
        // widened to the whole source tree, this goes red.
        check_def(
            "the staged playbooks are exactly the source playbooks that carry a machine",
            &[(STAGED_ROOT_KEY, "string")],
            |ctx, _params| {
                let staged_root = PathBuf::from(ctx.require::<String>(STAGED_ROOT_KEY)?);
                let expected = source_playbook_ids_with_machine(&workspace_root());
                let actual = staged_playbook_ids(&staged_root);
                if expected.is_empty() {
                    return Err(
                        "No source playbook directory carries a machine.yaml; refusing to pass."
                            .to_string(),
                    );
                }
                if expected == actual {
                    Ok(())
                } else {
                    Err(format!(
                        "staged playbook set does not match the machine-bearing source set\n  \
                         expected: {:?}\n  staged:   {:?}",
                        expected, actual
                    ))
                }
            },
        ),
    ]
}
