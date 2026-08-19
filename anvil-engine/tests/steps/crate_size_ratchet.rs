//! Crate-size ratchet guard.
//!
//! The measurement itself lives in `scripts/check-crate-size.sh` and is NOT reimplemented
//! here. Two copies of a size rule would drift, and the day they disagree is the day the
//! guard stops meaning anything — the same lesson as the seed-registry parity guard.
//! These steps run the one canonical implementation and assert on its exit code.
//!
//! The second scenario is the important one: it lowers a COPY of the baseline below the
//! real sizes and requires the script to fail. A ratchet nobody has watched fail is
//! indistinguishable from a ratchet that cannot fail.

use anvil_test_support::{check_def, step_def, Context, StepDef};
use std::path::{Path, PathBuf};
use std::process::Command;

const SCRIPT_KEY: &str = "crate_size_script";
const BASELINE_KEY: &str = "crate_size_baseline";
const STATUS_KEY: &str = "crate_size_status";
const OUTPUT_KEY: &str = "crate_size_output";

fn repo_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "no workspace root above anvil-engine".to_string())
}

fn run(script: &Path, baseline: &Path, root: &Path) -> Result<(i32, String), String> {
    let out = Command::new("bash")
        .arg(script)
        .arg("--root")
        .arg(root)
        .arg("--baseline")
        .arg(baseline)
        .output()
        .map_err(|e| format!("run {}: {e}", script.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the workspace crate size baseline",
            &[],
            &[(SCRIPT_KEY, "PathBuf"), (BASELINE_KEY, "PathBuf")],
            |_, _| {
                let root = repo_root()?;
                let script = root.join("scripts/check-crate-size.sh");
                let baseline = root.join(".crate-size-baseline");
                if !script.is_file() {
                    return Err(format!("missing {}", script.display()));
                }
                // A missing baseline would make the script WRITE one and exit 0, so the
                // scenario would pass while measuring nothing. Fail instead.
                if !baseline.is_file() {
                    return Err(format!(
                        "missing {} — the ratchet has no recorded baseline, so it cannot \
                         detect growth. Run scripts/check-crate-size.sh and commit it.",
                        baseline.display()
                    ));
                }
                let mut out = Context::new();
                out.set(SCRIPT_KEY, script);
                out.set(BASELINE_KEY, baseline);
                Ok(out)
            },
        ),
        step_def(
            "the crate sizes are measured",
            &[(SCRIPT_KEY, "PathBuf"), (BASELINE_KEY, "PathBuf")],
            &[(STATUS_KEY, "i32"), (OUTPUT_KEY, "String")],
            |ctx, _| {
                let script = ctx.get::<PathBuf>(SCRIPT_KEY).ok_or("no script")?.clone();
                let baseline = ctx.get::<PathBuf>(BASELINE_KEY).ok_or("no baseline")?.clone();
                // Measure against a COPY: the script ratchets a shrunken baseline DOWN in
                // place, and a test must not rewrite a committed file as a side effect.
                let tmp = std::env::temp_dir().join(format!(
                    "anvil-crate-size-{}",
                    std::process::id()
                ));
                std::fs::copy(&baseline, &tmp).map_err(|e| format!("copy baseline: {e}"))?;
                let (code, text) = run(&script, &tmp, &repo_root()?)?;
                let _ = std::fs::remove_file(&tmp);
                let mut out = Context::new();
                out.set(STATUS_KEY, code);
                out.set(OUTPUT_KEY, text);
                Ok(out)
            },
        ),
        check_def(
            "no crate exceeds its baseline",
            &[(STATUS_KEY, "i32"), (OUTPUT_KEY, "String")],
            |ctx, _| {
                let code = *ctx.get::<i32>(STATUS_KEY).ok_or("no status")?;
                let text = ctx.get::<String>(OUTPUT_KEY).ok_or("no output")?;
                if code == 0 {
                    return Ok(());
                }
                Err(format!(
                    "a crate grew past its baseline. rustc holds a whole crate's IR in \
                     memory at once, so this is a build-memory hazard, not a style \
                     preference — accumulate-test-support reached 169,616 lines and OOM'd \
                     this machine four times in one day. Split the crate, or run \
                     `scripts/check-crate-size.sh --update-baseline` and commit the new \
                     number as a deliberate decision.\n{text}"
                ))
            },
        ),
        step_def(
            "a crate is measured against a baseline lowered below its current size",
            &[(SCRIPT_KEY, "PathBuf"), (BASELINE_KEY, "PathBuf")],
            &[(STATUS_KEY, "i32"), (OUTPUT_KEY, "String")],
            |ctx, _| {
                let script = ctx.get::<PathBuf>(SCRIPT_KEY).ok_or("no script")?.clone();
                let baseline = ctx.get::<PathBuf>(BASELINE_KEY).ok_or("no baseline")?.clone();
                let real = std::fs::read_to_string(&baseline)
                    .map_err(|e| format!("read baseline: {e}"))?;
                // Halve every recorded size in a COPY. Every crate is then "grown", so the
                // script must fail — if it still passes, the ratchet is inert.
                let lowered: String = real
                    .lines()
                    .map(|line| {
                        if line.starts_with('#') || line.trim().is_empty() {
                            return line.to_string();
                        }
                        match line.split_once(' ') {
                            Some((name, n)) => match n.trim().parse::<u64>() {
                                Ok(v) => format!("{name} {}", v / 2),
                                Err(_) => line.to_string(),
                            },
                            None => line.to_string(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let tmp = std::env::temp_dir().join(format!(
                    "anvil-crate-size-mutated-{}",
                    std::process::id()
                ));
                std::fs::write(&tmp, format!("{lowered}\n"))
                    .map_err(|e| format!("write mutated baseline: {e}"))?;
                let (code, text) = run(&script, &tmp, &repo_root()?)?;
                let _ = std::fs::remove_file(&tmp);
                let mut out = Context::new();
                out.set(STATUS_KEY, code);
                out.set(OUTPUT_KEY, text);
                Ok(out)
            },
        ),
        check_def(
            "the ratchet reports a violation",
            &[(STATUS_KEY, "i32"), (OUTPUT_KEY, "String")],
            |ctx, _| {
                let code = *ctx.get::<i32>(STATUS_KEY).ok_or("no status")?;
                let text = ctx.get::<String>(OUTPUT_KEY).ok_or("no output")?;
                if code != 0 && text.contains("exceeds its baseline") {
                    return Ok(());
                }
                Err(format!(
                    "the ratchet did NOT fail against a baseline halved below every real \
                     crate size (exit {code}). A guard that cannot fail protects nothing.\n{text}"
                ))
            },
        ),
    ]
}
