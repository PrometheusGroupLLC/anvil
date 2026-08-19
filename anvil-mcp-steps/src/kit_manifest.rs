//! Step definitions for kit/foundry-manifest.json validation scenarios.
//!
//! Steps operate on the workspace root, resolving paths via `CARGO_MANIFEST_DIR`
//! walk-up (same approach as `harness::binary_path`). No runtime hearth required.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct FoundryKitAppManifest {
    engine: Option<FoundryEngineConfig>,
    frontend: FoundryFrontendConfig,
    lifecycle: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FoundryEngineConfig {
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    health_check: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FoundryFrontendConfig {
    dev_command: Option<String>,
    dev_cwd: Option<String>,
    dev_url: Option<String>,
    health_check: Option<String>,
    build_dir: Option<String>,
    serve_port: Option<String>,
}

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
        // Given: load manifest JSON from workspace-relative path into context.
        step_def(
            "the kit manifest at {string} is loaded",
            &[],
            &[("kit_manifest", "JsonValue"), ("manifest_path", "PathBuf")],
            |_ctx, params| {
                let rel = params.get_string(0).ok_or("Expected path")?.to_string();
                let root = workspace_root();
                let path = root.join(&rel);
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let value: serde_json::Value = serde_json::from_str(&content)
                    .map_err(|e| format!("Failed to parse JSON at {}: {}", path.display(), e))?;
                let mut out = Context::new();
                out.set("kit_manifest", value);
                out.set("manifest_path", path);
                Ok(out)
            },
        ),
        // Then: assert a file exists at workspace-relative path.
        check_def(
            "the file {string} exists in the workspace",
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
        // Then: assert manifest JSON field (jq-style dot path) equals expected string.
        // Supports simple dot-paths like "kit.id" or array access like "kit.components[0]".
        check_def(
            "the manifest field {string} equals {string}",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let path = params.get_string(0).ok_or("Expected field path")?;
                let expected = params.get_string(1).ok_or("Expected value")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let actual = jq_get(manifest, path)?;
                let actual_str = json_to_string(&actual);
                if actual_str == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "manifest field '{}': expected '{}', got '{}'",
                        path, expected, actual_str
                    ))
                }
            },
        ),
        // Then: assert manifest array field contains a given string value.
        check_def(
            "the manifest field {string} contains the value {string}",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field path")?;
                let needle = params.get_string(1).ok_or("Expected value")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let arr = jq_get(manifest, field)?;
                let items = arr
                    .as_array()
                    .ok_or_else(|| format!("manifest field '{}' is not an array", field))?;
                let found = items.iter().any(|v| json_to_string(v) == needle);
                if found {
                    Ok(())
                } else {
                    Err(format!(
                        "manifest field '{}' does not contain '{}'",
                        field, needle
                    ))
                }
            },
        ),
        // Then: assert manifest array field does NOT contain a given string value.
        check_def(
            "the manifest field {string} does not contain the value {string}",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field path")?;
                let needle = params.get_string(1).ok_or("Expected value")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let arr = jq_get(manifest, field)?;
                let items = arr
                    .as_array()
                    .ok_or_else(|| format!("manifest field '{}' is not an array", field))?;
                let found = items.iter().any(|v| json_to_string(v) == needle);
                if found {
                    Err(format!(
                        "manifest field '{}' unexpectedly contains '{}'",
                        field, needle
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // Then: assert a manifest string field starts with a given prefix.
        check_def(
            "the manifest field {string} starts with {string}",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field path")?;
                let prefix = params.get_string(1).ok_or("Expected prefix")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let val = jq_get(manifest, field)?;
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("manifest field '{}' is not a string", field))?;
                if s.starts_with(prefix) {
                    Ok(())
                } else {
                    Err(format!(
                        "manifest field '{}' = '{}' does not start with '{}'",
                        field, s, prefix
                    ))
                }
            },
        ),
        // Then: assert a manifest string field does NOT start with a given prefix.
        check_def(
            "the manifest field {string} does not start with {string}",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let field = params.get_string(0).ok_or("Expected field path")?;
                let prefix = params.get_string(1).ok_or("Expected prefix")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let val = jq_get(manifest, field)?;
                let s = val
                    .as_str()
                    .ok_or_else(|| format!("manifest field '{}' is not a string", field))?;
                if s.starts_with(prefix) {
                    Err(format!(
                        "manifest field '{}' = '{}' unexpectedly starts with '{}'",
                        field, s, prefix
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // Then: assert all required fields exist (non-null) at dot-paths.
        check_def(
            "the manifest has all required kit fields",
            &[("kit_manifest", "JsonValue")],
            |ctx, _params| {
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let required = [
                    "kit.id",
                    "kit.name",
                    "kit.version",
                    "kit.description",
                    "kit.schema_version",
                ];
                let mut missing = Vec::new();
                for field in required {
                    match jq_get(manifest, field) {
                        Ok(v) if !v.is_null() => {}
                        _ => missing.push(field),
                    }
                }
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Missing required kit fields: {}",
                        missing.join(", ")
                    ))
                }
            },
        ),
        check_def(
            "the manifest app block parses as a Foundry kit app manifest",
            &[("kit_manifest", "JsonValue")],
            |ctx, _params| {
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let app = manifest
                    .get("app")
                    .ok_or("Manifest missing app block")?
                    .clone();
                let parsed: FoundryKitAppManifest = serde_json::from_value(app).map_err(|e| {
                    format!("app block did not parse as Foundry KitAppManifest: {}", e)
                })?;
                let engine = parsed
                    .engine
                    .ok_or("app block parsed but is missing engine config")?;
                if engine.command.trim().is_empty() {
                    return Err("app.engine.command is empty".to_string());
                }
                if engine.args.is_empty() {
                    return Err("app.engine.args is empty".to_string());
                }
                if engine
                    .health_check
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                {
                    return Err("app.engine.health_check is empty".to_string());
                }
                if parsed
                    .frontend
                    .build_dir
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                    && parsed
                        .frontend
                        .dev_url
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                {
                    return Err("app.frontend has neither build_dir nor dev_url".to_string());
                }
                let _ = (
                    engine.cwd.as_deref(),
                    parsed.lifecycle.as_deref(),
                    parsed.frontend.dev_command.as_deref(),
                    parsed.frontend.dev_cwd.as_deref(),
                    parsed.frontend.health_check.as_deref(),
                    parsed.frontend.serve_port.as_deref(),
                );
                Ok(())
            },
        ),
        check_def(
            "the manifest app frontend has a build directory",
            &[("kit_manifest", "JsonValue")],
            |ctx, _params| {
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let build_dir = jq_get(manifest, "app.frontend.build_dir")?;
                match build_dir.as_str() {
                    Some(s) if !s.trim().is_empty() => Ok(()),
                    Some(_) => Err("app.frontend.build_dir is empty".to_string()),
                    None => Err("app.frontend.build_dir is not a string".to_string()),
                }
            },
        ),
        check_def(
            "the manifest app engine uses literal port {string} without ENGINE_PORT placeholders",
            &[("kit_manifest", "JsonValue")],
            |ctx, params| {
                let expected_port = params.get_string(0).ok_or("Expected port")?;
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let engine = jq_get(manifest, "app.engine")?;
                let rendered = serde_json::to_string(engine)
                    .map_err(|e| format!("Failed to serialize app.engine: {}", e))?;
                if rendered.contains("${ENGINE_PORT}") {
                    return Err(format!(
                        "app.engine contains ENGINE_PORT placeholder: {}",
                        rendered
                    ));
                }
                if !rendered.contains(expected_port) {
                    return Err(format!(
                        "app.engine does not contain literal port {}: {}",
                        expected_port, rendered
                    ));
                }
                Ok(())
            },
        ),
        // Then: assert the hooks block declares a non-empty gate_query.
        check_def(
            "the manifest hooks gate_query is non-empty",
            &[("kit_manifest", "JsonValue")],
            |ctx, _params| {
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let gate_query = jq_get(manifest, "hooks.gate_query")?;
                match gate_query.as_str() {
                    Some(s) if !s.trim().is_empty() => Ok(()),
                    Some(_) => Err("hooks.gate_query is empty".to_string()),
                    None => Err("hooks.gate_query is not a string".to_string()),
                }
            },
        ),
        // Then: assert every hooks.hard_enforce entry resolves to a real declared
        // playbook kind (a `playbooks.definitions[*].anvil_kind`). An empty
        // hard_enforce list trivially passes.
        check_def(
            "every manifest hooks.hard_enforce entry resolves to a declared playbook kind",
            &[("kit_manifest", "JsonValue")],
            |ctx, _params| {
                let manifest = ctx
                    .get::<serde_json::Value>("kit_manifest")
                    .ok_or("No kit_manifest in context")?;
                let hard = jq_get(manifest, "hooks.hard_enforce")?
                    .as_array()
                    .ok_or("hooks.hard_enforce is not an array")?
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let declared: Vec<String> = jq_get(manifest, "playbooks.definitions")?
                    .as_array()
                    .ok_or("playbooks.definitions is not an array")?
                    .iter()
                    .filter_map(|d| d.get("anvil_kind").and_then(serde_json::Value::as_str))
                    .map(str::to_string)
                    .collect();
                let unresolved: Vec<&String> =
                    hard.iter().filter(|k| !declared.contains(k)).collect();
                if unresolved.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "hooks.hard_enforce entries do not resolve to declared playbook kinds {:?}: {:?}",
                        declared, unresolved
                    ))
                }
            },
        ),
    ]
}

/// Traverse a JSON value using a simple dot-path like `"kit.id"` or
/// `"mcp.servers[0].command"`. Supports object key traversal and
/// integer array indexing.
fn jq_get<'a>(val: &'a serde_json::Value, path: &str) -> Result<&'a serde_json::Value, String> {
    let mut current = val;
    for segment in path.split('.') {
        // Handle array index suffix like `servers[0]`
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            let rest = &segment[bracket..];
            // Navigate the key first
            current = current
                .get(key)
                .ok_or_else(|| format!("Key '{}' not found in path '{}'", key, path))?;
            // Then handle [n] suffix(es)
            let mut s = rest;
            while let Some(close) = s.find(']') {
                let idx_str = s[1..close].trim();
                let idx: usize = idx_str
                    .parse()
                    .map_err(|_| format!("Invalid array index '{}' in path '{}'", idx_str, path))?;
                current = current.get(idx).ok_or_else(|| {
                    format!("Array index {} out of bounds in path '{}'", idx, path)
                })?;
                s = &s[close + 1..];
            }
        } else {
            current = current
                .get(segment)
                .ok_or_else(|| format!("Key '{}' not found in path '{}'", segment, path))?;
        }
    }
    Ok(current)
}

/// Convert a JSON value to a plain string for equality comparison.
/// String values are unwrapped (no quotes); other scalars use Display.
fn json_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
