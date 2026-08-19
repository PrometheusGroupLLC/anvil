//! Step module for `residual_token_allowlist.feature`.
//!
//! Exercises the REAL `scripts/residual-workflow-tokens.py` — the A7 gate —
//! against a throwaway fixture repository built per scenario. Nothing here
//! re-implements the classifier's logic; each scenario runs the shipped script
//! as a subprocess and asserts on its exit code and reported violation kind.
//!
//! The fixture carries its own `docs/vocabulary.md`, so the referent join is
//! exercised for real: a referent the fixture matrix does not declare must be
//! rejected.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const FIXTURE_PATH_KEY: &str = "rta_fixture_path";
const FIXTURE_HANDLE_KEY: &str = "rta_fixture_handle";
const ENTRIES_KEY: &str = "rta_entries";
const EXIT_CODE_KEY: &str = "rta_exit_code";
const OUTPUT_KEY: &str = "rta_output";

/// The document whose single token every scenario classifies (or fails to).
const SAMPLE_DOC: &str = "docs/sample.md";
const SAMPLE_LINE: usize = 3;

/// The anvil repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .expect("anvil-test-support has a parent directory")
        .to_path_buf()
}

fn classifier_script() -> PathBuf {
    repo_root().join("scripts/residual-workflow-tokens.py")
}

/// A minimal referent matrix in the same shape the real `docs/vocabulary.md`
/// uses — a table whose first cell is a backticked key.
///
/// The retired-noun declaration is a BULLET list, deliberately not a table: the
/// referent-key parser matches `| \`key\` |` rows, so a table here would smuggle
/// `playbook_instance_id` in as a legal referent — the exact collapse this rule
/// exists to forbid.
fn fixture_vocabulary() -> String {
    format!("{}{}", fixture_referent_table(), fixture_retired_nouns())
}

fn fixture_referent_table() -> String {
    "\
# Fixture vocabulary

| `referent` key | Table row |
|---|---|
| `definition_artifact` | Reusable lifecycle definition artifact |
| `playbook_run` | One execution driven by a playbook |
| `governed_artifact_kind` | Governed artifact kind selected by routing |
| `execution_route` | Engine vs fallback discriminator |
| `routing_hint` | Router hint |
| `playbook_generation` | Playbook authoring kind |
| `foreign_owned` | Foreign package/evaluation/dispatch schema |
| `opaque_identity` | Opaque historical identity |
"
    .to_string()
}

fn fixture_retired_nouns() -> String {
    "\n## Retired runtime nouns\n\n\
- retired: `playbook_instance_id` — collapses definition into execution\n\
- retired: `run_instance_id` — superseded by the ratified `playbook_run_id`\n"
        .to_string()
}

fn fixture_document() -> String {
    // The classifier HUNTS the retired token, so the fixture must contain it —
    // a fixture that says "playbook" gives the gate nothing to find and every
    // scenario below goes vacuously green. This is the one file in the sweep
    // whose subject is the word itself.
    "\
# Sample document

This line still calls the reusable definition a workflow.
"
    .to_string()
}

fn write_allowlist(root: &PathBuf, entries: &str) -> Result<(), String> {
    let body = format!(
        "version: 1\nscope:\n  include:\n    - \"docs/**/*.md\"\n  exclude:\n    - \".git/**\"\n{}",
        entries
    );
    std::fs::write(root.join("residual-tokens.allowlist.yaml"), body)
        .map_err(|e| format!("failed to write fixture allowlist: {e}"))
}

/// One complete, well-formed entry with the given values. `context_proof` is
/// emitted only when the gate is NOT `not_applicable`, so the
/// missing-context-proof scenario has a real red path.
fn entry_yaml(category: &str, referent: &str, gate: &str) -> String {
    entry_yaml_with(&format!("{SAMPLE_LINE}"), Some(1), category, referent, gate)
}

/// One complete entry with an explicit selector and declared hit count.
/// `hits: None` omits the field entirely, which is itself a violation — that is
/// the point of the omission scenario.
fn entry_yaml_with(
    selector: &str,
    hits: Option<usize>,
    category: &str,
    referent: &str,
    gate: &str,
) -> String {
    let mut s = format!("  - file: \"{SAMPLE_DOC}\"\n    selector: {selector}\n");
    if let Some(n) = hits {
        s.push_str(&format!("    hits: {n}\n"));
    }
    s.push_str(&format!(
        "    category: {category}\n    referent: {referent}\n    owner: \"fixture owner\"\n    gate: \"{gate}\"\n"
    ));
    if gate != "not_applicable" {
        s.push_str("    note: \"classified by the fixture\"\n");
    }
    s
}

/// Re-emit the fixture keys so downstream steps still see them: a step's output
/// context REPLACES the prior one, so anything not re-provided is lost (and the
/// TempDir handle would drop, deleting the fixture mid-scenario).
fn carry(ctx: &Context) -> Result<Context, String> {
    let root = ctx
        .get::<PathBuf>(FIXTURE_PATH_KEY)
        .ok_or("No fixture path")?
        .clone();
    let handle = ctx
        .get::<Arc<Mutex<Option<tempfile::TempDir>>>>(FIXTURE_HANDLE_KEY)
        .ok_or("No fixture handle")?
        .clone();
    let mut out = Context::new();
    out.set(FIXTURE_PATH_KEY, root);
    out.set(FIXTURE_HANDLE_KEY, handle);
    Ok(out)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a fixture repository whose only user-visible playbook token is in a document",
            &[],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |_ctx, _params| {
                let temp = tempfile::TempDir::new()
                    .map_err(|e| format!("failed to create fixture dir: {e}"))?;
                let root = temp.path().to_path_buf();
                std::fs::create_dir_all(root.join("docs"))
                    .map_err(|e| format!("failed to create docs/: {e}"))?;
                std::fs::write(root.join("docs/vocabulary.md"), fixture_vocabulary())
                    .map_err(|e| format!("failed to write fixture vocabulary: {e}"))?;
                std::fs::write(root.join(SAMPLE_DOC), fixture_document())
                    .map_err(|e| format!("failed to write fixture document: {e}"))?;

                let handle: Arc<Mutex<Option<tempfile::TempDir>>> =
                    Arc::new(Mutex::new(Some(temp)));
                let mut out = Context::new();
                out.set(FIXTURE_PATH_KEY, root);
                out.set(FIXTURE_HANDLE_KEY, handle);
                out.set(ENTRIES_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "the allowlist classifies nothing",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                write_allowlist(&root, "allowlist: []\n")?;
                let mut out = carry(&ctx)?;
                out.set(ENTRIES_KEY, String::new());
                Ok(out)
            },
        ),
        step_def(
            "the allowlist classifies the token with category {string} and referent {string} and gate {string}",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |ctx, params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let category = params.get_string(0).ok_or("Expected category")?.to_string();
                let referent = params.get_string(1).ok_or("Expected referent")?.to_string();
                let gate = params.get_string(2).ok_or("Expected gate")?.to_string();
                let entries = format!("allowlist:\n{}", entry_yaml(&category, &referent, &gate));
                write_allowlist(&root, &entries)?;
                let mut out = carry(&ctx)?;
                out.set(ENTRIES_KEY, entries);
                Ok(out)
            },
        ),
        step_def(
            "the allowlist exempts the whole document",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let entries = format!(
                    "allowlist:\n{}",
                    entry_yaml_with("file", Some(1), "legacy_adapter", "definition_artifact", "NG-PROTO-VNEXT")
                );
                write_allowlist(&root, &entries)?;
                let mut out = carry(&ctx)?;
                out.set(ENTRIES_KEY, entries);
                Ok(out)
            },
        ),
        step_def(
            "the allowlist exempts the whole document without declaring how many tokens it covers",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let entries = format!(
                    "allowlist:\n{}",
                    entry_yaml_with("file", None, "legacy_adapter", "definition_artifact", "NG-PROTO-VNEXT")
                );
                write_allowlist(&root, &entries)?;
                let mut out = carry(&ctx)?;
                out.set(ENTRIES_KEY, entries);
                Ok(out)
            },
        ),
        step_def(
            "someone later writes another retired token into that same document",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let path = root.join(SAMPLE_DOC);
                let existing = std::fs::read_to_string(&path)
                    .map_err(|e| format!("failed to read fixture document: {e}"))?;
                std::fs::write(
                    &path,
                    format!("{existing}\nAnd this brand-new line calls it a workflow too.\n"),
                )
                .map_err(|e| format!("failed to append to fixture document: {e}"))?;
                carry(&ctx)
            },
        ),
        step_def(
            "the allowlist also classifies a token in a file that has none",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (ENTRIES_KEY, "String"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let existing = ctx.get::<String>(ENTRIES_KEY).ok_or("No entries")?.clone();
                // A second document with no token at all, plus an entry claiming one.
                std::fs::write(root.join("docs/clean.md"), "# Clean\n\nNothing to see.\n")
                    .map_err(|e| format!("failed to write clean document: {e}"))?;
                let entries = format!(
                    "{existing}  - file: \"docs/clean.md\"\n    selector: 3\n    hits: 1\n    category: legacy_adapter\n    referent: definition_artifact\n    owner: \"fixture owner\"\n    gate: \"NG-PROTO-VNEXT\"\n"
                );
                write_allowlist(&root, &entries)?;
                let mut out = carry(&ctx)?;
                out.set(ENTRIES_KEY, entries);
                Ok(out)
            },
        ),
        step_def(
            "a source file names the runtime {string}",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let noun = params.get_string(0).ok_or("Expected a runtime noun")?.to_string();
                std::fs::create_dir_all(root.join("src"))
                    .map_err(|e| format!("failed to create src/: {e}"))?;
                // Carries the noun and NOT the `workflow` token, so the only
                // thing this file can trip is the retired-noun sweep.
                std::fs::write(
                    root.join("src/lib.rs"),
                    format!("pub struct Record {{\n    pub {noun}: Option<String>,\n}}\n"),
                )
                .map_err(|e| format!("failed to write fixture source: {e}"))?;
                carry(&ctx)
            },
        ),
        step_def(
            "the vocabulary declares no retired runtime nouns",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                std::fs::write(root.join("docs/vocabulary.md"), fixture_referent_table())
                    .map_err(|e| format!("failed to rewrite fixture vocabulary: {e}"))?;
                carry(&ctx)
            },
        ),
        step_def(
            "the residual-token classifier runs",
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
            ],
            &[
                (FIXTURE_PATH_KEY, "PathBuf"),
                (FIXTURE_HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (EXIT_CODE_KEY, "i64"),
                (OUTPUT_KEY, "String"),
            ],
            |ctx, _params| {
                let root = ctx.get::<PathBuf>(FIXTURE_PATH_KEY).ok_or("No fixture path")?.clone();
                let script = classifier_script();
                if !script.is_file() {
                    return Err(format!("classifier not found at {}", script.display()));
                }
                let output = std::process::Command::new("python3")
                    .arg(&script)
                    .arg("--root")
                    .arg(&root)
                    .arg("--allowlist")
                    .arg(root.join("residual-tokens.allowlist.yaml"))
                    .output()
                    .map_err(|e| format!("failed to run classifier: {e}"))?;
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let code = output.status.code().unwrap_or(-1) as i64;
                let mut out = carry(&ctx)?;
                out.set(EXIT_CODE_KEY, code);
                out.set(OUTPUT_KEY, combined);
                Ok(out)
            },
        ),
        check_def(
            "the classifier fails reporting {string}",
            &[(EXIT_CODE_KEY, "i64"), (OUTPUT_KEY, "String")],
            |ctx, params| {
                let code = *ctx.get::<i64>(EXIT_CODE_KEY).ok_or("No exit code")?;
                let output = ctx.get::<String>(OUTPUT_KEY).ok_or("No output")?.clone();
                let want = params.get_string(0).ok_or("Expected violation kind")?.to_string();
                if code == 0 {
                    return Err(format!(
                        "expected a non-zero exit, got 0. Output:\n{output}"
                    ));
                }
                if !output.contains(&want) {
                    return Err(format!(
                        "expected violation `{want}` in classifier output. Output:\n{output}"
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the classifier passes",
            &[(EXIT_CODE_KEY, "i64"), (OUTPUT_KEY, "String")],
            |ctx, _params| {
                let code = *ctx.get::<i64>(EXIT_CODE_KEY).ok_or("No exit code")?;
                let output = ctx.get::<String>(OUTPUT_KEY).ok_or("No output")?.clone();
                if code != 0 {
                    return Err(format!("expected exit 0, got {code}. Output:\n{output}"));
                }
                if !output.contains("violations:0") {
                    return Err(format!(
                        "expected zero violations reported. Output:\n{output}"
                    ));
                }
                Ok(())
            },
        ),
    ]
}
