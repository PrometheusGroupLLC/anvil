//! Every workspace root must declare the SAME `[workspace.dependencies]`.
//!
//! Splitting the private-sibling consumers out of the default workspace bought
//! a repository that `cargo build`s with no private siblings at all — the Brine
//! runners into `brine-tests/` (which needs `brine`), and the Foundry-coupled
//! engine build into `kit-build/` (which needs `foundry-kit-broker-client`).
//! The cost is one genuine duplication: cargo offers no way to inherit a
//! `[workspace.dependencies]` table across workspaces, so the table exists once
//! per root.
//!
//! N hand-maintained copies of a table WILL drift. The rule is: make one
//! canonical source and generate the rest — and where the tool forbids that,
//! GUARD every copy. This is that guard. Change one table, change the others, or
//! this test says so by name.
//!
//! The list below is hand-written, so `every_excluded_workspace_is_guarded`
//! cross-checks it against the root manifest's `exclude` list: a new excluded
//! workspace that brings a third copy of the table cannot slip past unguarded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Locate the repository root by walking up for a marker, never by counting
/// `parent()` hops — this test's own crate can be reached from either
/// workspace.
fn repo_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in start.ancestors() {
        if dir.join("anvil-core").join("features").is_dir() {
            return dir.to_path_buf();
        }
    }
    panic!("could not locate the repository root above {}", start.display());
}

/// Extract the `[workspace.dependencies]` table as `name -> requirement`,
/// ignoring comments and blank lines.
fn workspace_dependencies(manifest: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(manifest)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));

    let mut out = BTreeMap::new();
    let mut in_table = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            // A new table header ends the one we care about.
            in_table = line == "[workspace.dependencies]";
            continue;
        }
        if !in_table || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            out.insert(name.trim().to_string(), value.trim().to_string());
        }
    }
    assert!(
        !out.is_empty(),
        "no [workspace.dependencies] entries parsed from {} — the guard would \
         have passed vacuously",
        manifest.display()
    );
    out
}

/// The non-default workspace roots, each of which carries its own copy of the
/// table. Kept in sync with the root manifest's `exclude` list by
/// `every_excluded_workspace_is_guarded`.
const SECONDARY_WORKSPACES: &[&str] = &["brine-tests", "kit-build"];

/// # Absent vs. broken, and why the difference is the whole design
///
/// This test used to assert that every secondary root's `Cargo.toml` EXISTS,
/// on the correct reasoning that a missing manifest means the guard compared
/// nothing. In the public export mirror both secondary workspaces are trimmed
/// (`scripts/anvil-public-staging-EXPORT.sh` does not include them), so the
/// assertion turned the mirror's first `cargo test` into a panic naming a
/// directory that is not in the repository the stranger cloned. Its sibling
/// `no_private_path_deps.rs` shipped the identical defect; this one went
/// unnoticed for longer because the review only reached the sibling.
///
/// "Skip if missing" is NOT the fix — that is the silent no-op the original
/// assertion existed to prevent. The discriminator is the DIRECTORY:
///
/// * the directory is absent      ⇒ a trimmed export. Skip, and say so.
/// * the directory exists but has no `Cargo.toml` ⇒ a broken checkout, or a
///   manifest someone moved. PANIC, exactly as before.
///
/// Every entry in `SECONDARY_WORKSPACES` must land in one bucket or the other,
/// and the tally is printed on every run. In this repository both directories
/// exist, so nothing here can degrade to zero comparisons; in the mirror there
/// is only ONE copy of the table, so zero comparisons is the correct and
/// complete answer rather than a gap.
#[test]
fn every_workspace_root_declares_identical_dependencies() {
    let root = repo_root();
    let default_ws = workspace_dependencies(&root.join("Cargo.toml"));

    let mut problems = Vec::new();
    let mut roots_compared = 0;
    let mut compared: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();

    for workspace in SECONDARY_WORKSPACES {
        let dir = root.join(workspace);
        let manifest = dir.join("Cargo.toml");
        if !dir.is_dir() {
            skipped.push(workspace);
            continue;
        }
        assert!(
            manifest.is_file(),
            "`{workspace}/` exists but has no Cargo.toml at {} — this guard \
             would have silently compared nothing. An ABSENT directory is a \
             trimmed export and is skipped; a directory with no manifest is a \
             broken checkout or a moved manifest, and is not.",
            manifest.display()
        );
        let other_ws = workspace_dependencies(&manifest);
        roots_compared += 1;
        compared.push(workspace);

        for (name, req) in &default_ws {
            match other_ws.get(name) {
                None => problems.push(format!(
                    "`{name}` is declared in Cargo.toml but MISSING from {workspace}/Cargo.toml"
                )),
                Some(other) if other != req => problems.push(format!(
                    "`{name}` differs:\n     Cargo.toml: {req}\n  {workspace}/: {other}"
                )),
                Some(_) => {}
            }
        }
        for name in other_ws.keys() {
            if !default_ws.contains_key(name) {
                problems.push(format!(
                    "`{name}` is declared in {workspace}/Cargo.toml but MISSING from Cargo.toml"
                ));
            }
        }
    }

    // Printed on every run, pass or fail: "it passed" is worthless without
    // "over what". `default_ws` is guaranteed non-vacuous by
    // `workspace_dependencies`, so the one thing this test always proves is
    // that the canonical table parsed.
    println!(
        "workspace-parity guard: {} canonical dependencies; compared {} secondary root(s) \
         {compared:?}; skipped {} absent (trimmed-export) root(s) {skipped:?}",
        default_ws.len(),
        compared.len(),
        skipped.len()
    );

    // Every declared root is accounted for as either compared or absent. A
    // root cannot go missing from BOTH tallies, which is the shape a silent
    // no-op would need.
    assert_eq!(
        roots_compared + skipped.len(),
        SECONDARY_WORKSPACES.len(),
        "accounted for {} of {} declared secondary roots ({roots_compared} compared, \
         {} skipped) — every root must land in exactly one bucket",
        roots_compared + skipped.len(),
        SECONDARY_WORKSPACES.len(),
        skipped.len()
    );

    assert!(
        problems.is_empty(),
        "the [workspace.dependencies] tables have drifted:\n  - {}",
        problems.join("\n  - ")
    );
}

/// A guard over a hand-written list silently stops covering anything added
/// after it was written. The root manifest's `exclude` list is the authoritative
/// enumeration of the other workspaces in this repository, so require the two to
/// agree rather than trusting whoever adds the next one to remember this file.
#[test]
fn every_excluded_workspace_is_guarded() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");

    let exclude_block = text
        .split_once("exclude = [")
        .expect("root Cargo.toml has no `exclude = [`")
        .1
        .split_once(']')
        .expect("unterminated exclude list")
        .0;

    let excluded: Vec<String> = exclude_block
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('"'))
        .map(|l| l.trim_matches(|c| c == '"' || c == ',').to_string())
        .collect();

    let guarded: Vec<String> = SECONDARY_WORKSPACES.iter().map(|s| s.to_string()).collect();

    assert_eq!(
        excluded, guarded,
        "the root manifest's excluded workspaces changed; update \
         SECONDARY_WORKSPACES in this test so every copy of \
         [workspace.dependencies] stays guarded"
    );
}
