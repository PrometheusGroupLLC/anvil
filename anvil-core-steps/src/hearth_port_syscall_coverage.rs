//! Step definitions for `anvil-core/features/hearth_port_syscall_coverage.feature`.
//!
//! # The criterion stops being anybody's list
//!
//! **C-d.1 round 8.** Seven rounds closed this class against an enumeration and
//! seven rounds were defeated by a noun nobody wrote down:
//!
//! * rounds 3–6 — **a module a list did not name** (`fs_query_adapter.rs`, the
//!   `QueryPort` the engine constructs eleven times, in no section of the record);
//! * round 7 — **an address form a matrix did not carry** (its own §40.4 finding,
//!   which is the best methodological moment on the track);
//! * round 7 again — **a filesystem NODE the fixture never created.** The
//!   249-cell matrix folds `domain::transition_log::read_event_files` on five of
//!   its own methods, on both of its own ports. That function `read_dir`s
//!   `<artifact_dir>/transitions/` and answers `Vec::new()` on ANY error. The
//!   fixture writes history as a legacy array inside `status.yaml` and never
//!   creates `transitions/` at all — so the swallowing arm ran in **all 249
//!   cells**, on a `NotFound` where emptying is the correct answer, and passed
//!   249 times. Gutting BOTH event readers to return empty unconditionally left
//!   the matrix **249/249 GREEN**, while the same mutation reds 21 scenarios
//!   elsewhere in the crate.
//!
//! An axis is a column. **A fixture is the universe the columns range over**, and
//! it was hand-written from one artifact shape. Adding a column cannot reach a
//! node that is not there to take a mode off.
//!
//! ## What this instrument asserts
//!
//! > **Every filesystem path the ports actually look at during the readable
//! > control must be a path some matrix row varies the mode of.**
//!
//! The left side is derived from the code's own **syscalls**, recorded by
//! interposing `stat`/`lstat`/`open`/`opendir`/`access` in a child process
//! (`anvil-test-support/fstrace/fstrace.c`). The right side is derived by
//! **parsing `hearth_port_reachability.feature`** — not from a list in this
//! module — so a token can only count as covered when a row in that table
//! actually takes a non-`0755` mode off it.
//!
//! Neither side is anyone's reading of the code. That is the whole point: the
//! only failure mode this track has ever had is *somebody did not think of the
//! right noun*, and a predicate that reads the noun out of a `stat` cannot have
//! that failure mode.
//!
//! ## Why LOOKUPS and not successful reads
//!
//! The recorder logs the lookup, not the outcome. A `stat` of a path that does
//! not exist is still a dependency on that path — and `transitions/` is exactly
//! that shape: read on every fold, absent in every fixture, swallowed silently.
//! Recording only successful opens would reproduce the hole this instrument
//! exists to close.
//!
//! ## Reuse
//!
//! The adapter sweep (`fs_hearth_reader` and the thirteen other `fs_*` adapters,
//! the `SnapshotPort` write half, the gRPC mapping) is its own track. It inherits
//! this instrument by pointing [`covered_shapes`] at its own feature file and
//! [`probe_subjects`] at its own port list: the recorder, the shape
//! normalisation and the assertion are subject-independent.

use anvil_test_support::ScratchDir;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::hearth_port_reachability as matrix;

const HEARTH_KEY: &str = "hpsc_hearth";
const HANDLE_KEY: &str = "hpsc_handle";
const TOUCHED_KEY: &str = "hpsc_touched";
const COVERED_KEY: &str = "hpsc_covered";
const MISSING_KEY: &str = "hpsc_missing";
const SUBJECTS_KEY: &str = "hpsc_subjects";
const VARIANTS_KEY: &str = "hpsc_variants";
const VAR_HANDLE_KEY: &str = "hpsc_var_handle";
const CENSUS_KEY: &str = "hpsc_census";

/// The env var the probe child reads to know it is the probe and not the suite.
pub const PROBE_ENV: &str = "ANVIL_FSTRACE_PROBE";
/// The env var carrying the root the probe builds its DEGRADED-ARM fixtures
/// under, one `vNNN/` per distinct `(subject, mode)` the matrix varies.
pub const VARIANTS_ENV: &str = "ANVIL_FSTRACE_VARIANTS";
/// The env var `fstrace.c` appends its records to.
pub const TRACE_ENV: &str = "FSTRACE_OUT";

/// The matrix whose coverage this instrument audits.
const MATRIX_FEATURE: &str = "anvil-core/features/hearth_port_reachability.feature";

fn workspace_root() -> PathBuf {
    PathBuf::from(anvil_test_support::TEST_SUPPORT_DIR)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

// ── the recorder ────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const RECORDER_FILE: &str = "libanvilfstrace.dylib";
#[cfg(not(target_os = "macos"))]
const RECORDER_FILE: &str = "libanvilfstrace.so";

/// The env var that makes the dynamic loader inject the recorder.
#[cfg(target_os = "macos")]
pub const INJECT_ENV: &str = "DYLD_INSERT_LIBRARIES";
#[cfg(not(target_os = "macos"))]
pub const INJECT_ENV: &str = "LD_PRELOAD";

/// Compile `fstrace.c` into `target/fstrace/`, reusing a build that is newer
/// than the source.
///
/// **This fails loud.** A missing compiler makes the instrument unable to
/// measure, and an instrument that cannot measure must not report coverage —
/// that is the shape of every green-by-absence check this track has already
/// been caught by.
fn build_recorder() -> Result<PathBuf, String> {
    let src = workspace_root().join("anvil-test-support/fstrace/fstrace.c");
    if !src.exists() {
        return Err(format!("the recorder source is missing: {}", src.display()));
    }
    let out_dir = workspace_root().join("target/fstrace");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir {}: {e}", out_dir.display()))?;
    let out = out_dir.join(RECORDER_FILE);

    let fresh = match (std::fs::metadata(&out), std::fs::metadata(&src)) {
        (Ok(o), Ok(s)) => match (o.modified(), s.modified()) {
            (Ok(om), Ok(sm)) => om >= sm,
            _ => false,
        },
        _ => false,
    };
    if fresh {
        return Ok(out);
    }

    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = std::process::Command::new(&compiler)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-O1")
        .arg("-o")
        .arg(&out)
        .arg(&src)
        .output()
        .map_err(|e| format!("could not run {compiler:?} to build the syscall recorder: {e}"))?;
    if !status.status.success() {
        return Err(format!(
            "building the syscall recorder failed ({compiler} -shared): {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    Ok(out)
}

// ── parsing the matrix feature ──────────────────────────────────────────────

/// One matrix cell, as parsed out of the feature file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatrixRow {
    pub port: String,
    pub method: String,
    pub address: String,
    /// The node this row takes a mode off, in the feature's own words.
    pub subject: String,
    pub mode: String,
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    if !t.starts_with('|') {
        return Vec::new();
    }
    t.trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

fn substitute(template: &str, header: &[String], row: &[String]) -> String {
    let mut out = template.to_string();
    for (h, v) in header.iter().zip(row.iter()) {
        out = out.replace(&format!("<{h}>"), v);
    }
    out
}

fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c == '"' {
            let mut s = String::new();
            for c2 in it.by_ref() {
                if c2 == '"' {
                    break;
                }
                s.push(c2);
            }
            out.push(s);
        }
    }
    out
}

/// Every cell of the port-reachability matrix, read out of the feature FILE.
///
/// Derived rather than declared on purpose: a subject can only count as covered
/// when a row on disk varies it, so "declare it covered" is not available
/// without writing the row.
pub fn matrix_rows() -> Result<Vec<MatrixRow>, String> {
    let path = workspace_root().join(MATRIX_FEATURE);
    let body = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;

    let mut rows = Vec::new();
    let mut given = String::new();
    let mut when = String::new();
    let mut header: Vec<String> = Vec::new();
    let mut in_examples = false;

    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("Scenario Outline:") {
            given.clear();
            when.clear();
            header.clear();
            in_examples = false;
        } else if t.starts_with("Given ") {
            given = t.to_string();
        } else if t.starts_with("When ") {
            when = t.to_string();
        } else if t.starts_with("Examples:") {
            in_examples = true;
            header.clear();
        } else if in_examples && t.starts_with('|') {
            let c = cells(line);
            if header.is_empty() {
                header = c;
                continue;
            }
            if c.len() != header.len() {
                return Err(format!(
                    "the matrix has an Examples row whose width does not match its header: {t}"
                ));
            }
            let g = substitute(&given, &header, &c);
            let w = substitute(&when, &header, &c);
            let idx = |name: &str| header.iter().position(|h| h == name);
            let get = |name: &str| idx(name).map(|i| c[i].clone());
            let gq = quoted(&g);
            let wq = quoted(&w);
            let subject = gq.first().cloned().ok_or_else(|| {
                format!("a matrix Given carries no quoted subject: {g}")
            })?;
            // A row that varies its subject by SHAPE rather than by mode (the
            // ELOOP outline, C-d.1 round 8 M-2) carries no mode column. It is
            // still a row that varies the node, which is all this instrument
            // asks, so it is recorded under a non-readable pseudo-mode.
            let mode = get("mode")
                .or_else(|| gq.get(1).cloned())
                .unwrap_or_else(|| "symlink loop".to_string());
            let port = wq
                .first()
                .cloned()
                .ok_or_else(|| format!("a matrix When carries no port: {w}"))?;
            let method = wq
                .get(1)
                .cloned()
                .ok_or_else(|| format!("a matrix When carries no method: {w}"))?;
            let address = wq
                .get(2)
                .cloned()
                .ok_or_else(|| format!("a matrix When carries no address: {w}"))?;
            rows.push(MatrixRow {
                port,
                method,
                address,
                subject,
                mode,
            });
        } else if in_examples && t.is_empty() {
            // blank line ends a table but not the outline
            header.clear();
        }
    }
    if rows.is_empty() {
        return Err(format!(
            "parsed ZERO rows out of {MATRIX_FEATURE}. An instrument that reads no rows would \
             report every path as uncovered OR every path as covered depending on which way the \
             comparison fell — either way it would not be measuring. Fix the parser."
        ));
    }
    Ok(rows)
}

/// The distinct `(port, method, address)` triples the matrix exercises.
pub fn probe_subjects() -> Result<Vec<(String, String, String)>, String> {
    let mut out: BTreeSet<(String, String, String)> = BTreeSet::new();
    for r in matrix_rows()? {
        out.insert((r.port, r.method, r.address));
    }
    Ok(out.into_iter().collect())
}

// ── shapes ──────────────────────────────────────────────────────────────────

/// Reduce a hearth-relative path to a SHAPE: the artifact ids and the
/// write-time-prefixed event filenames are generalised, everything else is
/// literal.
///
/// Generalisation is deliberately shallow. Every component that is not an
/// artifact id or an event filename stays exactly as the code spelled it, so a
/// node cannot be swept under a coarser bucket — which would rebuild the hole
/// by naming, one level up.
pub fn shape(rel: &str) -> String {
    rel.split('/')
        .map(|c| {
            if c == matrix::TRACK_ID || c == matrix::KNOWLEDGE_ID {
                "<id>".to_string()
            } else if c.ends_with(".yaml") && c.len() > 20 && c.contains('_') && c.starts_with("2") {
                "<event>.yaml".to_string()
            } else {
                c.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The modes that leave a node READABLE to its owner, and therefore cannot be
/// the mode a row varies it at.
///
/// `0500` is in here deliberately. On a directory it is `r-x`: listable AND
/// searchable, so it is a CONTROL — round 8's own tables use it as the row that
/// proves a refusal is a refusal and not a uniform failure. Counting it as
/// "varied" would let a node be declared covered by a mode at which nothing can
/// go wrong.
const READABLE_MODES: &[&str] = &["0755", "0644", "0500"];

/// The modes a DIRECTORY subject must be varied at.
///
/// **C-d.1 round 9, M-1.** These are three distinct POSIX failure sites and the
/// code discriminates between them:
///
/// * `0300` — `-wx`: searchable, not listable. `stat` through it succeeds,
///   `read_dir` of it fails.
/// * `0600` — `rw-`: listable, NOT searchable. `read_dir` succeeds and the
///   per-entry `stat` beneath it fails. **This is the mode round 7's H-2 failed
///   open at**, and the only one that can distinguish a swallow at the per-entry
///   read from a refusal at the listing above it.
/// * `0000` — nothing.
const DIR_MODES: &[&str] = &["0300", "0600", "0000"];

/// The modes a FILE subject must be varied at: write-only (present, statable,
/// unreadable — the mode `atomic_write` still succeeds at) and nothing.
const FILE_MODES: &[&str] = &["0200", "0000"];

/// The modes some matrix row varies each node at — parsed from the feature and
/// resolved through the matrix's OWN subject→path mapping.
///
/// # Why this is a MAP and not a set
///
/// **C-d.1 round 9, M-1.** Round 8's predicate was per-NODE: a node counted as
/// covered when ANY row varied it at ANY non-readable mode. The round-8 reviewer
/// deleted the eight `the transitions directory | 0600` rows — a plausible,
/// invisible future edit, since the node stays varied at `0300` and `0000` — and
/// **the instrument stayed GREEN.** `MUT-STRICT-ISFILE`, round 7's H-2 lexeme in
/// the FAIL-CLOSED adoption reader, then left the matrix 363/363 green, where
/// with the full table it reds exactly one cell.
///
/// So the worst defect on this track was pinned by a single row, and the
/// criterion built to make coverage underivable-from-a-list could not see that
/// row leave. The predicate is now `(node, mode)`: [`DIR_MODES`] for a directory
/// and [`FILE_MODES`] for a file, because those are the distinct POSIX failure
/// sites and the code answers differently at each.
pub fn covered_modes(hearth: &Path) -> Result<std::collections::BTreeMap<String, BTreeSet<String>>, String> {
    let mut out: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for r in matrix_rows()? {
        if READABLE_MODES.contains(&r.mode.as_str()) {
            continue;
        }
        let Some(p) = matrix::target(hearth, &r.subject)? else {
            continue;
        };
        let rel = p
            .strip_prefix(hearth)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let key = if rel.is_empty() {
            ".".to_string()
        } else {
            shape(&rel)
        };
        out.entry(key).or_default().insert(r.mode.clone());
    }
    Ok(out)
}

/// The shapes some matrix row varies at SOME non-readable mode.
///
/// Retained as the coarse view — it is what the failure message counts and what
/// the adapter-sweep track's own instrument can start from before it has enough
/// rows for the per-mode predicate. The criterion itself is [`covered_modes`].
pub fn covered_shapes(hearth: &Path) -> Result<BTreeSet<String>, String> {
    Ok(covered_modes(hearth)?.into_keys().collect())
}

// ── the probe child ─────────────────────────────────────────────────────────

/// Every DISTINCT `(subject, mode)` degradation the matrix applies, in the
/// feature's own order — the fixtures the probe must rebuild.
pub fn degraded_arms() -> Result<Vec<(String, String)>, String> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out = Vec::new();
    for r in matrix_rows()? {
        if READABLE_MODES.contains(&r.mode.as_str()) {
            continue;
        }
        if seen.insert((r.subject.clone(), r.mode.clone())) {
            out.push((r.subject, r.mode));
        }
    }
    Ok(out)
}

/// Run every matrix port method against the readable control AND against every
/// degraded fixture the matrix builds.
///
/// Called in a child process with the recorder injected. It asserts nothing: its
/// only job is to make the ports take every filesystem lookup they take, so the
/// recorder can see them.
///
/// # Why the degraded arms are here — C-d.1 round 9, M-2
///
/// Round 8's probe ran the control ONLY, and performed no chmod. So every path
/// the ports take **on an error branch** was outside the instrument's universe,
/// and the round-8 reviewer measured it: a swallowing `read_to_string` of a node
/// the fixture never creates, placed behind `if <a readable file>.is_err()`,
/// left the instrument **GREEN**. The read simply never happened.
///
/// That is not a generic limitation. **The degraded path is this class's home
/// address** — every fallback, every "if I could not read the primary, read the
/// secondary", every recovery branch is code that runs precisely when something
/// is already unreadable. An instrument whose predicate is *"a path the ports
/// look at when everything is fine"* rather than *"a path the ports depend on"*
/// excludes exactly the arms the defect lives in.
///
/// So the probe now rebuilds one fresh hearth per distinct `(subject, mode)` the
/// matrix carries, applies that row's degradation through the MATRIX'S OWN
/// [`matrix::degrade`] (not a second copy of what "0600" means), runs every port
/// method, and the tapes are unioned. The fixtures are the matrix's fixtures by
/// construction, so the instrument cannot measure an arm the table does not
/// build — and it can no longer miss one the table does.
///
/// Entered from `anvil-core/tests/brine_runner.rs` when [`PROBE_ENV`] is set.
pub fn run_probe(control: &Path, variants_root: &Path) -> Result<(), String> {
    let subjects = probe_subjects()?;
    let sweep = |hearth: &Path| -> Result<(), String> {
        for (port, method, address) in &subjects {
            // An individual method's answer is irrelevant here — the matrix
            // asserts answers. What matters is that the call HAPPENED, so its
            // syscalls are on the tape. An `Err` from `invoke` is a
            // matrix/probe disagreement and must be loud.
            matrix::invoke(hearth, port, method, address)?;
        }
        Ok(())
    };
    sweep(control)?;

    for (i, (subject, mode)) in degraded_arms()?.into_iter().enumerate() {
        let hearth = variants_root.join(format!("v{i:03}"));
        std::fs::create_dir_all(&hearth)
            .map_err(|e| format!("mkdir {}: {e}", hearth.display()))?;
        matrix::seed(&hearth)?;
        matrix::degrade(&hearth, &subject, &mode)?;
        sweep(&hearth)?;
    }
    Ok(())
}

/// One recorded filesystem lookup, reduced to what the criterion needs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Lookup {
    /// `true` for `opendir` and read-only `open` — the code consumed this
    /// node's CONTENTS. `false` for `stat`/`lstat`/`access` — the code only
    /// asked what is there.
    reads_contents: bool,
    /// Hearth-relative path, exactly as the code spelled it.
    rel: String,
}

/// Every path under `dir`, hearth-relative, as it stands BEFORE the probe runs.
///
/// Taken before, not after, because three of the matrix's methods rebuild a
/// projection — an "exists" set taken afterwards would count the probe's own
/// outputs as fixture.
fn existing(hearth: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if let Ok(rel) = p.strip_prefix(root) {
                out.insert(rel.display().to_string());
            }
            if p.is_dir() {
                walk(root, &p, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(hearth, hearth, &mut out);
    out
}

/// Ops the recorder MUST have seen for the tape to be a complete measurement.
///
/// # C-d.1 round 9, M-3 — the guard was calibrated for a DEAD recorder, not a
/// partly deaf one
///
/// The round-8 reviewer raised that `fstrace.c` interposes the symbols `stat`
/// and `opendir`, while `libc`'s Rust bindings carry
/// `#[cfg_attr(all(target_os = "macos", not(target_arch = "aarch64")),
/// link_name = "stat$INODE64")]` — so on `x86_64-apple-darwin` libstd would call
/// `_stat$INODE64` and the interposer would miss 80% of the tape while still
/// clearing `touched.len() < 5`.
///
/// **The mechanism is real; the conclusion for THIS recorder is falsified by
/// measurement, and the measurement is why the guard below is the right fix
/// rather than two more interpose entries.** The `$INODE64` suffix is not a Rust
/// fact, it is an SDK fact: `<sys/stat.h>` declares `stat` with
/// `__DARWIN_INODE64(stat)`, which expands to `__asm("_stat$INODE64")` whenever
/// `__DARWIN_64_BIT_INO_T && !__DARWIN_ONLY_64_BIT_INO_T`. `fstrace.c` includes
/// that same header, so the `(const void *)stat` it puts in `__DATA,__interpose`
/// gets the SAME alias the Rust binding does. Compiled both ways on the review's
/// own machine (MacOSX.sdk, `cc -arch`):
///
/// ```text
/// arm64    _access  _lstat           _open  _opendir           _stat
/// x86_64   _access  _lstat$INODE64   _open  _opendir$INODE64   _stat$INODE64
/// ```
///
/// And `build_recorder` compiles the source at test time on the host, so the
/// recorder's symbols always match the binary it is injected into.
///
/// The GENERAL risk the reviewer identified is nevertheless live and worth
/// closing: a partly deaf recorder — a different SDK, a statically-linked libstd
/// issuing raw syscalls, an interpose entry deleted in a refactor — would still
/// clear the count guard and report coverage over a fraction of the evidence. So
/// the tape is now required to carry EVERY op class the ports demonstrably use.
/// `opendir` is the one that matters most: it is how `transitions/` is read, and
/// it is the op the whole round-8 finding rests on.
const REQUIRED_OPS: &[&str] = &["stat", "open", "opendir"];

/// Spawn the probe with the recorder injected and return the lookups it made
/// inside the hearth, together with the per-op tape census.
fn record(hearth: &Path, variants: &Path) -> Result<(Vec<Lookup>, Vec<(String, usize)>), String> {
    let recorder = build_recorder()?;
    let exe = std::env::current_exe()
        .map_err(|e| format!("current_exe (needed to re-enter as the probe): {e}"))?;
    let trace = hearth
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join(format!("fstrace-{}.tsv", std::process::id()));
    let _ = std::fs::remove_file(&trace);

    let out = std::process::Command::new(&exe)
        .env(PROBE_ENV, hearth)
        .env(VARIANTS_ENV, variants)
        .env(TRACE_ENV, &trace)
        .env(INJECT_ENV, &recorder)
        .output()
        .map_err(|e| format!("spawning the probe ({}): {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "the probe child failed ({}): {}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let body = std::fs::read_to_string(&trace).map_err(|e| {
        format!(
            "the recorder wrote no trace at {} ({e}). The probe ran, so either the loader did not \
             inject {INJECT_ENV} or the recorder failed to open its output — and an EMPTY trace \
             would make this instrument pass by measuring nothing.",
            trace.display()
        )
    })?;

    // The hearth arrives canonicalised on macOS (`/tmp` -> `/private/tmp`), and
    // the recorder reports whatever string the caller passed, so BOTH forms are
    // stripped. The DEGRADED-ARM fixtures live under `variants/vNNN/`, so that
    // root is stripped too and its `vNNN/` component removed, which is what
    // unions every arm's tape into one shape space.
    let canonical = std::fs::canonicalize(hearth).unwrap_or_else(|_| hearth.to_path_buf());
    let var_canonical = std::fs::canonicalize(variants).unwrap_or_else(|_| variants.to_path_buf());
    let hearth_prefixes = [
        hearth.display().to_string(),
        canonical.display().to_string(),
    ];
    let variant_prefixes = [
        variants.display().to_string(),
        var_canonical.display().to_string(),
    ];
    let strip_variant = |rest: &str| -> Option<String> {
        // `/vNNN/tracks/...` -> `tracks/...`; `/vNNN` -> `.`
        let rest = rest.trim_start_matches('/');
        let (head, tail) = match rest.split_once('/') {
            Some((h, t)) => (h, t),
            None => (rest, ""),
        };
        if !head.starts_with('v') || head.len() != 4 {
            return None;
        }
        Some(if tail.is_empty() {
            ".".to_string()
        } else {
            tail.to_string()
        })
    };

    let mut out_lookups: BTreeSet<Lookup> = BTreeSet::new();
    let mut census: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut lines = 0usize;
    let mut dropped_inside = 0usize;
    for line in body.lines() {
        lines += 1;
        let Some((op, path)) = line.split_once('\t') else {
            continue;
        };
        *census.entry(op.to_string()).or_default() += 1;
        // A WRITE open is not a read. This class is "an unreadable input read as
        // an empty one"; a temp sibling the code itself just created is an
        // output, and requiring a row to take a mode off it would be noise.
        if op == "openw" {
            continue;
        }
        let reads_contents = op == "open" || op == "opendir";
        let mut rel: Option<String> = None;
        for prefix in &hearth_prefixes {
            if let Some(rest) = path.strip_prefix(prefix.as_str()) {
                let r = rest.trim_start_matches('/');
                rel = Some(if r.is_empty() { ".".to_string() } else { r.to_string() });
                break;
            }
        }
        if rel.is_none() {
            for prefix in &variant_prefixes {
                if let Some(rest) = path.strip_prefix(prefix.as_str()) {
                    // A path under the variants ROOT that is not under a `vNNN`
                    // is accounted for rather than dropped — see below.
                    rel = strip_variant(rest);
                    if rel.is_none() {
                        dropped_inside += 1;
                    }
                    break;
                }
            }
        }
        if let Some(rel) = rel {
            out_lookups.insert(Lookup { reads_contents, rel });
        }
    }
    let _ = std::fs::remove_file(&trace);
    if lines == 0 {
        return Err("the recorder produced an EMPTY trace; nothing was measured".to_string());
    }
    if out_lookups.is_empty() {
        return Err(format!(
            "the recorder logged {lines} filesystem lookups and NONE of them were inside the \
             hearth at {}. The prefix filter is wrong, and a wrong filter makes this instrument \
             green by measuring nothing.",
            hearth.display()
        ));
    }
    // C-d.1 round 9: the round-8 reviewer noted a silent-drop channel — a
    // recorded path under the fixture root that the prefix logic discards with
    // no accounting. It is now counted and fails loud, because a filter that
    // quietly drops evidence is the same defect as a reader that quietly
    // empties it.
    if dropped_inside > 0 {
        return Err(format!(
            "{dropped_inside} recorded lookup(s) fell inside the variants root at {} but under no \
             `vNNN` fixture, so they were about to be dropped with no accounting. A filter that \
             silently discards evidence is this track's own defect class, in the instrument.",
            variants.display()
        ));
    }
    // C-d.1 round 9, M-3: EVERY op class must be on the tape. A recorder that
    // lost one class would still clear the node-count guard and report coverage
    // over a fraction of the evidence.
    let missing_ops: Vec<&str> = REQUIRED_OPS
        .iter()
        .copied()
        .filter(|op| census.get(*op).copied().unwrap_or(0) == 0)
        .collect();
    if !missing_ops.is_empty() {
        return Err(format!(
            "the recorder saw NO {} on a tape of {lines} lookups ({}). Interposition is \
             INCOMPLETE on this platform, and a partly deaf recorder reports coverage over a \
             fraction of the evidence while clearing every count-based guard. `opendir` is how \
             `transitions/` is read at all — the node the whole round-8 finding rests on. Fix the \
             recorder's symbol binding for this target before trusting any figure here.",
            missing_ops.join(" and no "),
            census
                .iter()
                .map(|(op, n)| format!("{op}={n}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    Ok((
        out_lookups.into_iter().collect(),
        census.into_iter().collect(),
    ))
}

/// The node whose MODE governs a lookup, and therefore the node a row has to
/// vary for the lookup to be covered.
///
/// Two rules, and the split between them is the instrument's whole precision:
///
/// * **A read** (`opendir`, read-only `open`) is governed by the path's OWN
///   mode, so the path itself must be varied. **A read of a path the fixture
///   never creates is uncovered by construction** and is reported as such —
///   that is the `transitions/` finding stated as a rule. No row can vary a node
///   that is not there, so the swallowing arm runs on `NotFound`, where
///   emptying is the CORRECT answer, and passes every time. Green cells over a
///   swallow that happens to be right for the one input they supply is the exact
///   definition of an unfailable assertion.
/// * **A probe** (`stat`, `lstat`, `access`) can never fail because of the
///   node's OWN mode. `stat` needs search permission on the directories above
///   it, so a probe is governed by its **nearest existing ancestor directory** —
///   which is POSIX, not a convenience. This is why the six-directory lookup
///   scan does not demand forty rows: `proposals/<id>` is probed, `proposals/`
///   is not in the fixture at all, and the only mode that can change that
///   probe's answer is the hearth root's — which the matrix does vary.
///
/// The asymmetry between the two is the instrument's edge. A read demands the
/// node itself and therefore demands the node EXIST; a probe demands only what
/// POSIX says decides it. Collapsing them either way would break it: treat every
/// lookup as a read and the scan produces forty impossible rows; treat every
/// lookup as a probe and `transitions/` is "covered" by `tracks/<id>` and
/// vanishes again.
fn governing_node(l: &Lookup, on_disk: &BTreeSet<String>) -> String {
    if l.reads_contents {
        return l.rel.clone();
    }
    let mut cur = l.rel.as_str();
    while let Some((parent, _)) = cur.rsplit_once('/') {
        if on_disk.contains(parent) {
            return parent.to_string();
        }
        cur = parent;
    }
    ".".to_string()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the fixtures the port-reachability matrix builds — the readable control and every degraded arm",
            &[],
            &[
                (HEARTH_KEY, "string"),
                (HANDLE_KEY, "handle"),
                (VARIANTS_KEY, "string"),
                (VAR_HANDLE_KEY, "handle"),
            ],
            |_ctx, _params| {
                let t = ScratchDir::new()?;
                let hearth = t.path().to_path_buf();
                matrix::seed(&hearth)?;
                // The degraded arms are built by the PROBE CHILD (it is the
                // process the recorder is injected into), under this root. Held
                // here so its ScratchDir drop restores modes and cleans up even
                // when a fixture ends at 0000.
                let v = ScratchDir::new()?;
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth.display().to_string());
                out.set(HANDLE_KEY, Arc::new(t));
                out.set(VARIANTS_KEY, v.path().display().to_string());
                out.set(VAR_HANDLE_KEY, Arc::new(v));
                Ok(out)
            },
        ),
        step_def(
            "every port method the matrix exercises is invoked under a syscall recorder",
            &[
                (HEARTH_KEY, "string"),
                (HANDLE_KEY, "handle"),
                (VARIANTS_KEY, "string"),
                (VAR_HANDLE_KEY, "handle"),
            ],
            &[
                (HEARTH_KEY, "string"),
                (HANDLE_KEY, "handle"),
                (VARIANTS_KEY, "string"),
                (VAR_HANDLE_KEY, "handle"),
                (TOUCHED_KEY, "string"),
                (COVERED_KEY, "string"),
                (MISSING_KEY, "string"),
                (SUBJECTS_KEY, "string"),
                (CENSUS_KEY, "string"),
            ],
            |mut ctx, _params| {
                let hearth = ctx.get::<String>(HEARTH_KEY).ok_or("no hearth")?.clone();
                let variants = ctx.get::<String>(VARIANTS_KEY).ok_or("no variants")?.clone();
                let h = Path::new(&hearth);
                let on_disk = existing(h);
                let (lookups, census) = record(h, Path::new(&variants))?;
                let covered_by_mode = covered_modes(h)?;
                let covered: BTreeSet<String> = covered_by_mode.keys().cloned().collect();
                let subjects = probe_subjects()?.len();

                // The governing node of every lookup, as a SHAPE, tagged with
                // why it is uncovered when it is. Reduced here rather than in
                // the check so the check holds a decided list and cannot
                // re-decide it more leniently.
                let mut touched: BTreeSet<String> = BTreeSet::new();
                let mut missing: BTreeSet<String> = BTreeSet::new();
                for l in &lookups {
                    let node = governing_node(l, &on_disk);
                    let s = shape(&node);
                    touched.insert(s.clone());
                    if !covered.contains(&s) {
                        if !on_disk.contains(&node) && node != "." {
                            missing.insert(format!(
                                "{s}  [READ of a node the fixture NEVER CREATES — no row can vary it]"
                            ));
                        } else {
                            missing.insert(format!(
                                "{s}  [on disk, {} — and no row takes a mode off it]",
                                if l.reads_contents { "read" } else { "probed through" }
                            ));
                        }
                        continue;
                    }
                    // ── C-d.1 round 9, M-1: covered AT WHICH MODES ──────────
                    //
                    // A node varied at some mode is not a node varied at the
                    // mode that matters. The required set is decided by what the
                    // node IS on disk, not by what any row says about it, so a
                    // row cannot make its own subject easier by choosing a mode.
                    let is_dir = h.join(&node).is_dir();
                    let required: &[&str] = if is_dir { DIR_MODES } else { FILE_MODES };
                    let varied = covered_by_mode.get(&s);
                    let absent: Vec<&str> = required
                        .iter()
                        .copied()
                        .filter(|m| !varied.map(|v| v.contains(*m)).unwrap_or(false))
                        .collect();
                    if !absent.is_empty() {
                        let have: Vec<&str> = varied
                            .map(|v| v.iter().map(String::as_str).collect())
                            .unwrap_or_default();
                        missing.insert(format!(
                            "{s}  [a {} varied only at {} — NO row takes mode(s) {} off it]",
                            if is_dir { "DIRECTORY" } else { "FILE" },
                            if have.is_empty() {
                                "nothing".to_string()
                            } else {
                                have.join("/")
                            },
                            absent.join("/")
                        ));
                    }
                }

                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                out.set(VARIANTS_KEY, variants);
                if let Some(handle) = ctx.take::<Arc<ScratchDir>>(HANDLE_KEY) {
                    out.set(HANDLE_KEY, handle);
                }
                if let Some(handle) = ctx.take::<Arc<ScratchDir>>(VAR_HANDLE_KEY) {
                    out.set(VAR_HANDLE_KEY, handle);
                }
                out.set(
                    TOUCHED_KEY,
                    touched.into_iter().collect::<Vec<_>>().join("\n"),
                );
                out.set(
                    COVERED_KEY,
                    covered.into_iter().collect::<Vec<_>>().join("\n"),
                );
                out.set(MISSING_KEY, missing.into_iter().collect::<Vec<_>>().join("\n"));
                out.set(SUBJECTS_KEY, subjects.to_string());
                out.set(
                    CENSUS_KEY,
                    census
                        .iter()
                        .map(|(op, n)| format!("{op}={n}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                Ok(out)
            },
        ),
        check_def(
            "every path they looked at is a path the matrix varies",
            &[
                (TOUCHED_KEY, "string"),
                (COVERED_KEY, "string"),
                (MISSING_KEY, "string"),
                (SUBJECTS_KEY, "string"),
                (CENSUS_KEY, "string"),
            ],
            |ctx, _params| {
                let touched: Vec<&str> = ctx
                    .get::<String>(TOUCHED_KEY)
                    .ok_or("no touched set")?
                    .lines()
                    .filter(|l| !l.is_empty())
                    .collect();
                let covered: Vec<&str> = ctx
                    .get::<String>(COVERED_KEY)
                    .ok_or("no covered set")?
                    .lines()
                    .filter(|l| !l.is_empty())
                    .collect();
                let missing: Vec<&str> = ctx
                    .get::<String>(MISSING_KEY)
                    .ok_or("no missing set")?
                    .lines()
                    .filter(|l| !l.is_empty())
                    .collect();
                let subjects = ctx.get::<String>(SUBJECTS_KEY).ok_or("no subject count")?;
                let census = ctx.get::<String>(CENSUS_KEY).ok_or("no tape census")?;

                // Anti-vacuity, both directions. A recorder that saw nothing and
                // a matrix that varies nothing would both satisfy a naive subset
                // check, and this instrument exists because a check that cannot
                // fail is how this class survived seven rounds. The per-op
                // completeness of the tape (M-3) is enforced in `record`, where
                // the census is taken; it is carried here so a reader of a
                // passing run can see WHAT was measured and not only that
                // something was.
                if census.is_empty() {
                    return Err(
                        "the tape census is empty, so nothing is known about WHICH filesystem \
                         operations were interposed. A count of nodes cannot distinguish a \
                         complete recorder from a partly deaf one."
                            .to_string(),
                    );
                }
                if touched.len() < 5 {
                    return Err(format!(
                        "the recorder saw only {} governing node(s) inside the hearth. The ports \
                         open a status.yaml, a registry, a projection, an op log, a hook and a \
                         context file at minimum — this is a broken measurement, not coverage.",
                        touched.len()
                    ));
                }
                if covered.is_empty() {
                    return Err(
                        "the matrix varies NO node at a non-readable mode. Either the feature \
                         parse broke or the table stopped being a criterion."
                            .to_string(),
                    );
                }

                if missing.is_empty() {
                    return Ok(());
                }
                Err(format!(
                    "{} filesystem node(s) govern a lookup the ports make, and the matrix does \
                     NOT take the governing modes off them:\n  {}\n\nThe {subjects} port-method \
                     subjects made lookups governed by {} node shapes; {} of those are varied at \
                     SOME non-readable mode by a row of `hearth_port_reachability.feature`, and \
                     the criterion is per-(node, mode): a directory needs 0300, 0600 and 0000, a \
                     file needs 0200 and 0000.\n\nEach line above is a node whose unreadable \
                     behaviour nothing on this track asserts. A node the fixture never creates is \
                     how `transitions/` stayed invisible through a 249-cell matrix that folds it \
                     on five of its own methods; a node varied at the WRONG mode is how deleting \
                     eight rows left round 7's H-2 pinned by nothing while this instrument stayed \
                     green (C-d.1 round 9, M-1). Seed the node and add the missing rows; do NOT \
                     widen this check.\n\nTape census: {census}",
                    missing.len(),
                    missing.join("\n  "),
                    touched.len(),
                    covered.len(),
                ))
            },
        ),
    ]
}
