//! Step definitions for anvil/playbooks/track_lifecycle/ validation scenarios.
//!
//! Steps operate on the workspace root, resolving paths via `CARGO_MANIFEST_DIR`
//! walk-up (same approach as `kit_manifest`). No runtime hearth required.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let start = PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR);
    let mut current: &std::path::Path = &start;
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

pub fn steps() -> Vec<StepDef> {
    vec![
        // Given: load a YAML file from workspace-relative path, validate it parses.
        step_def(
            "the playbook file {string} is loaded",
            &[],
            &[
                ("playbook_file_content", "String"),
                ("playbook_file_path", "PathBuf"),
            ],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?.to_string();
                let root = workspace_root();
                let path = root.join(&rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                // Validate it is parseable YAML
                serde_yaml::from_str::<serde_yaml::Value>(&content)
                    .map_err(|e| format!("Failed to parse YAML at {}: {}", path.display(), e))?;
                let mut out = Context::new();
                out.set("playbook_file_content", content);
                out.set("playbook_file_path", path);
                Ok(out)
            },
        ),
        // Then: assert a file exists at workspace-relative path.
        // Reuses same pattern as kit_manifest but under a distinct step text to avoid
        // ambiguity when both modules are loaded.
        check_def(
            "the playbook file {string} exists in the workspace",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let root = workspace_root();
                let path = root.join(rel);
                if path.exists() {
                    Ok(())
                } else {
                    Err(format!("File not found: {}", path.display()))
                }
            },
        ),
        // Then: assert the loaded playbook YAML has field `state` equal to `active`.
        check_def(
            "the playbook file has state {string}",
            &[("playbook_file_content", "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected state value")?;
                let content = ctx
                    .get::<String>("playbook_file_content")
                    .ok_or("No playbook_file_content in context")?;
                let val: serde_yaml::Value = serde_yaml::from_str(content)
                    .map_err(|e| format!("YAML parse error: {}", e))?;
                let state = val
                    .get("state")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'state' field in YAML")?;
                if state == expected {
                    Ok(())
                } else {
                    Err(format!("Expected state '{}', got '{}'", expected, state))
                }
            },
        ),
        // Then: assert a workspace-relative file contains a given word (substring).
        check_def(
            "the file {string} contains the word {string}",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let word = params.get_string(1).ok_or("Expected word")?;
                let root = workspace_root();
                let path = root.join(rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if content.contains(word) {
                    Ok(())
                } else {
                    Err(format!(
                        "File '{}' does not contain the word '{}'",
                        path.display(),
                        word
                    ))
                }
            },
        ),
    ]
}
