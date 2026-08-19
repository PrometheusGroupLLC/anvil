//! Filesystem implementation of [`BacklogItemPort`] — the strict K8 store and
//! its recoverable compound transaction (plan Task 4).
//!
//! Layout, exactly:
//!
//! ```text
//! <hearth>/backlog_items/<bi_id>/status.yaml
//! <hearth>/backlog_items/<bi_id>/item.yaml
//! <hearth>/backlog_items/<bi_id>/history.yaml
//! <hearth>/backlog_items/<bi_id>/transitions/<preallocated>.yaml
//! <hearth>/backlog_items/.transactions/<operation_id>/{manifest.yaml,phase,staging/}
//! <hearth>/backlog_items.md
//! ```
//!
//! Every public read calls [`FileSystemBacklogItemAdapter::recover`] FIRST, so a
//! Catalog / Checkin / Describe scan that is the first caller after an
//! interruption never observes a partial write. Recovery is fixed:
//!
//! * `prepared` — no effect can have landed; discard staging and the journal.
//! * `applying` — roll forward idempotently in manifest order.
//! * `committed` — verify every effect landed, then remove the journal.
//!
//! For every target the live value must equal either the manifest's expected
//! old value/absence or the exact desired bytes. A third value is a loud
//! [`BacklogStoreError::Conflict`] and is NEVER overwritten.

use anvil_core::domain::backlog_item::{self as bi, BacklogItem, HistoryEntry, HistoryKind, State};
use anvil_core::domain::content_hash::content_hash;
use anvil_core::domain::transition_log::read_event_files_strict;
use crate::atomic_write::atomic_write;
use anvil_core::ports::backlog_item_port::{
    BacklogEffect, BacklogItemPort, BacklogJournalManifest, BacklogStoreError, JournalPhase,
    LoadedBacklogItem, PreparedBacklogCommit, RevisionExpectation, BACKLOG_ITEMS_DIR,
    BACKLOG_REGISTRY_FILE, BACKLOG_TRANSACTIONS_DIR,
};
use std::path::{Path, PathBuf};

/// The hash of a file that may be absent. `None` means "must not exist".
fn hash_at(path: &Path) -> Result<Option<String>, BacklogStoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(content_hash(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(BacklogStoreError::Io {
            message: format!("read {}: {e}", path.display()),
        }),
    }
}

pub struct FileSystemBacklogItemAdapter {
    hearth_path: PathBuf,
}

impl FileSystemBacklogItemAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }

    pub fn hearth_path(&self) -> &Path {
        &self.hearth_path
    }

    pub fn items_dir(&self) -> PathBuf {
        self.hearth_path.join(BACKLOG_ITEMS_DIR)
    }

    pub fn journal_root(&self) -> PathBuf {
        self.items_dir().join(BACKLOG_TRANSACTIONS_DIR)
    }

    pub fn registry_path(&self) -> PathBuf {
        self.hearth_path.join(BACKLOG_REGISTRY_FILE)
    }

    /// Resolve `backlog_items/<bi_id>` and prove containment. The id grammar is
    /// validated first so a traversal segment can never reach the join.
    pub fn item_dir(&self, bi_id: &str) -> Result<PathBuf, BacklogStoreError> {
        bi::validate_backlog_item_id(bi_id).map_err(|e| BacklogStoreError::Invalid {
            message: format!("backlog item id '{bi_id}': {e}"),
        })?;
        let dir = self.items_dir().join(bi_id);
        // Defense in depth: the grammar already excludes `/`, `.` and `\`, but a
        // containment proof is cheap and this is the only path builder.
        let base = self.items_dir();
        if !dir.starts_with(&base) || dir.parent() != Some(base.as_path()) {
            return Err(BacklogStoreError::Containment {
                message: format!("'{bi_id}' does not resolve inside {}", base.display()),
            });
        }
        Ok(dir)
    }

    /// Hearth-relative form of an absolute path under this hearth.
    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.hearth_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn absolute(&self, relative: &str) -> PathBuf {
        self.hearth_path.join(relative)
    }

    // ── crash points ────────────────────────────────────────────────────────

    /// Whether the debug-only crash points are armed for THIS hearth.
    ///
    /// Three independent conditions, all required: a debug build, explicit
    /// `ANVIL_TEST_MODE=1`, and a hearth that lives under the platform
    /// temporary directory. A release build ignores both keys entirely.
    fn crash_after(&self) -> Result<Option<String>, BacklogStoreError> {
        if !cfg!(debug_assertions) {
            return Ok(None);
        }
        if std::env::var("ANVIL_TEST_MODE").ok().as_deref() != Some("1") {
            return Ok(None);
        }
        let raw = match std::env::var("ANVIL_TEST_BACKLOG_CRASH_AFTER") {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let canonical =
            std::fs::canonicalize(&self.hearth_path).unwrap_or_else(|_| self.hearth_path.clone());
        let tmp =
            std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
        if !canonical.starts_with(&tmp) {
            return Err(BacklogStoreError::Invalid {
                message: "ANVIL_TEST_BACKLOG_CRASH_AFTER is only honored for a temporary hearth"
                    .to_string(),
            });
        }
        validate_crash_token(&raw)?;
        Ok(Some(raw))
    }

    fn maybe_crash(&self, armed: &Option<String>, token: &str) -> Result<(), BacklogStoreError> {
        match armed {
            Some(value) if value == token => Err(BacklogStoreError::TestCrash {
                at: token.to_string(),
            }),
            _ => Ok(()),
        }
    }

    // ── strict load ─────────────────────────────────────────────────────────

    /// Strictly load one item WITHOUT running recovery. Used internally after
    /// recovery has already run once for the whole call.
    fn load_item_unrecovered(&self, bi_id: &str) -> Result<LoadedBacklogItem, BacklogStoreError> {
        let dir = self.item_dir(bi_id)?;
        if !dir.is_dir() {
            return Err(BacklogStoreError::NotFound {
                id: bi_id.to_string(),
            });
        }

        let status_path = dir.join("status.yaml");
        let item_path = dir.join("item.yaml");
        let history_path = dir.join("history.yaml");

        let status_bytes = read_required(&status_path)?;
        // A malformed K8 status.yaml is an error, never an empty default.
        serde_yaml::from_str::<serde_yaml::Value>(&status_bytes).map_err(|e| {
            BacklogStoreError::Malformed {
                path: self.relative(&status_path),
                message: format!("status.yaml is not parseable YAML: {e}"),
            }
        })?;

        let item_bytes = read_required(&item_path)?;
        let item: BacklogItem =
            serde_yaml::from_str(&item_bytes).map_err(|e| BacklogStoreError::Malformed {
                path: self.relative(&item_path),
                message: format!("item.yaml: {e}"),
            })?;
        if item.backlog_item_id != bi_id {
            return Err(BacklogStoreError::Mismatch {
                message: format!(
                    "item.yaml declares id '{}' but is published at '{bi_id}'",
                    item.backlog_item_id
                ),
            });
        }
        bi::validate_item_semantics(&item).map_err(|e| BacklogStoreError::Malformed {
            path: self.relative(&item_path),
            message: e.to_string(),
        })?;

        let history_bytes = read_required(&history_path).map_err(|e| match e {
            BacklogStoreError::Malformed { .. } => BacklogStoreError::History {
                message: format!("history.yaml is absent for '{bi_id}'"),
            },
            other => other,
        })?;
        let history: Vec<HistoryEntry> =
            serde_yaml::from_str(&history_bytes).map_err(|e| BacklogStoreError::Malformed {
                path: self.relative(&history_path),
                message: format!("history.yaml: {e}"),
            })?;
        // Contiguity, created(seq: 0) first, no genesis state_change, no
        // conflicting sequence — the reconcile layer owns the always-required
        // history rule (§1.3).
        bi::validate_history(&history).map_err(|e| BacklogStoreError::History {
            message: e.to_string(),
        })?;

        // The authoritative ledger. Strict only — the lenient reader would drop
        // exactly the event whose absence lets a mirror lie.
        let event_files =
            read_event_files_strict(&dir).map_err(|e| BacklogStoreError::Malformed {
                path: self.relative(&dir.join("transitions")),
                message: e.to_string(),
            })?;
        let mut ordered = event_files;
        ordered.sort_by(|a, b| {
            a.record
                .at
                .cmp(&b.record.at)
                .then(a.file_name.cmp(&b.file_name))
        });
        // A duplicate transition event (byte-identical content under two
        // filenames) is damage, not history.
        for i in 1..ordered.len() {
            if ordered[i].record == ordered[i - 1].record {
                return Err(BacklogStoreError::Duplicate {
                    message: format!(
                        "duplicate transition event for '{bi_id}': '{}' repeats '{}'",
                        ordered[i].file_name,
                        ordered[i - 1].file_name
                    ),
                });
            }
        }
        let events: Vec<_> = ordered.iter().map(|e| e.record.clone()).collect();

        // ── reconcile item mirror ↔ history ↔ ledger ────────────────────────
        let state_changes: Vec<&HistoryEntry> = history
            .iter()
            .filter(|e| e.kind == HistoryKind::StateChange)
            .collect();
        // Genesis records ONE authoritative transition event plus
        // `created(seq: 0)` and NEVER a genesis `state_change` (§1). So the
        // ledger always leads the state-change mirror by exactly that one
        // genesis event.
        let genesis = events.first().ok_or_else(|| BacklogStoreError::Mismatch {
            message: format!("'{bi_id}' has no authoritative genesis transition event"),
        })?;
        if genesis.to != State::Candidate.as_str() {
            return Err(BacklogStoreError::Mismatch {
                message: format!(
                    "'{bi_id}' genesis event targets '{}' rather than candidate",
                    genesis.to
                ),
            });
        }
        let post_genesis = &events[1..];
        if state_changes.len() != post_genesis.len() {
            return Err(BacklogStoreError::Mismatch {
                message: format!(
                    "'{bi_id}' has {} state_change history entries but {} post-genesis \
                     authoritative transition events",
                    state_changes.len(),
                    post_genesis.len()
                ),
            });
        }
        for (entry, event) in state_changes.iter().zip(post_genesis.iter()) {
            let to = entry.to_state.map(|s| s.as_str()).unwrap_or("");
            if to != event.to || entry.actor != event.actor || entry.role.as_str() != event.role {
                return Err(BacklogStoreError::Mismatch {
                    message: format!(
                        "'{bi_id}' state_change(seq {}) → {to}/{}/{} does not mirror its \
                         transition event → {}/{}/{}",
                        entry.seq,
                        entry.actor,
                        entry.role.as_str(),
                        event.to,
                        event.actor,
                        event.role
                    ),
                });
            }
        }
        let last = events.last().expect("the genesis event is present");
        let resolved = parse_state(&last.to).ok_or_else(|| BacklogStoreError::Mismatch {
            message: format!("'{bi_id}' ledger names unknown state '{}'", last.to),
        })?;
        if item.state != resolved {
            return Err(BacklogStoreError::Mismatch {
                message: format!(
                    "'{bi_id}' item mirror says '{}' but the authoritative ledger resolves \
                     '{}'",
                    item.state.as_str(),
                    resolved.as_str()
                ),
            });
        }

        let history_tail_seq = history.last().map(|e| e.seq).unwrap_or(0);
        let ledger_repr = ordered
            .iter()
            .map(|e| format!("{}\u{1}{:?}", e.file_name, e.record))
            .collect::<Vec<_>>()
            .join("\u{2}");

        Ok(LoadedBacklogItem {
            id: bi_id.to_string(),
            relative_dir: self.relative(&dir),
            item,
            history,
            events,
            item_hash: content_hash(item_bytes.as_bytes()),
            history_hash: content_hash(history_bytes.as_bytes()),
            ledger_hash: content_hash(ledger_repr.as_bytes()),
            status_hash: content_hash(status_bytes.as_bytes()),
            status_bytes,
            history_tail_seq,
        })
    }

    /// Every published item id, in byte order. Skips the dot-journal.
    pub fn published_ids(&self) -> Result<Vec<String>, BacklogStoreError> {
        let dir = self.items_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(BacklogStoreError::Io {
                    message: format!("read_dir {}: {e}", dir.display()),
                })
            }
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| BacklogStoreError::Io {
                message: format!("read_dir {}: {e}", dir.display()),
            })?;
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            ids.push(name);
        }
        ids.sort();
        Ok(ids)
    }

    // ── journal ─────────────────────────────────────────────────────────────

    fn write_phase(&self, journal: &Path, phase: JournalPhase) -> Result<(), BacklogStoreError> {
        atomic_write(&journal.join("phase"), phase.as_str().as_bytes()).map_err(|e| {
            BacklogStoreError::Io {
                message: format!("write phase in {}: {e}", journal.display()),
            }
        })
    }

    fn read_phase(&self, journal: &Path) -> Result<JournalPhase, BacklogStoreError> {
        let raw =
            std::fs::read_to_string(journal.join("phase")).map_err(|e| BacklogStoreError::Io {
                message: format!("read phase in {}: {e}", journal.display()),
            })?;
        match raw.trim() {
            "prepared" => Ok(JournalPhase::Prepared),
            "applying" => Ok(JournalPhase::Applying),
            "committed" => Ok(JournalPhase::Committed),
            other => Err(BacklogStoreError::Malformed {
                path: self.relative(&journal.join("phase")),
                message: format!("unknown journal phase '{other}'"),
            }),
        }
    }

    fn read_manifest(&self, journal: &Path) -> Result<BacklogJournalManifest, BacklogStoreError> {
        let path = journal.join("manifest.yaml");
        let raw = std::fs::read_to_string(&path).map_err(|e| BacklogStoreError::Io {
            message: format!("read {}: {e}", path.display()),
        })?;
        serde_yaml::from_str(&raw).map_err(|e| BacklogStoreError::Malformed {
            path: self.relative(&path),
            message: format!("manifest.yaml: {e}"),
        })
    }

    fn journal_dirs(&self) -> Result<Vec<PathBuf>, BacklogStoreError> {
        let root = self.journal_root();
        let entries = match std::fs::read_dir(&root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(BacklogStoreError::Io {
                    message: format!("read_dir {}: {e}", root.display()),
                })
            }
        };
        let mut dirs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| BacklogStoreError::Io {
                message: format!("read_dir {}: {e}", root.display()),
            })?;
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
        dirs.sort();
        Ok(dirs)
    }

    /// Verify every revision the preparation bound itself to. A moved revision
    /// is [`BacklogStoreError::Stale`] and no byte is written.
    fn verify_expectations(
        &self,
        expected: &[RevisionExpectation],
    ) -> Result<(), BacklogStoreError> {
        for exp in expected {
            let live = self.live_revision(&exp.bi_id, &exp.kind)?;
            if live != exp.hash {
                return Err(BacklogStoreError::Stale {
                    message: format!(
                        "'{}' {} moved after the capability was prepared",
                        exp.bi_id, exp.kind
                    ),
                });
            }
        }
        Ok(())
    }

    fn live_revision(&self, bi_id: &str, kind: &str) -> Result<Option<String>, BacklogStoreError> {
        match kind {
            "registry" => hash_at(&self.registry_path()),
            "item" => hash_at(&self.item_dir(bi_id)?.join("item.yaml")),
            "history" => hash_at(&self.item_dir(bi_id)?.join("history.yaml")),
            "status" => hash_at(&self.item_dir(bi_id)?.join("status.yaml")),
            "ledger" => {
                let dir = self.item_dir(bi_id)?;
                if !dir.is_dir() {
                    return Ok(None);
                }
                match self.load_item_unrecovered(bi_id) {
                    Ok(loaded) => Ok(Some(loaded.ledger_hash)),
                    Err(BacklogStoreError::NotFound { .. }) => Ok(None),
                    Err(e) => Err(e),
                }
            }
            "context" => {
                // An aggregate over the whole same-organ ranked set; the
                // preparer supplies the hash and we recompute it the same way.
                let mut parts = Vec::new();
                for id in self.published_ids()? {
                    let loaded = self.load_item_unrecovered(&id)?;
                    parts.push(format!(
                        "{id}\u{1}{}\u{1}{}",
                        loaded.item_hash, loaded.history_hash
                    ));
                }
                Ok(Some(content_hash(parts.join("\u{2}").as_bytes())))
            }
            other => Err(BacklogStoreError::Invalid {
                message: format!("unknown revision kind '{other}'"),
            }),
        }
    }

    /// Aggregate strict organ-context hash over every published item.
    pub fn organ_context_hash(&self) -> Result<String, BacklogStoreError> {
        match self.live_revision("", "context")? {
            Some(h) => Ok(h),
            None => Ok(content_hash(b"")),
        }
    }

    /// Apply the ordered effect vector idempotently. Every target must be at
    /// its expected old value (apply), already at the desired value (skip), or
    /// a third value (loud conflict).
    fn apply_effects(
        &self,
        manifest: &BacklogJournalManifest,
        journal: &Path,
        armed: &Option<String>,
    ) -> Result<(), BacklogStoreError> {
        for effect in &manifest.effects {
            match effect {
                BacklogEffect::Publish { bi_id, path, files } => {
                    let target = self.absolute(path);
                    if target.exists() {
                        // Already published — the idempotent roll-forward case.
                        // Every desired byte is verified by the reconcile step
                        // and by `verify_committed`; a third value there is a
                        // loud conflict, never an overwrite.
                        for file in files {
                            let live = hash_at(&target.join(&file.relative))?;
                            if live.as_deref()
                                != Some(content_hash(file.contents.as_bytes()).as_str())
                            {
                                return Err(BacklogStoreError::Conflict {
                                    path: format!("{path}/{}", file.relative),
                                    message: "a published genesis file holds a third value"
                                        .to_string(),
                                });
                            }
                        }
                        continue;
                    }
                    // Rebuild staging from the manifest so recovery never needs
                    // surviving scratch bytes.
                    let staged = journal.join("staging").join(bi_id);
                    let _ = std::fs::remove_dir_all(&staged);
                    stage_directory(&staged, files)?;
                    std::fs::create_dir_all(target.parent().unwrap_or(&target)).map_err(|e| {
                        BacklogStoreError::Io {
                            message: format!("create parent for {}: {e}", target.display()),
                        }
                    })?;
                    std::fs::rename(&staged, &target).map_err(|e| BacklogStoreError::Io {
                        message: format!(
                            "publish {} -> {}: {e}",
                            staged.display(),
                            target.display()
                        ),
                    })?;
                }
                BacklogEffect::Status {
                    path,
                    expected_old,
                    desired,
                    ..
                }
                | BacklogEffect::Item {
                    path,
                    expected_old,
                    desired,
                    ..
                }
                | BacklogEffect::Event {
                    path,
                    expected_old,
                    desired,
                    ..
                }
                | BacklogEffect::Registry {
                    path,
                    expected_old,
                    desired,
                    ..
                } => {
                    self.apply_whole_file(path, expected_old.as_deref(), desired, effect.class())?;
                }
                BacklogEffect::History {
                    path,
                    expected_old,
                    desired,
                    ..
                } => {
                    // Append-only: never truncate or rewrite a prior entry. The
                    // desired bytes are the exact full file AFTER the append, so
                    // roll-forward is byte-checkable, and the delta is what is
                    // actually appended.
                    self.apply_history_append(path, expected_old.as_deref(), desired)?;
                }
            }
            self.maybe_crash(armed, &effect.crash_token())?;
        }
        Ok(())
    }

    fn apply_whole_file(
        &self,
        rel: &str,
        expected_old: Option<&str>,
        desired: &str,
        class: &str,
    ) -> Result<(), BacklogStoreError> {
        let path = self.absolute(rel);
        let live = hash_at(&path)?;
        let desired_hash = content_hash(desired.as_bytes());
        if live.as_deref() == Some(desired_hash.as_str()) {
            return Ok(()); // idempotent roll-forward
        }
        if live.as_deref() != expected_old {
            return Err(BacklogStoreError::Conflict {
                path: rel.to_string(),
                message: format!(
                    "live {class} bytes match neither the expected old value nor the desired \
                     value; refusing to overwrite a third value"
                ),
            });
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BacklogStoreError::Io {
                message: format!("create {}: {e}", parent.display()),
            })?;
        }
        atomic_write(&path, desired.as_bytes()).map_err(|e| BacklogStoreError::Io {
            message: format!("write {}: {e}", path.display()),
        })
    }

    fn apply_history_append(
        &self,
        rel: &str,
        expected_old: Option<&str>,
        desired: &str,
    ) -> Result<(), BacklogStoreError> {
        use std::io::Write;
        let path = self.absolute(rel);
        let live_bytes = match std::fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(BacklogStoreError::Io {
                    message: format!("read {}: {e}", path.display()),
                })
            }
        };
        let live_hash = live_bytes.as_deref().map(content_hash);
        if live_hash.as_deref() == Some(content_hash(desired.as_bytes()).as_str()) {
            return Ok(()); // already appended
        }
        if live_hash.as_deref() != expected_old {
            return Err(BacklogStoreError::Conflict {
                path: rel.to_string(),
                message: "live history bytes match neither the expected old value nor the \
                          desired value; refusing to overwrite a third value"
                    .to_string(),
            });
        }
        let prefix_len = live_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
        let desired_bytes = desired.as_bytes();
        if desired_bytes.len() < prefix_len
            || live_bytes
                .as_ref()
                .map(|b| &desired_bytes[..prefix_len] != b.as_slice())
                .unwrap_or(false)
        {
            return Err(BacklogStoreError::Conflict {
                path: rel.to_string(),
                message: "the desired history is not an append over the live history".to_string(),
            });
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BacklogStoreError::Io {
                message: format!("create {}: {e}", parent.display()),
            })?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| BacklogStoreError::Io {
                message: format!("open {} for append: {e}", path.display()),
            })?;
        file.write_all(&desired_bytes[prefix_len..])
            .map_err(|e| BacklogStoreError::Io {
                message: format!("append {}: {e}", path.display()),
            })?;
        Ok(())
    }

    /// Verify a `committed` journal actually landed every effect, then clean it.
    fn verify_committed(&self, manifest: &BacklogJournalManifest) -> Result<(), BacklogStoreError> {
        for effect in &manifest.effects {
            match effect {
                BacklogEffect::Publish { path, .. } => {
                    if !self.absolute(path).exists() {
                        return Err(BacklogStoreError::Conflict {
                            path: path.clone(),
                            message: "a committed journal names a directory that is absent"
                                .to_string(),
                        });
                    }
                }
                other => {
                    let (path, desired) = match other {
                        BacklogEffect::Status { path, desired, .. }
                        | BacklogEffect::Item { path, desired, .. }
                        | BacklogEffect::History { path, desired, .. }
                        | BacklogEffect::Event { path, desired, .. }
                        | BacklogEffect::Registry { path, desired, .. } => (path, desired),
                        BacklogEffect::Publish { .. } => unreachable!(),
                    };
                    let live = hash_at(&self.absolute(path))?;
                    if live.as_deref() != Some(content_hash(desired.as_bytes()).as_str()) {
                        return Err(BacklogStoreError::Conflict {
                            path: path.clone(),
                            message: "a committed journal names bytes that are not live"
                                .to_string(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn discard_journal(&self, journal: &Path) -> Result<(), BacklogStoreError> {
        std::fs::remove_dir_all(journal).map_err(|e| BacklogStoreError::Io {
            message: format!("remove journal {}: {e}", journal.display()),
        })
    }

    /// Stage, verify, apply, reconcile, commit, clean. The one write topology.
    fn run_transaction(&self, prepared: PreparedBacklogCommit) -> Result<(), BacklogStoreError> {
        let armed = self.crash_after()?;
        let manifest = prepared.manifest;
        if prepared.decide_commit {
            // DECIDE is the one frozen ordering special case: every byte-exact
            // `rank_committed` authorization entry must precede the first
            // approved position byte. The manifest is rejected outright rather
            // than silently reordered.
            let first_item = manifest
                .effects
                .iter()
                .position(|e| matches!(e, BacklogEffect::Item { .. }));
            let last_history = manifest
                .effects
                .iter()
                .rposition(|e| matches!(e, BacklogEffect::History { .. }));
            if let (Some(item_at), Some(hist_at)) = (first_item, last_history) {
                if item_at < hist_at {
                    return Err(BacklogStoreError::Invalid {
                        message: "a DECIDE commit manifest writes a position before its \
                                  authorization entry"
                            .to_string(),
                    });
                }
            }
        }
        let journal = self.journal_root().join(&manifest.operation_id);
        if journal.exists() {
            return Err(BacklogStoreError::Stale {
                message: format!(
                    "operation '{}' was already journaled — a prepared capability is \
                     consume-once",
                    manifest.operation_id
                ),
            });
        }
        std::fs::create_dir_all(&journal).map_err(|e| BacklogStoreError::Io {
            message: format!("create journal {}: {e}", journal.display()),
        })?;

        let manifest_yaml =
            serde_yaml::to_string(&manifest).map_err(|e| BacklogStoreError::Invalid {
                message: format!("serialize manifest: {e}"),
            })?;
        atomic_write(&journal.join("manifest.yaml"), manifest_yaml.as_bytes()).map_err(|e| {
            BacklogStoreError::Io {
                message: format!("write manifest in {}: {e}", journal.display()),
            }
        })?;
        self.write_phase(&journal, JournalPhase::Prepared)?;
        self.maybe_crash(&armed, "prepared")?;

        // Verify every bound revision BEFORE a single live byte moves.
        if let Err(e) = self.verify_expectations(&manifest.expected) {
            let _ = self.discard_journal(&journal);
            return Err(e);
        }

        self.write_phase(&journal, JournalPhase::Applying)?;
        self.maybe_crash(&armed, "applying")?;
        self.apply_effects(&manifest, &journal, &armed)?;

        // Strictly reconcile every touched item before declaring committed.
        for id in touched_ids(&manifest) {
            self.load_item_unrecovered(&id)?;
        }

        self.write_phase(&journal, JournalPhase::Committed)?;
        self.maybe_crash(&armed, "committed")?;
        self.discard_journal(&journal)?;
        self.maybe_crash(&armed, "after_cleanup")?;
        Ok(())
    }
}

/// Materialize a complete staged exact-ID directory from manifest bytes.
fn stage_directory(
    staged: &Path,
    files: &[anvil_core::ports::backlog_item_port::StagedFile],
) -> Result<(), BacklogStoreError> {
    std::fs::create_dir_all(staged).map_err(|e| BacklogStoreError::Io {
        message: format!("stage {}: {e}", staged.display()),
    })?;
    for file in files {
        if file.relative.contains("..") || file.relative.starts_with('/') {
            return Err(BacklogStoreError::Containment {
                message: format!("staged file '{}' escapes the item directory", file.relative),
            });
        }
        let path = staged.join(&file.relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| BacklogStoreError::Io {
                message: format!("stage {}: {e}", parent.display()),
            })?;
        }
        std::fs::write(&path, &file.contents).map_err(|e| BacklogStoreError::Io {
            message: format!("stage {}: {e}", path.display()),
        })?;
    }
    Ok(())
}

fn touched_ids(manifest: &BacklogJournalManifest) -> Vec<String> {
    let mut ids: Vec<String> = manifest
        .effects
        .iter()
        .filter_map(|e| match e {
            BacklogEffect::Publish { bi_id, .. }
            | BacklogEffect::Status { bi_id, .. }
            | BacklogEffect::Item { bi_id, .. }
            | BacklogEffect::History { bi_id, .. }
            | BacklogEffect::Event { bi_id, .. } => Some(bi_id.clone()),
            BacklogEffect::Registry { .. } => None,
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn read_required(path: &Path) -> Result<String, BacklogStoreError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(BacklogStoreError::Malformed {
            path: path.display().to_string(),
            message: "required file is absent".to_string(),
        }),
        Err(e) => Err(BacklogStoreError::Io {
            message: format!("read {}: {e}", path.display()),
        }),
    }
}

fn parse_state(raw: &str) -> Option<State> {
    match raw {
        "candidate" => Some(State::Candidate),
        "ready" => Some(State::Ready),
        "in_flight" => Some(State::InFlight),
        "done" => Some(State::Done),
        "parked" => Some(State::Parked),
        "superseded" => Some(State::Superseded),
        "aged_out" => Some(State::AgedOut),
        _ => None,
    }
}

/// The exact `ANVIL_TEST_BACKLOG_CRASH_AFTER` grammar. Any other value is
/// rejected loudly rather than silently ignored.
pub fn validate_crash_token(raw: &str) -> Result<(), BacklogStoreError> {
    let ok = match raw {
        "prepared" | "applying" | "after_registry" | "committed" | "after_cleanup" => true,
        other => {
            let mut parts = other.splitn(2, ':');
            let head = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("");
            match head {
                "after_publish" | "after_status" | "after_item" => {
                    !rest.is_empty() && !rest.contains(':')
                }
                "after_history" => {
                    let mut it = rest.splitn(2, ':');
                    let id = it.next().unwrap_or("");
                    let seq = it.next().unwrap_or("");
                    !id.is_empty() && !seq.is_empty() && seq.chars().all(|c| c.is_ascii_digit())
                }
                "after_event" => {
                    let mut it = rest.splitn(2, ':');
                    let id = it.next().unwrap_or("");
                    let file = it.next().unwrap_or("");
                    !id.is_empty() && file.ends_with(".yaml")
                }
                _ => false,
            }
        }
    };
    if ok {
        Ok(())
    } else {
        Err(BacklogStoreError::Invalid {
            message: format!("unknown ANVIL_TEST_BACKLOG_CRASH_AFTER value '{raw}'"),
        })
    }
}

impl BacklogItemPort for FileSystemBacklogItemAdapter {
    fn recover(&self) -> Result<(), BacklogStoreError> {
        let armed = self.crash_after()?;
        for journal in self.journal_dirs()? {
            let phase = self.read_phase(&journal)?;
            let manifest = self.read_manifest(&journal)?;
            match phase {
                // No effect can have landed: the phase flips to `applying`
                // before the first byte moves. Discard staging and the journal.
                JournalPhase::Prepared => self.discard_journal(&journal)?,
                JournalPhase::Applying => {
                    self.apply_effects(&manifest, &journal, &armed)?;
                    for id in touched_ids(&manifest) {
                        self.load_item_unrecovered(&id)?;
                    }
                    self.write_phase(&journal, JournalPhase::Committed)?;
                    self.discard_journal(&journal)?;
                }
                JournalPhase::Committed => {
                    self.verify_committed(&manifest)?;
                    self.discard_journal(&journal)?;
                }
            }
        }
        Ok(())
    }

    fn load_item(&self, bi_id: &str) -> Result<LoadedBacklogItem, BacklogStoreError> {
        self.recover()?;
        self.load_item_unrecovered(bi_id)
    }

    fn load_all(&self) -> Result<Vec<LoadedBacklogItem>, BacklogStoreError> {
        self.recover()?;
        let ids = self.published_ids()?;
        // Duplicate publication is checked FIRST, from the declared ids alone.
        // Checking it after the per-item strict load would report the second
        // copy as a mere id/directory mismatch and hide the real damage.
        let mut declared: Vec<(String, String)> = Vec::new();
        for id in &ids {
            let item_path = self.item_dir(id)?.join("item.yaml");
            if let Ok(raw) = std::fs::read_to_string(&item_path) {
                if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
                    if let Some(declared_id) = value.get("backlog_item_id").and_then(|v| v.as_str())
                    {
                        declared.push((id.clone(), declared_id.to_string()));
                    }
                }
            }
        }
        for i in 0..declared.len() {
            for j in (i + 1)..declared.len() {
                if declared[i].1 == declared[j].1 {
                    return Err(BacklogStoreError::Duplicate {
                        message: format!(
                            "backlog item id '{}' is published under more than one directory                              ('{}' and '{}')",
                            declared[i].1, declared[i].0, declared[j].0
                        ),
                    });
                }
            }
        }
        let mut out = Vec::new();
        for id in ids {
            out.push(self.load_item_unrecovered(&id)?);
        }
        Ok(out)
    }

    fn create_genesis(&self, prepared: PreparedBacklogCommit) -> Result<String, BacklogStoreError> {
        self.recover()?;
        let publish = prepared
            .manifest
            .effects
            .iter()
            .find_map(|e| match e {
                BacklogEffect::Publish { bi_id, path, .. } => Some((bi_id.clone(), path.clone())),
                _ => None,
            })
            .ok_or_else(|| BacklogStoreError::Invalid {
                message: "a genesis commit must carry exactly one publish effect".to_string(),
            })?;
        if self.item_dir(&publish.0)?.exists() {
            return Err(BacklogStoreError::Duplicate {
                message: format!("backlog item '{}' already exists", publish.0),
            });
        }
        self.commit(prepared)?;
        Ok(publish.1)
    }

    fn commit(&self, prepared: PreparedBacklogCommit) -> Result<(), BacklogStoreError> {
        self.recover()?;
        self.run_transaction(prepared)
    }

    fn registry_bytes(&self) -> Result<Option<String>, BacklogStoreError> {
        let path = self.hearth_path.join(BACKLOG_REGISTRY_FILE);
        match std::fs::read_to_string(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BacklogStoreError::Io {
                message: format!("read {}: {e}", path.display()),
            }),
        }
    }
}
