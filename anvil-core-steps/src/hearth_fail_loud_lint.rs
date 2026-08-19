//! Step definitions for `anvil-core/features/hearth_fail_loud_lint.feature`.
//!
//! A TRIPWIRE FOR THE OBVIOUS SPELLING. **Not a ban, and not enforcement.**
//! C-d.1 round 5 (HIGH-1), corrected in round 6.
//!
//! `Path::is_dir()`, `Path::is_file()` and `Path::exists()` return `bool` and
//! map every error to `false`. On the hearth-root guard path `false` means
//! "there is nothing here" — the answer on which every guard concludes nothing
//! would be lost and clears a writer. That defect was filed in round 3, closed
//! at ONE syscall in round 4, and reproduced at three further sites by the
//! round-4 review.
//!
//! # What this check is, stated so it cannot be over-read
//!
//! It is `line.contains(needle)` over the non-comment lines of TWO hand-named
//! files. Round 5 shipped it claiming it made the swallow *unrepresentable*. An
//! independent review then wrote the same swallow, at the same site, with the
//! same behaviour, **ten** different ways — UFCS (`Path::is_dir(&p)`), a type
//! alias, the receiver and the method on different SOURCE LINES,
//! `matches!(metadata(p), Ok(_))`, `.ok().is_some()`, `.is_err()`,
//! `.unwrap_or_default()`, a `-> bool` helper in a third module, a
//! `pub(crate) -> bool` helper inside `fs_probe` itself, and the same split-line
//! trick at a real round-4 site. All ten compiled. All ten left this check
//! GREEN. One of them differs from a spelling this check DOES catch only by
//! where a newline falls.
//!
//! A lexical test cannot close a class whose every instance has been a new
//! lexeme: `read_dir`'s `Err` -> `entry.path().is_dir()` -> `.is_file()` ->
//! `.exists()`. So the honest description is: **this catches a developer who
//! reintroduces the defect the way it was written the last three times.** That
//! is worth having — it is cheap, and it states its rationale at the point of
//! failure — and it is all it is.
//!
//! # What actually closes the class, and where
//!
//! Structure, not scanning. Round 6 hoisted the hearth-root guard's I/O to one
//! edge (`registry_projection::survey_hearth_roots`) producing an owned,
//! path-free description (`HearthRootFacts`), and the code that DECIDES —
//! `decide_hearth_roots`, the function that concludes "nothing would be lost"
//! and clears a writer — receives only that. Round 6 also moved the loader's
//! only filesystem access out to `fs_probe`, leaving `domain::playbook::loader`
//! with no path type in scope at all.
//!
//! **Round 7, M-1 — this paragraph used to end "not 'is not allowed to', but
//! *cannot*, at compile time, in every spelling at once", and that is FALSE.**
//! `std::path` and `std::fs` are reachable by absolute path from every module in
//! Rust, and `String` is `AsRef<Path>`. The round-6 reviewer wrote ATK-1, ATK-2
//! and ATK-3 inside `decide_hearth_roots` with the binding created on the line
//! above; it compiles, the lint stays 2/2 GREEN and the projection suite stays
//! 56/56 GREEN. The three mutations round 6 reported as compile errors failed on
//! a BINDING NAME (`hearth`, `legacy`, `p`), not on the bypass. That is the same
//! lexical sensitivity the hoist was undertaken to escape, one level up.
//!
//! **The true property, stated as what it is:** the decision holds no path
//! NAMING THE HEARTH — `HearthRootFacts` carries `Vec<String>` basenames and
//! unit variants and nothing else — so nothing written there can reach the
//! hearth's roots. A predicate in the decision can only probe a path the author
//! constructs out of thin air. To make the decision answer wrongly about THIS
//! hearth you must also change the edge or the type. Narrower than "unwritable",
//! and worth having.
//!
//! Everywhere else on the guard path, this check is a tripwire. The measured
//! bypass count is now ELEVEN, not ten: a type alias (`type Flag = bool;
//! … -> Flag`) defeats the `-> bool` scenario below, which widens a declared
//! residual rather than contradicting a claim.
//!
//! # Coverage, declared rather than implied
//!
//! [`GUARD_MODULES`] is TWO hand-written paths. See its own comment for the
//! modules on the guard path that it does NOT scan and why.

use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;

const SUBJECTS_KEY: &str = "hfl_subjects";

/// The modules this check scans, relative to the workspace root.
///
/// Both are modules whose whole job is to answer "what is on disk here?" for a
/// caller that will decide whether a definition would be LOST.
///
/// # THIS LIST IS TWO OF FOUR, and that is declared, not implied
///
/// C-d.1 round 6, M-1. The round-5 review proved the guard path is wider than
/// this list by finding the class LIVE on unmutated HEAD one frame below a
/// scanned module. The two modules NOT scanned, and their disposition:
///
/// * **`anvil-core/src/domain/playbook/loader.rs`** — reached from the scanned
///   `hearth_registry::hook_file_names`. It carried the round-3 AND round-4
///   signatures verbatim, so an unreadable `hooks/` reported a hook file that is
///   on disk as an unknown reference and dropped the definition. **Fixed
///   structurally**: its only filesystem access moved to `fs_probe`, and the
///   module now has no path type in scope at all. It is not on this list because
///   it no longer needs to be — and because it legitimately uses
///   `.unwrap_or(false)` three times on pure YAML/graph logic, which is the
///   other half of why a shared substring list does not generalise.
/// * **`anvil-core-hearth/src/fs_snapshot_adapter.rs`** — the sole production
///   consumer of `resolve_generation_identity`, which it BRACKETED with two
///   `exists()` calls. **Those two are fixed** (they go through `fs_probe` and
///   answer `IoError`, not `NotFound`). The module is not scanned: it is 1600+
///   lines of general artifact I/O with legitimate uses of these predicates
///   outside the guard path, so adding it would mean either exempting most of it
///   or reds that carry no signal — and a check that has to be silenced is a
///   check that gets deleted.
///
/// The list is NOT derived from the call graph. Deriving it would need name
/// resolution this Brine step does not have, and hand-maintaining a list that
/// CLAIMS to be call-graph-complete is worse than a short list that says what it
/// covers. What it covers is: two modules, for three historical spellings.
const GUARD_MODULES: &[&str] = &[
    "anvil-core/src/domain/playbook/registry_projection.rs",
    "anvil-core/src/domain/playbook/hearth_registry.rs",
];

/// The one module allowed to call the raw predicates, because it is the one that
/// turns them into a `Result`.
const PROBE_MODULE: &str = "anvil-core/src/domain/playbook/fs_probe.rs";

/// Every spelling of the swallow that has actually been shipped on this path,
/// plus the two obvious ways to write it back without naming the predicates.
///
/// `.is_ok()` and `.unwrap_or(false)` are here because they are how a
/// `std::fs::metadata(..)` call gets collapsed into the same `bool` the ban
/// exists to remove — banning only the three predicates would leave the swallow
/// one refactor away.
const BANNED: &[(&str, &str)] = &[
    (".is_dir()", "maps EACCES/ESTALE/EIO onto 'not a directory'"),
    (".is_file()", "maps EACCES/ESTALE/EIO onto 'not a file'"),
    (".exists()", "maps EACCES/ESTALE/EIO onto 'absent'"),
    (".is_ok()", "collapses a fallible probe back into a bool"),
    (
        ".unwrap_or(false)",
        "names the swallow outright: an error becomes 'no'",
    ),
];

fn workspace_root() -> PathBuf {
    // `anvil-test-support/` -> workspace root. The runner's cwd is the CRATE
    // directory under test, which is not stable across crates; the manifest dir
    // is.
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Source lines that are not comments.
///
/// Comment lines are excluded because the modules' own doc comments QUOTE the
/// banned predicates in order to explain the ban — a lint that fired on its own
/// rationale would be deleted within the week, and a lint that has been deleted
/// bans nothing.
fn code_lines(source: &str) -> Vec<(usize, &str)> {
    source
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
        .collect()
}

/// Every NON-PRIVATE function signature in `source`, as `(line number, text)`.
///
/// C-d.1 round 6, L-2. The previous check matched only lines beginning
/// `pub fn `, which is one of five ways Rust spells a visibility a caller
/// outside this module can reach: `pub(crate)`, `pub(super)`, `pub(in path)`
/// and `pub(self)` all passed it, and `pub(crate) fn looks_present(..) -> bool`
/// inside the probe module was the round-5 reviewer's ATK-9.
///
/// The signature text is joined across lines up to the opening brace, so a
/// two-line signature — `pub fn f(` on one line and `) -> bool {` on the next —
/// is read as the one signature it is. Ten lines is a generous bound; nothing in
/// this module has a signature anywhere near it, and an unbounded join would let
/// one unbalanced line swallow the rest of the file.
fn signature_starts(source: &str) -> Vec<(usize, String)> {
    const VISIBILITIES: &[&str] = &["pub fn ", "pub(crate) fn ", "pub(super) fn ", "pub(in "];
    let lines: Vec<&str> = source.lines().collect();
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let is_signature = VISIBILITIES.iter().any(|v| t.starts_with(v))
            || (t.starts_with("pub(self) fn ") || t.starts_with("pub (crate) fn "));
        if !is_signature {
            continue;
        }
        let mut sig = String::new();
        for line in lines.iter().skip(i).take(10) {
            let piece = line.trim();
            if piece.starts_with("//") {
                continue;
            }
            sig.push(' ');
            sig.push_str(piece);
            // The signature ends at the body's brace or at a trait/impl `;`.
            if piece.ends_with('{') || piece.ends_with(';') {
                break;
            }
        }
        out.push((i + 1, sig));
    }
    out
}

/// Does a joined signature return a bare `bool`?
///
/// `-> bool` and `->bool`, with or without a trailing brace, and NOT
/// `-> io::Result<bool>` / `-> Option<bool>` — a fallible or optional `bool` is
/// not the swallow. The swallow is specifically the answer that CANNOT carry "I
/// could not look".
fn returns_bool(signature: &str) -> bool {
    let normalized = signature.replace("->", " -> ");
    let mut parts = normalized.split(" -> ");
    let _before = parts.next();
    let Some(after) = parts.next() else {
        return false;
    };
    let ret = after
        .trim()
        .trim_end_matches('{')
        .trim()
        .trim_end_matches(';')
        .trim();
    ret == "bool"
}

fn read_subject(rel: &str) -> Result<String, String> {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "the ban cannot read its own subject {}: {e}. A lint that cannot find the file it \
             guards passes vacuously, which is worse than no lint.",
            path.display()
        )
    })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the hearth-root guard modules",
            &[],
            &[(SUBJECTS_KEY, "string")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(SUBJECTS_KEY, GUARD_MODULES.join(","));
                Ok(out)
            },
        ),
        step_def(
            "the fallible filesystem probe module",
            &[],
            &[(SUBJECTS_KEY, "string")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(SUBJECTS_KEY, PROBE_MODULE.to_string());
                Ok(out)
            },
        ),
        check_def(
            "every guard module is present and non-empty",
            &[(SUBJECTS_KEY, "string")],
            |ctx, _params| {
                // ANTI-VACUITY. A rename, a move, or a typo in the list above
                // would otherwise turn this whole gate green over zero bytes.
                let subjects = ctx.get::<String>(SUBJECTS_KEY).ok_or("no subjects")?;
                for rel in subjects.split(',') {
                    let source = read_subject(rel)?;
                    if source.trim().is_empty() {
                        return Err(format!("{rel} is empty — the ban would scan nothing"));
                    }
                }
                Ok(())
            },
        ),
        check_def(
            "no guard module calls a bool-returning filesystem predicate",
            &[(SUBJECTS_KEY, "string")],
            |ctx, _params| {
                let subjects = ctx.get::<String>(SUBJECTS_KEY).ok_or("no subjects")?;
                let mut hits: Vec<String> = Vec::new();
                for rel in subjects.split(',') {
                    let source = read_subject(rel)?;
                    for (n, line) in code_lines(&source) {
                        for (needle, why) in BANNED {
                            if line.contains(needle) {
                                hits.push(format!(
                                    "{rel}:{n}  {needle}  ({why})\n      {}",
                                    line.trim()
                                ));
                            }
                        }
                    }
                }
                if !hits.is_empty() {
                    return Err(format!(
                        "the swallow is back on the hearth-root guard path, at {} site(s):\n  {}\n\n\
                         Every filesystem question on this path must go through \
                         `fs_probe::node_kind`, which answers `Result<NodeKind, io::Error>`. A \
                         `bool` cannot distinguish 'there is nothing here' from 'I could not \
                         look', and on this path the first answer clears a writer while the \
                         second must block one. This is the fourth round of this defect; it was \
                         closed at one syscall twice and came back at the next spelling both \
                         times.",
                        hits.len(),
                        hits.join("\n  ")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no non-private function in the probe returns a bare bool",
            &[(SUBJECTS_KEY, "string")],
            |ctx, _params| {
                let subjects = ctx.get::<String>(SUBJECTS_KEY).ok_or("no subjects")?;
                let mut hits: Vec<String> = Vec::new();
                for rel in subjects.split(',') {
                    let source = read_subject(rel)?;
                    // C-d.1 round 6, L-2. This used to require the line to
                    // begin `pub fn ` AND to contain `-> bool` ON THE SAME
                    // LINE, so the round-5 reviewer's ATK-9 —
                    // `pub(crate) fn looks_present(p: &Path) -> bool` added to
                    // this very module — passed, as did any two-line signature
                    // (`pub fn f(` ⏎ `) -> bool`). The check now recognises
                    // every visibility form Rust can spell and reads the
                    // signature across the lines it actually occupies.
                    //
                    // This is a repair of a check that did not do what it said,
                    // NOT the answer to H-1. It is still lexical. What actually
                    // closes ATK-9's shape is that the guard's DECISION no
                    // longer holds a `Path` (see `registry_projection`'s
                    // `HearthRootFacts`) — a helper handing back a `bool` there
                    // has nothing to be called on.
                    for (n, sig) in signature_starts(&source) {
                        if !returns_bool(&sig) {
                            continue;
                        }
                        hits.push(format!("{rel}:{n}  {}", sig.trim()));
                    }
                }
                if !hits.is_empty() {
                    return Err(format!(
                        "the probe module hands a bare `bool` back out, at:\n  {}\n\n\
                         It is exempt from the predicate scan ONLY because it is the module that \
                         converts those predicates into a `Result`. A reachable `-> bool` here \
                         relocates the swallow rather than removing it: a guard module would call \
                         it, the scan would not see it, and the answer reaching the writer would \
                         be the same `false`. `pub(crate)`, `pub(super)` and a signature split \
                         across lines all count — round 5's check matched only `pub fn` on one \
                         line, and `pub(crate) fn looks_present(..) -> bool` walked straight \
                         through it.",
                        hits.join("\n  ")
                    ));
                }
                Ok(())
            },
        ),
    ]
}
