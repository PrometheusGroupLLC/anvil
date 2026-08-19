//! Step module for playbook_skills_ownership_language.feature.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::{Path, PathBuf};

const FILE_PATH_KEY: &str = "wsol_file_path";
const FILE_CONTENT_KEY: &str = "wsol_file_content";
const SKILL_DIR_KEY: &str = "wsol_skill_dir";
const SKILL_FILE_CONTENT_KEY: &str = "wsol_skill_file_content";

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the file at {string} exists",
            &[],
            &[(FILE_PATH_KEY, "PathBuf"), (FILE_CONTENT_KEY, "String")],
            |_ctx, params| {
                let path = resolve_repo_path(&params.get_string(0).ok_or("Expected path")?);
                if !path.is_file() {
                    return Err(format!("Expected file to exist: {}", path.display()));
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let mut out = Context::new();
                out.set(FILE_PATH_KEY, path);
                out.set(FILE_CONTENT_KEY, content);
                Ok(out)
            },
        ),
        step_def(
            "the Skills section is scanned for {string} ownership claims",
            &[(FILE_CONTENT_KEY, "String")],
            &[(FILE_CONTENT_KEY, "String")],
            |ctx, _params| Ok(ctx.clone()),
        ),
        step_def(
            "the Skills section is scanned for {string} conflation language",
            &[(FILE_CONTENT_KEY, "String")],
            &[(FILE_CONTENT_KEY, "String")],
            |ctx, _params| Ok(ctx.clone()),
        ),
        check_def(
            "no lines in the Skills section contain the phrase {string}",
            &[(FILE_CONTENT_KEY, "String")],
            |ctx, params| {
                let phrase = params.get_string(0).ok_or("Expected phrase")?;
                let content = ctx
                    .get::<String>(FILE_CONTENT_KEY)
                    .ok_or("No file content to scan")?;
                let offending: Vec<_> = skills_section_lines(content)
                    .into_iter()
                    .filter(|line| line.contains(&phrase))
                    .collect();
                if offending.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Skills section contains phrase '{}': {}",
                        phrase,
                        offending.join(" | ")
                    ))
                }
            },
        ),
        step_def(
            "the forge skill directory is accessible at {string}",
            &[],
            &[(SKILL_DIR_KEY, "PathBuf")],
            |_ctx, params| {
                let path = resolve_repo_path(&params.get_string(0).ok_or("Expected path")?);
                if !path.is_dir() {
                    return Err(format!("Expected directory to exist: {}", path.display()));
                }
                let mut out = Context::new();
                out.set(SKILL_DIR_KEY, path);
                Ok(out)
            },
        ),
        step_def(
            "the file {string} is read from the forge skill directory",
            &[(SKILL_DIR_KEY, "PathBuf")],
            &[(SKILL_FILE_CONTENT_KEY, "String")],
            |ctx, params| {
                let filename = params.get_string(0).ok_or("Expected filename")?;
                let dir = ctx
                    .get::<PathBuf>(SKILL_DIR_KEY)
                    .ok_or("No forge skill directory")?;
                let path = dir.join(filename);
                if !path.is_file() {
                    return Err(format!("Expected file to exist: {}", path.display()));
                }
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                let mut out = Context::new();
                out.set(SKILL_FILE_CONTENT_KEY, content);
                Ok(out)
            },
        ),
        check_def(
            "the first H1 heading in the file is {string}",
            &[(SKILL_FILE_CONTENT_KEY, "String")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected heading")?;
                let content = ctx
                    .get::<String>(SKILL_FILE_CONTENT_KEY)
                    .ok_or("No skill file content")?;
                let actual = content
                    .lines()
                    .find(|line| line.starts_with("# "))
                    .ok_or("No H1 heading found")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected first H1 heading '{}' but got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
    ]
}

fn resolve_repo_path(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() || path.exists() {
        return path;
    }

    let workspace_root = Path::new(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .expect("workspace root");

    if let Some(stripped) = raw.strip_prefix("anvil/") {
        let stripped_path = workspace_root.join(stripped);
        if stripped_path.exists() {
            return stripped_path;
        }
    }

    workspace_root.join(raw)
}

fn skills_section_lines(content: &str) -> Vec<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if lower.starts_with('#') && lower.contains("skills") {
            in_section = true;
            lines.push(line.to_string());
            continue;
        }
        if in_section && lower.starts_with('#') {
            break;
        }
        if in_section {
            lines.push(line.to_string());
        }
    }
    if lines.is_empty() {
        content.lines().map(ToString::to_string).collect()
    } else {
        lines
    }
}
