//! Step definitions for kit build script (scripts/build-kit.sh) scenarios.
//!
//! Steps operate on the workspace root, resolving paths via `CARGO_MANIFEST_DIR`
//! walk-up (same approach as kit_manifest / kit_playbook_source).

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::process::Command;

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

fn dist_file_path(rel: &str) -> PathBuf {
    if let Ok(kit_dir) = std::env::var("KIT_DIR") {
        if let Some(suffix) = rel.strip_prefix("dist/anvil-kit/") {
            return PathBuf::from(kit_dir).join(suffix);
        }
    }
    workspace_root().join(rel)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // Then: assert a script exists in the workspace and is executable.
        check_def(
            "the build script {string} exists and is executable",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let root = workspace_root();
                let path = root.join(rel);
                if !path.exists() {
                    return Err(format!("Script not found: {}", path.display()));
                }
                // Check executable bit via metadata permissions
                use std::os::unix::fs::PermissionsExt;
                let meta = std::fs::metadata(&path)
                    .map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?;
                let mode = meta.permissions().mode();
                if mode & 0o111 == 0 {
                    return Err(format!(
                        "Script exists but is not executable (mode {:o}): {}",
                        mode,
                        path.display()
                    ));
                }
                Ok(())
            },
        ),
        // Then: run the script and assert it exits 0.
        check_def(
            "executing the script {string} exits 0",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let root = workspace_root();
                let script = root.join(rel);
                let output = Command::new(&script)
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("Failed to execute {}: {}", script.display(), e))?;
                if output.status.success() {
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Err(format!(
                        "Script {} exited with {:?}\nstdout: {}\nstderr: {}",
                        script.display(),
                        output.status.code(),
                        stdout,
                        stderr
                    ))
                }
            },
        ),
        // Given: execute the build script (caching the result for subsequent steps).
        step_def(
            "the build script has been executed",
            &[],
            &[("build_script_executed", "bool")],
            |_ctx, _params| {
                let root = workspace_root();
                let script = root.join("scripts/build-kit.sh");
                let output = Command::new(&script)
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("Failed to execute build script: {}", e))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return Err(format!(
                        "Build script failed with {:?}\nstdout: {}\nstderr: {}",
                        output.status.code(),
                        stdout,
                        stderr
                    ));
                }
                let mut out = Context::new();
                out.set("build_script_executed", true);
                Ok(out)
            },
        ),
        // Then: assert a file exists under dist/ (workspace-relative).
        check_def("the dist file {string} exists", &[], |_ctx, params| {
            let rel = params.get_string(0).ok_or("Expected path")?;
            let path = dist_file_path(&rel);
            if path.exists() {
                Ok(())
            } else {
                Err(format!("Dist file not found: {}", path.display()))
            }
        }),
        // Then: assert a dist file exists and is executable.
        check_def(
            "the dist file {string} exists and is executable",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let path = dist_file_path(&rel);
                if !path.exists() {
                    return Err(format!("Dist file not found: {}", path.display()));
                }
                use std::os::unix::fs::PermissionsExt;
                let meta = std::fs::metadata(&path)
                    .map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?;
                let mode = meta.permissions().mode();
                if mode & 0o111 == 0 {
                    return Err(format!(
                        "Dist file exists but is not executable (mode {:o}): {}",
                        mode,
                        path.display()
                    ));
                }
                Ok(())
            },
        ),
        // Then: assert a dist file contains expected text.
        check_def(
            "the dist file {string} contains {string}",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let expected = params.get_string(1).ok_or("Expected text")?;
                let path = dist_file_path(&rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                if content.contains(expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Dist file {} did not contain expected text {:?}",
                        path.display(),
                        expected
                    ))
                }
            },
        ),
        // Then: assert a dist file exists and is jq-parseable JSON.
        check_def(
            "the dist file {string} exists and is jq-parseable",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?;
                let path = dist_file_path(&rel);
                if !path.exists() {
                    return Err(format!("Dist file not found: {}", path.display()));
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                serde_json::from_str::<serde_json::Value>(&content)
                    .map_err(|e| format!("Not valid JSON at {}: {}", path.display(), e))?;
                Ok(())
            },
        ),
        check_def(
            "the dist manifest {string} declares playbook definition {string} at path {string} with anvil kind {string}",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected manifest path")?;
                let expected_id = params.get_string(1).ok_or("Expected playbook id")?;
                let expected_path = params.get_string(2).ok_or("Expected playbook path")?;
                let expected_kind = params.get_string(3).ok_or("Expected anvil kind")?;
                let path = dist_file_path(&rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let manifest: serde_json::Value = serde_json::from_str(&content)
                    .map_err(|e| format!("Not valid JSON at {}: {}", path.display(), e))?;
                let definitions = manifest
                    .pointer("/playbooks/definitions")
                    .and_then(|value| value.as_array())
                    .ok_or_else(|| {
                        format!(
                            "Manifest {} has no playbooks.definitions array",
                            path.display()
                        )
                    })?;
                let found = definitions.iter().any(|definition| {
                    definition.get("id").and_then(|value| value.as_str()) == Some(&expected_id)
                        && definition.get("path").and_then(|value| value.as_str())
                            == Some(&expected_path)
                        && definition.get("anvil_kind").and_then(|value| value.as_str())
                            == Some(&expected_kind)
                });
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "Manifest {} did not declare playbook id {:?} path {:?} anvil_kind {:?}; definitions: {}",
                        path.display(),
                        expected_id,
                        expected_path,
                        expected_kind,
                        serde_json::Value::Array(definitions.clone())
                    ))
                }
            },
        ),
        check_def(
            "the dist manifest {string} does not declare playbook definition {string}",
            &[],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected manifest path")?;
                let excluded_id = params.get_string(1).ok_or("Expected playbook id")?;
                let path = dist_file_path(&rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let manifest: serde_json::Value = serde_json::from_str(&content)
                    .map_err(|e| format!("Not valid JSON at {}: {}", path.display(), e))?;
                let definitions = manifest
                    .pointer("/playbooks/definitions")
                    .and_then(|value| value.as_array())
                    .ok_or_else(|| {
                        format!(
                            "Manifest {} has no playbooks.definitions array",
                            path.display()
                        )
                    })?;
                let found = definitions.iter().any(|definition| {
                    definition.get("id").and_then(|value| value.as_str()) == Some(&excluded_id)
                });
                if found {
                    Err(format!(
                        "Manifest {} unexpectedly declared playbook id {:?}; definitions: {}",
                        path.display(),
                        excluded_id,
                        serde_json::to_string(definitions)
                            .unwrap_or_else(|_| "<unprintable>".to_string())
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        check_def("the dist path {string} does not exist", &[], |_ctx, params| {
            let rel = params.get_string(0).ok_or("Expected path")?;
            let path = dist_file_path(&rel);
            if path.exists() {
                Err(format!("Dist path unexpectedly exists: {}", path.display()))
            } else {
                Ok(())
            }
        }),
        // Then: run the build script twice and diff non-binary output (reproducibility).
        // This step is expensive (invokes cargo build --release twice).
        check_def(
            "two sequential runs of the build script produce identical non-binary output",
            &[],
            |_ctx, _params| {
                let root = workspace_root();
                let script = root.join("scripts/build-kit.sh");

                // First run
                let out1 = Command::new(&script)
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("Failed to execute build script (run 1): {}", e))?;
                if !out1.status.success() {
                    return Err(format!(
                        "Build script failed on run 1: {}",
                        String::from_utf8_lossy(&out1.stderr)
                    ));
                }

                // Rename dist → dist.run1
                let dist = root.join("dist");
                let dist_run1 = root.join("dist.run1");
                if dist_run1.exists() {
                    std::fs::remove_dir_all(&dist_run1)
                        .map_err(|e| format!("Failed to remove old dist.run1: {}", e))?;
                }
                std::fs::rename(&dist, &dist_run1)
                    .map_err(|e| format!("Failed to rename dist → dist.run1: {}", e))?;

                // Second run
                let out2 = Command::new(&script)
                    .current_dir(&root)
                    .output()
                    .map_err(|e| format!("Failed to execute build script (run 2): {}", e))?;
                if !out2.status.success() {
                    let _ = std::fs::remove_dir_all(&dist_run1);
                    return Err(format!(
                        "Build script failed on run 2: {}",
                        String::from_utf8_lossy(&out2.stderr)
                    ));
                }

                // Diff the two runs excluding the binary
                let diff_out = Command::new("diff")
                    .args([
                        "-r",
                        dist_run1.join("anvil-kit").to_str().unwrap(),
                        dist.join("anvil-kit").to_str().unwrap(),
                        "--exclude=anvil-mcp",
                    ])
                    .output()
                    .map_err(|e| format!("Failed to run diff: {}", e))?;

                // Cleanup dist.run1 regardless of result
                let _ = std::fs::remove_dir_all(&dist_run1);

                if diff_out.status.success() && diff_out.stdout.is_empty() {
                    Ok(())
                } else {
                    let diff_text = String::from_utf8_lossy(&diff_out.stdout);
                    Err(format!(
                        "Reproducibility diff was non-empty (builds are not identical):\n{}",
                        diff_text
                    ))
                }
            },
        ),
    ]
}
