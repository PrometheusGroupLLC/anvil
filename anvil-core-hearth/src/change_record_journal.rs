//! The three-phase journal that makes (filesystem effects, commit) one atomic
//! unit across process death, and the recovery pass that finishes it.
//!
//! The shape is the backlog port's, with a commit in place of the effect set:
//! `prepared -> applying -> committed`, each phase written through
//! `atomic_write`, with recovery discarding a `prepared` journal, rolling an
//! `applying` one forward, verifying and clearing a `committed` one, and
//! REFUSING anything it cannot reconcile.
//!
//! ## Where the journal lives, and why the GUARDED hearth owns it
//!
//! `<guarded>/.git/anvil/journal/<operation_id>/<leg>/{phase,manifest.yaml,index}`.
//!
//! Inside `.git/`, so it is outside the worktree and the exhaustive path
//! declaration never sees it. Under the GUARDED hearth even for a leg
//! targeting another repository, because an `owner_home` repository may be
//! written only as somebody else's second leg and may never itself be the
//! hearth of an RPC — a recovery keyed on "before this hearth serves a write"
//! would leave its journal unrolled forever.
//!
//! ## Exactly-once
//!
//! The manifest carries an explicit operation id and every field the commit
//! message is rendered from, INCLUDING `at`. So a rolled-forward commit is
//! byte-identical to the one an uninterrupted run would have written, and
//! "exactly one commit for this transaction" is checkable by reading the
//! lineage rather than by trusting a flag.
//!
//! ## The ref update is a compare-and-swap, and losing it is a retry
//!
//! The in-process hearth mutex does not span processes, and a leg writing a
//! second repository holds no guard at all, so `update-ref <ref> <new> <old>`
//! against the observed tip is the WHOLE of the serialization. A writer that
//! loses re-reads the tip, asks the lineage whether its operation id is
//! already there (the same key recovery uses), and otherwise rebuilds on the
//! new tip and commits again. It never forces, and it never gives up
//! silently: the retry is bounded and then refuses.
//!
//! ## Recovery never rewinds disk
//!
//! The manifest stores each path's CONTENT HASH and never its content, so a
//! roll-forward has to read the surviving bytes off disk; it cannot invent
//! them. A path whose live bytes match neither the journal's expectation nor
//! nothing is a loud conflict and is never overwritten.
//!
//! Out of scope and stated so nobody mistakes crash-atomic for durable: power
//! loss. There is no `fsync` in this codebase, so `atomic_write`'s rename may
//! be ordered before the data reaches disk, and that is true of the phase file
//! too.

use crate::atomic_write::atomic_write;
use crate::change_record_commit::{
    anvil_dir, build_tree, commit_for_operation, commit_tree, git_err, io_err, read_ref,
    stage_entries, tree_of, update_ref_cas, ChangeRecordError,
};
use crate::git_plumbing::CHANGE_RECORD_REF;
use anvil_core::domain::change_record::message::{render_commit_message, CommitMetadata};
use anvil_core::domain::content_hash::content_hash;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The injected crash point's environment key.
pub const CRASH_AFTER_ENV: &str = "ANVIL_TEST_CHANGE_RECORD_CRASH_AFTER";

/// The rendezvous barrier's environment key. Its value is the barrier file's
/// path; see [`hold_at_barrier`].
pub const CAS_BARRIER_ENV: &str = "ANVIL_TEST_CHANGE_RECORD_CAS_BARRIER";

/// How many times a lost compare-and-swap is retried against the new tip
/// before the leg refuses. Bounded on purpose: an unbounded retry against a
/// ref somebody else is rewriting in a tight loop is a livelock wearing
/// robustness's clothes, and a refusal is counted while a hang is not.
const MAX_CAS_ATTEMPTS: usize = 8;

/// How long a held writer waits for its release before calling the barrier a
/// defect. Only ever reached in a test process; a hang here would be a
/// scenario that never fails rather than one that passes.
const BARRIER_RELEASE_DEADLINE: Duration = Duration::from_secs(60);

/// The closed token grammar of the injected crash point. A token outside it is
/// REFUSED, so a typo cannot degrade into "no crash point" — a crash scenario
/// that silently never crashes is the decorative kind.
pub const CRASH_TOKENS: &[&str] = &[
    "journalled",
    "tree_written",
    "before_ref_update",
    "after_ref_update",
    "before_row",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Prepared,
    Applying,
    Committed,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Prepared => "prepared",
            Phase::Applying => "applying",
            Phase::Committed => "committed",
        }
    }

    fn parse(raw: &str) -> Option<Phase> {
        match raw.trim() {
            "prepared" => Some(Phase::Prepared),
            "applying" => Some(Phase::Applying),
            "committed" => Some(Phase::Committed),
            _ => None,
        }
    }
}

/// One leg's journalled intent: which repository, which parent it chose, the
/// metadata the message renders from, and every path with the hash of the
/// bytes the transaction wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegManifest {
    pub repository: PathBuf,
    pub parent: Option<String>,
    pub meta: CommitMetadata,
    pub paths: Vec<(String, String)>,
}

/// What one recovery pass did. Reported rather than logged: a swallowed
/// recovery outcome is a recovery nobody can audit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recovery {
    pub rolled_forward: Vec<String>,
    pub verified: Vec<String>,
    pub discarded: Vec<String>,
    pub conflicts: Vec<String>,
}

fn journal_root(guarded: &Path) -> PathBuf {
    anvil_dir(guarded).join("journal")
}

// ── the test-only hooks: the crash point and the rendezvous barrier ─────────

/// Which crash token is armed for THIS hearth, gated by [`test_hook_value`]'s
/// three conditions and validated against the closed token set — so a typo
/// cannot degrade into "no crash point".
fn crash_after(hearth: &Path) -> Result<Option<String>, ChangeRecordError> {
    let Some(raw) = test_hook_value(hearth, CRASH_AFTER_ENV)? else {
        return Ok(None);
    };
    if !CRASH_TOKENS.contains(&raw.as_str()) {
        return Err(git_err(format!(
            "'{}' is not a change-record crash point; the closed set is {}",
            raw,
            CRASH_TOKENS.join(", ")
        )));
    }
    Ok(Some(raw))
}

/// The three gates, in one place, for every test-only hook this module has.
/// Together they are what make both hooks unreachable in a shipped engine:
///
/// 1. `cfg!(debug_assertions)` — the kit ships built `--release`, where this
///    is false and no hook variable is even read.
/// 2. `ANVIL_TEST_MODE=1`, set explicitly. A stray hook variable in an
///    environment does nothing on its own.
/// 3. A hearth under the platform temporary directory. A hearth anywhere else
///    is an ERROR rather than a silent un-arming: an armed process pointed at
///    a real hearth must refuse loudly instead of running on, looking
///    un-armed and proving nothing.
///
/// An empty value is unset, not armed. Two hooks share this function because
/// the alternative is two copies of a three-condition gate, and a gate that
/// drifts between copies is a gate one of its callers no longer has.
fn test_hook_value(hearth: &Path, key: &str) -> Result<Option<String>, ChangeRecordError> {
    if !cfg!(debug_assertions) {
        return Ok(None);
    }
    if std::env::var("ANVIL_TEST_MODE").ok().as_deref() != Some("1") {
        return Ok(None);
    }
    let raw = match std::env::var(key) {
        Ok(value) if !value.is_empty() => value,
        _ => return Ok(None),
    };
    let canonical = std::fs::canonicalize(hearth).unwrap_or_else(|_| hearth.to_path_buf());
    let tmp = std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    if !canonical.starts_with(&tmp) {
        return Err(git_err(format!(
            "{} is only honored for a temporary hearth",
            key
        )));
    }
    Ok(Some(raw))
}

/// The barrier file this leg must rendezvous on, or `None` when unarmed.
fn cas_barrier(hearth: &Path) -> Result<Option<PathBuf>, ChangeRecordError> {
    Ok(test_hook_value(hearth, CAS_BARRIER_ENV)?.map(PathBuf::from))
}

/// The marker a held writer drops so the other side of the race can know it is
/// held without polling for a timing window. Exported so a test writes the
/// same path this module reads, rather than a second copy of the convention.
pub fn cas_barrier_waiting_marker(barrier: &Path) -> PathBuf {
    let mut name = barrier.as_os_str().to_os_string();
    name.push(".waiting");
    PathBuf::from(name)
}

/// Hold the FIRST writer to reach the ref update, between reading the tip and
/// moving it, until the barrier file appears.
///
/// This is what makes a lost compare-and-swap CERTAIN rather than likely: the
/// held writer's observed-old is guaranteed stale by the time it attempts its
/// update, so the retry path is exercised without a `sleep` anywhere ordering
/// the race.
///
/// **One-shot, and that is the load-bearing property.** The hold is claimed by
/// creating the waiting marker with `create_new`, so exactly one writer can
/// ever be held: every later arrival — the other side of the race, and this
/// same writer's own retry — finds the marker present and passes straight
/// through. A barrier that held both writers would deadlock them against each
/// other and turn a race scenario into a timeout.
fn hold_at_barrier(barrier: &Path) -> Result<(), ChangeRecordError> {
    if let Some(parent) = barrier.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| io_err(format!("create {}: {}", parent.display(), e)))?;
    }
    let waiting = cas_barrier_waiting_marker(barrier);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&waiting)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(e) => return Err(io_err(format!("claim {}: {}", waiting.display(), e))),
    }
    let start = Instant::now();
    while !barrier.exists() {
        if start.elapsed() > BARRIER_RELEASE_DEADLINE {
            return Err(git_err(format!(
                "the compare-and-swap barrier at {} was never released",
                barrier.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn maybe_crash(armed: &Option<String>, token: &str) -> Result<(), ChangeRecordError> {
    match armed {
        Some(value) if value == token => Err(ChangeRecordError::TestCrash {
            at: token.to_string(),
        }),
        _ => Ok(()),
    }
}

// ── recording one leg ───────────────────────────────────────────────────────

/// Journal, build, commit, move the ref, clear. `paths` are hearth-relative
/// paths in the leg's own repository, already written to disk by the
/// transaction.
pub fn record_leg(
    guarded: &Path,
    repository: &Path,
    meta: &CommitMetadata,
    paths: &[String],
) -> Result<String, ChangeRecordError> {
    let armed = crash_after(guarded)?;
    let barrier = cas_barrier(guarded)?;
    let mut meta = meta.clone();
    meta.paths_recorded = paths.len();
    let parent = read_ref(repository, CHANGE_RECORD_REF)?;
    let manifest = LegManifest {
        repository: repository.to_path_buf(),
        parent: parent.clone(),
        meta,
        paths: hash_paths(repository, paths)?,
    };

    let dir = open_leg(guarded, &manifest)?;
    maybe_crash(&armed, "journalled")?;

    let entries = stage_entries(repository, paths)?;
    let base = match &parent {
        Some(commit) => Some(tree_of(repository, commit)?),
        None => None,
    };
    let tree = build_tree(repository, &dir.join("index"), base.as_deref(), &entries)?;
    maybe_crash(&armed, "tree_written")?;

    let message = render_commit_message(&manifest.meta);
    let mut commit = commit_tree(repository, &tree, parent.as_deref(), &message, Some(&manifest.meta.at))?;
    write_phase(&dir, Phase::Applying)?;
    maybe_crash(&armed, "before_ref_update")?;

    // The ref update is the only mutation of state another process can see,
    // and it is a compare-and-swap against the tip this leg's parent was
    // chosen from. A nonzero exit is a LOST RACE, never a reason to force: the
    // in-process hearth mutex does not span processes, and a cross-repository
    // leg holds no guard at all, so this is the whole of the serialization.
    let mut observed = parent.clone();
    let mut attempt = 1usize;
    loop {
        if let Some(barrier) = &barrier {
            hold_at_barrier(barrier)?;
        }
        if update_ref_cas(repository, CHANGE_RECORD_REF, &commit, observed.as_deref())? {
            break;
        }
        if attempt >= MAX_CAS_ATTEMPTS {
            return Err(ChangeRecordError::Conflict {
                detail: format!(
                    "{} moved under this transaction {} times running; the ref update was \
                     declined rather than forced",
                    CHANGE_RECORD_REF, attempt
                ),
            });
        }
        attempt += 1;
        let tip = read_ref(repository, CHANGE_RECORD_REF)?;
        // The idempotency key the recovery path already has, applied to the
        // retry path: the operation id is per TRANSACTION, so a commit already
        // carrying it is this transaction, already recorded by whoever won the
        // race. Rebuilding and committing again here is how one transaction
        // ends up recorded twice.
        if let Some(landed) = commit_for_operation(
            repository,
            tip.as_deref(),
            manifest.parent.as_deref(),
            &manifest.meta.operation_id,
        )? {
            commit = landed;
            break;
        }
        // Rebuild against the NEW tip, so the losing commit is a child of the
        // winning one and nothing the winner recorded is dropped from the
        // tree. The operation id, the message and the declared `at` are
        // unchanged, so the retry carries exactly the identity the first
        // attempt did.
        let base = match &tip {
            Some(commit) => Some(tree_of(repository, commit)?),
            None => None,
        };
        let tree = build_tree(repository, &dir.join("index"), base.as_deref(), &entries)?;
        commit = commit_tree(repository, &tree, tip.as_deref(), &message, Some(&manifest.meta.at))?;
        observed = tip;
    }
    maybe_crash(&armed, "after_ref_update")?;

    write_phase(&dir, Phase::Committed)?;
    // The durable row is appended HERE, between the committed phase and the
    // clear, so a crash leaves a journal recovery can finish.
    maybe_crash(&armed, "before_row")?;

    clear_leg(&dir)?;
    Ok(commit)
}

fn hash_paths(
    repository: &Path,
    paths: &[String],
) -> Result<Vec<(String, String)>, ChangeRecordError> {
    let mut out = Vec::with_capacity(paths.len());
    for relative in paths {
        let absolute = repository.join(relative);
        let bytes = std::fs::read(&absolute)
            .map_err(|e| io_err(format!("read {}: {}", absolute.display(), e)))?;
        out.push((relative.clone(), content_hash(&bytes)));
    }
    out.sort();
    Ok(out)
}

// ── recovery ────────────────────────────────────────────────────────────────

/// Roll every unfinished leg of every operation this hearth journalled
/// forward, including legs targeting a repository it does not own.
pub fn recover(guarded: &Path) -> Result<Recovery, ChangeRecordError> {
    let mut out = Recovery::default();
    for dir in leg_dirs(guarded)? {
        let phase = read_phase(&dir)?;
        let manifest = read_manifest(&dir)?;
        let id = manifest.meta.operation_id.clone();
        let tip = read_ref(&manifest.repository, CHANGE_RECORD_REF)?;
        let landed = commit_for_operation(
            &manifest.repository,
            tip.as_deref(),
            manifest.parent.as_deref(),
            &id,
        )?;
        match (phase, landed) {
            // Nothing can have landed: the phase flips to `applying` before
            // the ref moves.
            (Phase::Prepared, _) => {
                clear_leg(&dir)?;
                out.discarded.push(id);
            }
            (_, Some(_)) => {
                write_phase(&dir, Phase::Committed)?;
                clear_leg(&dir)?;
                out.verified.push(id);
            }
            (Phase::Committed, None) => out.conflicts.push(format!(
                "operation {} is journalled committed but no commit carrying it is reachable from {}",
                id, CHANGE_RECORD_REF
            )),
            (Phase::Applying, None) => match roll_forward(&dir, &manifest, tip.as_deref()) {
                Ok(()) => out.rolled_forward.push(id),
                Err(ChangeRecordError::Conflict { detail }) => out.conflicts.push(detail),
                Err(other) => return Err(other),
            },
        }
    }
    Ok(out)
}

/// Re-derive the leg from the bytes that SURVIVED on disk and commit them. The
/// journal holds hashes, never content, so this cannot invent what it records.
fn roll_forward(
    dir: &Path,
    manifest: &LegManifest,
    tip: Option<&str>,
) -> Result<(), ChangeRecordError> {
    let repository = manifest.repository.as_path();
    for (relative, expected) in &manifest.paths {
        let absolute = repository.join(relative);
        let live = match std::fs::read(&absolute) {
            Ok(bytes) => content_hash(&bytes),
            Err(e) => {
                return Err(ChangeRecordError::Conflict {
                    detail: format!(
                        "{} is journalled for operation {} but could not be read ({}); disk is \
                         the authority and is left exactly as it is",
                        relative, manifest.meta.operation_id, e
                    ),
                })
            }
        };
        if &live != expected {
            return Err(ChangeRecordError::Conflict {
                detail: format!(
                    "{} changed after operation {} was journalled (expected {}, found {}); disk \
                     is the authority and is left exactly as it is",
                    relative, manifest.meta.operation_id, expected, live
                ),
            });
        }
    }
    let paths: Vec<String> = manifest.paths.iter().map(|(p, _)| p.clone()).collect();
    let entries = stage_entries(repository, &paths)?;
    let base = match tip {
        Some(commit) => Some(tree_of(repository, commit)?),
        None => None,
    };
    let tree = build_tree(repository, &dir.join("index"), base.as_deref(), &entries)?;
    // The message re-renders from the manifest's own fields, `at` included, so
    // the rolled-forward commit is byte-identical to the interrupted one.
    let message = render_commit_message(&manifest.meta);
    let commit = commit_tree(repository, &tree, tip, &message, Some(&manifest.meta.at))?;
    if !update_ref_cas(repository, CHANGE_RECORD_REF, &commit, tip)? {
        return Err(ChangeRecordError::Conflict {
            detail: format!(
                "{} moved during recovery of operation {}",
                CHANGE_RECORD_REF, manifest.meta.operation_id
            ),
        });
    }
    write_phase(dir, Phase::Committed)?;
    clear_leg(dir)
}

// ── journal io ──────────────────────────────────────────────────────────────

/// Every leg directory this hearth journalled, sorted, so a recovery pass is
/// deterministic in its order.
pub fn leg_dirs(guarded: &Path) -> Result<Vec<PathBuf>, ChangeRecordError> {
    let root = journal_root(guarded);
    let mut out = Vec::new();
    let Ok(operations) = std::fs::read_dir(&root) else {
        return Ok(out);
    };
    for operation in operations {
        let operation = operation.map_err(|e| io_err(format!("read {}: {}", root.display(), e)))?;
        if !operation.path().is_dir() {
            continue;
        }
        let legs = std::fs::read_dir(operation.path())
            .map_err(|e| io_err(format!("read {}: {}", operation.path().display(), e)))?;
        for leg in legs {
            let leg = leg.map_err(|e| io_err(format!("read a leg entry: {}", e)))?;
            if leg.path().join("phase").is_file() {
                out.push(leg.path());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn open_leg(guarded: &Path, manifest: &LegManifest) -> Result<PathBuf, ChangeRecordError> {
    let operation = journal_root(guarded).join(sanitize(&manifest.meta.operation_id));
    // One directory per leg, numbered rather than labelled: two legs of one
    // transaction can target repositories whose basenames collide.
    let mut index = 0usize;
    while operation.join(index.to_string()).exists() {
        index += 1;
    }
    let dir = operation.join(index.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| io_err(format!("create {}: {}", dir.display(), e)))?;
    atomic_write(&dir.join("manifest.yaml"), render_manifest(manifest).as_bytes())
        .map_err(|e| io_err(format!("write the manifest: {}", e)))?;
    write_phase(&dir, Phase::Prepared)?;
    Ok(dir)
}

fn write_phase(dir: &Path, phase: Phase) -> Result<(), ChangeRecordError> {
    atomic_write(&dir.join("phase"), phase.as_str().as_bytes())
        .map_err(|e| io_err(format!("write the phase: {}", e)))
}

fn read_phase(dir: &Path) -> Result<Phase, ChangeRecordError> {
    let raw = std::fs::read_to_string(dir.join("phase"))
        .map_err(|e| io_err(format!("read {}/phase: {}", dir.display(), e)))?;
    Phase::parse(&raw).ok_or_else(|| {
        git_err(format!(
            "'{}' is not a journal phase in {}",
            raw.trim(),
            dir.display()
        ))
    })
}

fn clear_leg(dir: &Path) -> Result<(), ChangeRecordError> {
    std::fs::remove_dir_all(dir)
        .map_err(|e| io_err(format!("clear {}: {}", dir.display(), e)))?;
    // An operation directory left behind after its last leg would read as an
    // unfinished operation forever.
    if let Some(operation) = dir.parent() {
        let _ = std::fs::remove_dir(operation);
    }
    Ok(())
}

/// A path segment that cannot climb out of the journal root.
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ── the manifest, hand-rendered ─────────────────────────────────────────────
//
// Every scalar is JSON-encoded, which is valid YAML and removes the whole
// quoting-and-escaping class from a file this module both writes and reads.

fn scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn render_manifest(manifest: &LegManifest) -> String {
    let meta = &manifest.meta;
    let mut out = String::new();
    let mut line = |key: &str, value: String| {
        out.push_str(&format!("{}: {}\n", key, value));
    };
    line("operation_id", scalar(&meta.operation_id));
    line("repository", scalar(&manifest.repository.to_string_lossy()));
    line("parent", scalar(manifest.parent.as_deref().unwrap_or("")));
    line("command", scalar(&meta.command));
    line("artifact_kind", scalar(&meta.artifact_kind));
    line("event_kinds", scalar(&meta.event_kinds.join(",")));
    line("repository_label", scalar(&meta.repository_label));
    line("at", scalar(&meta.at));
    line("actor_hash", scalar(meta.actor_hash.as_deref().unwrap_or("")));
    line(
        "conversation_hash",
        scalar(meta.conversation_hash.as_deref().unwrap_or("")),
    );
    line(
        "project_label",
        scalar(meta.project_label.as_deref().unwrap_or("")),
    );
    line(
        "playbook_run_id",
        scalar(meta.playbook_run_id.as_deref().unwrap_or("")),
    );
    out.push_str("paths:\n");
    for (path, hash) in &manifest.paths {
        out.push_str(&format!("  - {}\n", scalar(&format!("{} {}", hash, path))));
    }
    out
}

fn read_manifest(dir: &Path) -> Result<LegManifest, ChangeRecordError> {
    let raw = std::fs::read_to_string(dir.join("manifest.yaml"))
        .map_err(|e| io_err(format!("read {}/manifest.yaml: {}", dir.display(), e)))?;
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut paths: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        if let Some(entry) = line.trim().strip_prefix("- ") {
            let decoded = decode(entry, dir)?;
            let (hash, path) = decoded
                .split_once(' ')
                .ok_or_else(|| git_err(format!("'{}' is not a journalled path", decoded)))?;
            paths.push((path.to_string(), hash.to_string()));
        } else if let Some((key, value)) = line.split_once(": ") {
            fields.push((key.trim().to_string(), decode(value, dir)?));
        }
    }
    let get = |key: &str| -> String {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let optional = |key: &str| -> Option<String> {
        let value = get(key);
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    };
    let event_kinds = get("event_kinds");
    Ok(LegManifest {
        repository: PathBuf::from(get("repository")),
        parent: optional("parent"),
        meta: CommitMetadata {
            operation_id: get("operation_id"),
            command: get("command"),
            artifact_kind: get("artifact_kind"),
            event_kinds: if event_kinds.is_empty() {
                Vec::new()
            } else {
                event_kinds.split(',').map(str::to_string).collect()
            },
            repository_label: get("repository_label"),
            paths_recorded: paths.len(),
            at: get("at"),
            actor_hash: optional("actor_hash"),
            conversation_hash: optional("conversation_hash"),
            project_label: optional("project_label"),
            playbook_run_id: optional("playbook_run_id"),
        },
        paths,
    })
}

fn decode(raw: &str, dir: &Path) -> Result<String, ChangeRecordError> {
    serde_json::from_str::<String>(raw.trim()).map_err(|e| {
        git_err(format!(
            "{}/manifest.yaml holds an unreadable value {}: {}",
            dir.display(),
            raw.trim(),
            e
        ))
    })
}
