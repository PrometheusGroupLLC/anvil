//! Fallible filesystem inspection for the hearth-root guard path.
//!
//! # Why this module exists
//!
//! `Path::is_dir()`, `Path::is_file()` and `Path::exists()` return `bool`. They
//! call `stat` and map **every** error — `EACCES`, `ESTALE`, `EIO`, `EMFILE` —
//! onto `false`. On the hearth-root guard path `false` reads as *"there is
//! nothing here"*, and *"there is nothing here"* is the one answer on which
//! every guard concludes that nothing would be lost and **clears a writer**.
//!
//! That defect was filed three times and closed twice at the instance:
//!
//! * round 3 found `list_dirs` swallowing `read_dir`'s `Err` into `Vec::new()`;
//! * round 4 fixed **that syscall** and published a three-row contract whose
//!   third row — *"present, one ENTRY unreadable → `Err`"* — the code did not
//!   implement, because three per-entry `stat` swallows sat behind the fixed
//!   `read_dir`: `entry.path().is_dir()`, `…/machine.yaml.is_file()` and
//!   `artifact_dir.is_dir()`;
//! * round 5 (this module) states the **property** instead of the syscall:
//!   **no unreadable input may be read as an empty one**, and makes every
//!   filesystem read on the path answer with a type that cannot express the
//!   swallow.
//!
//! # The mechanism, and its honest limit
//!
//! [`node_kind`] returns `io::Result<NodeKind>`. `NodeKind` has no `bool`
//! conversion, so a caller cannot re-collapse the four answers into two without
//! writing the collapse down; and `Err` is a separate arm, so "I could not look"
//! can only become "there is nothing there" by an explicit, visible decision.
//!
//! **C-d.1 round 7, M-2 — this header used to say two things that are false,
//! and it said them after the hearth record had already corrected them.**
//! Corrected in place, because a document is read by one track and a module
//! header is read by whoever touches the module next, forever.
//!
//! * It said *"the enforcement is mechanical … reds a test."* **It is not
//!   enforcement.** `hearth_fail_loud_lint.feature` is `line.contains(needle)`
//!   over a hand-named subject list. Eleven bypasses are measured and tabulated
//!   in `implementation-c.md` §38.1 + §40 — UFCS, a type alias, a line break, a
//!   macro, the un-banned neighbours of the banned combinators, a helper in a
//!   third module. Every recurrence of this defect on this track has been a
//!   DIFFERENT LEXEME, and a substring scan is the one mechanism that cannot see
//!   a new lexeme. **It is a tripwire for the spellings that came back before.**
//! * It said `clippy::disallowed_methods` *"is workspace-wide"* and therefore
//!   cannot be scoped. **The lint LIST is workspace-wide; the lint LEVEL is
//!   per-item**, so `#![deny(clippy::disallowed_methods)]` at the crate root with
//!   `#[allow(..)]` on the modules that legitimately use the predicates is
//!   exactly the granularity that sentence said Rust cannot express. It resolves
//!   paths, so it is semantic where the scan is textual, and it would catch 3 of
//!   the measured bypasses and none of the rest. It is not wired here — there is
//!   no clippy invocation in any gate in this tree, so wiring it is new CI work
//!   with its own blast radius, and a lint nothing runs is green by absence.
//!
//! **What actually holds the line is structural and behavioural, not lexical:**
//! the deciding code holds no path (`decide_hearth_roots`, `loader`), the I/O is
//! hoisted to an edge that answers with a type, and mode-parameterised outlines
//! with readable CONTROL rows red when a converted site regresses.
//!
//! This module is the ONE place allowed to call those predicates, because it is
//! the place that converts them into a `Result`. The lint holds it to that: no
//! public function here may return `bool`.

use std::io;
use std::path::Path;

/// What a path IS — an answer that could not have been produced by an error.
///
/// Four arms, not two, because the questions the guard path actually asks are
/// "is this absent?", "is this the directory I need?" and "is this a file where
/// a directory belongs?" — and `bool` answers all three with the same token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// The path does not exist. This is a real answer, not a failure: a hearth
    /// with no legacy root genuinely holds nothing under it, and the callers'
    /// "nothing would be lost" conclusion is sound.
    Absent,
    Directory,
    File,
    /// A socket, fifo, device — present, and not something this path can read.
    Other,
}

/// `stat` with symlinks FOLLOWED, `NotFound` as an answer, and every other
/// error PROPAGATED.
///
/// Symlinks are followed for parity with `Path::is_dir()`, which the hearth
/// guard depends on: `{hearth}/playbooks` as a symlink to `{hearth}/playbooks`
/// is the compatibility shim an operator leaves behind, and it must read as the
/// directory it points at.
///
/// `NotFound` and `NotADirectory` are the ONLY errors mapped to an answer, and
/// both for the same reason: they are the errors whose meaning genuinely is
/// "there is nothing at this path". Every other error means "I could not look",
/// which is a different fact with a different consequence. A dangling symlink
/// resolves to `NotFound` and therefore to `Absent`, which is the same answer
/// `Path::is_dir()` gave and the correct one.
///
/// **`NotADirectory` (`ENOTDIR`), added in C-d.1 round 7 with its red measured.**
/// `stat` answers `ENOTDIR` when a PREFIX component of the path is a regular
/// file — `projections/execution.md/status.yaml` is the live example, produced
/// by a hearth scan probing every subdirectory for `<subdir>/<id>/status.yaml`.
/// A path whose parent is a file cannot name an existing node, so `Absent` is
/// the true answer and it is the answer `Path::exists()` gave. This is NOT a
/// widening of the swallow: `ENOTDIR` is a fact about the SHAPE of the path, and
/// the access errors this module exists to stop collapsing — `EACCES`, `EIO`,
/// `ESTALE`, `EMFILE`, `ELOOP` — are untouched and still refuse. Two engine
/// scenarios red without it (`Origin-turn hit still returns playbook fields`,
/// `routed begin emits one selection half and no route half`), both with
/// `gRPC INTERNAL … Not a directory (os error 20)` over a hearth that is
/// perfectly readable — which is the mirror-image defect: a false refusal.
pub fn node_kind(path: &Path) -> io::Result<NodeKind> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let file_type = meta.file_type();
            if file_type.is_dir() {
                Ok(NodeKind::Directory)
            } else if file_type.is_file() {
                Ok(NodeKind::File)
            } else {
                Ok(NodeKind::Other)
            }
        }
        Err(e)
            if e.kind() == io::ErrorKind::NotFound
                || e.kind() == io::ErrorKind::NotADirectory =>
        {
            Ok(NodeKind::Absent)
        }
        Err(e) => Err(e),
    }
}

/// One directory entry, already inspected.
///
/// The point of the type is that it arrives with its [`NodeKind`] already
/// resolved by the edge that listed it. Code holding a `DirListing` has the
/// facts and no longer has a reason to reach for the filesystem, so there is
/// nothing left at the decision site for a `bool` predicate to be written on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryFacts {
    /// The entry's own file name (no directory components).
    pub name: String,
    /// The full path, `dir.join(name)`.
    pub path: std::path::PathBuf,
    /// What the entry IS, with symlinks followed — see [`node_kind`].
    pub kind: NodeKind,
}

/// A directory listing in which every entry has already been inspected.
pub type DirListing = Vec<DirEntryFacts>;

/// List `dir`, inspecting every entry, propagating EVERY error.
///
/// **C-d.1 round 7, H-1.** This is the edge §39.8(1) named as the way to close
/// the scan half of the class: *"a typed directory listing, so the survey holds
/// a `Vec<Entry>` rather than a root."* The four historical spellings of this
/// defect are all scan spellings and all of them are removed by construction:
///
/// ```text
/// let Ok(entries) = read_dir(d) else { return Vec::new() };  // round 3 — Err emptied
/// std::fs::read_dir(d).ok()?                                 // the same, as an Option
/// for entry in entries.flatten()                             // the per-ENTRY error, dropped
/// if !entry.path().is_dir() { continue; }                    // round 4 — the per-entry stat
/// ```
///
/// `entries.flatten()` is the quietest of the four and the easiest to miss in
/// review: it reads as a convenience, and it silently discards a `read_dir`
/// iteration error — an entry the kernel could not hand back at all — so a
/// listing comes out SHORT and complete-looking.
///
/// **`NotFound` is propagated here, not answered.** A caller asking "what is in
/// this directory" is usually asking about a directory it believes exists, and
/// the two callers that treat absence as an answer say so at their own site
/// (see [`list_hook_files`]). Answering `Ok(vec![])` for an absent directory
/// here would rebuild the exact collapse this module exists to remove — an
/// absent listing and an unreadable one arriving as the same value.
pub fn list_dir(dir: &Path) -> io::Result<DirListing> {
    let mut out: DirListing = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = node_kind(&path)?;
        out.push(DirEntryFacts {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            kind,
        });
    }
    Ok(out)
}

/// The regular-file `*.md` hook filenames directly under `hooks_dir`, sorted.
///
/// **C-d.1 round 6, M-1.** This function used to live in
/// `domain::playbook::loader` — a module whose own header says *"No I/O; the
/// caller supplies the file contents and directory listing"* — and it carried
/// BOTH prior spellings of the swallow verbatim:
///
/// ```text
/// let Ok(dir_iter) = std::fs::read_dir(hooks_dir) else { return Vec::new(); };  // round 3
/// if !path.is_file() { return None; }                                          // round 4
/// ```
///
/// It is called from the SCANNED `hearth_registry::hook_file_names`, one frame
/// below it and outside the lint's two-file subject list, so an unreadable
/// `hooks/` answered *"this artifact has no hook files"*. The loader then
/// rejected the machine for referencing a hook that **is on disk**
/// (`workflow_unknown_hook_reference`), dropped the definition, and
/// `registration_blocked()` answered `None` — a false diagnostic that sends an
/// operator to fix a `machine.yaml` that is correct.
///
/// The fix is structural, not another entry on a ban list: **the I/O moved to
/// the edge and the pure module became pure.** `loader` now performs no
/// filesystem access at all, which is what its header always claimed, so there
/// is nothing there for a predicate to be written on.
///
/// * `hooks_dir` absent -> `Ok(vec![])`. Most artifacts have no `hooks/`, and a
///   directory that is not there genuinely holds no hook files.
/// * `hooks_dir` present and unreadable -> `Err`. Propagated, never emptied.
/// * an individual entry uninspectable -> `Err`. A partial listing is a
///   complete-looking answer to an incomplete question, and here it produces a
///   confident, wrong `unknown hook` diagnostic.
///
/// Filtering to REGULAR FILES is load-bearing and preserved exactly: a
/// subdirectory or a broken symlink named `*.md` must not satisfy a hook
/// reference. [`node_kind`] follows symlinks and answers `Absent` for a dangling
/// one, which is the same answer `Path::is_file()` gave and the correct one.
pub fn list_hook_files(hooks_dir: &Path) -> io::Result<Vec<String>> {
    let entries = match std::fs::read_dir(hooks_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if node_kind(&path)? != NodeKind::File {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if name.ends_with(".md") && !name.contains('/') && !name.contains('\\') {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}
