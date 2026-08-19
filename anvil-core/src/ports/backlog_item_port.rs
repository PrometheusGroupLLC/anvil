//! Strict K8 `backlog_item` store port (plan Task 4).
//!
//! This is the ONLY authoritative read/write seam for K8 lifecycle bytes. It is
//! deliberately narrower and stricter than the generic snapshot/query seams:
//!
//! * every read is strict — a malformed, noncontiguous, duplicated, or
//!   mirror-divergent item is a loud error, never a lenient partial value;
//! * every write is a journaled, consume-once compound transaction whose
//!   manifest names each affected `status.yaml`, `item.yaml`, `history.yaml`,
//!   preallocated transition event file, and the single `backlog_items.md`
//!   registry entry, each with an expected old hash-or-absence and its exact
//!   desired bytes;
//! * recovery rolls a journal forward idempotently, discards a merely-prepared
//!   one, and REFUSES (never overwrites) any live value that matches neither
//!   the expected old value nor the exact desired value.
//!
//! There is no silent fallback anywhere in this port: an unexpected shape is an
//! error variant, not a default.

use crate::domain::backlog_item::{BacklogItem, HistoryEntry};
use crate::domain::shared_types::ActorIdentity;
use crate::ports::transition_event_write_port::TransitionRecord;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The hearth-relative directory that holds every K8 item.
pub const BACKLOG_ITEMS_DIR: &str = "backlog_items";
/// The journal root, a dot-directory inside [`BACKLOG_ITEMS_DIR`] so it is
/// never mistaken for an item by an exact-ID scan.
pub const BACKLOG_TRANSACTIONS_DIR: &str = ".transactions";
/// The single K8 registry projection.
pub const BACKLOG_REGISTRY_FILE: &str = "backlog_items.md";

/// Errors surfaced by [`BacklogItemPort`]. Every variant is fail-loud; none is
/// recoverable by substituting a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklogStoreError {
    /// Adapter-level I/O failure.
    Io { message: String },
    /// The requested item id has no directory.
    NotFound { id: String },
    /// A resolved path escaped `<hearth>/backlog_items/<bi_id>`.
    Containment { message: String },
    /// The same `backlog_item_id` was published under more than one directory.
    Duplicate { message: String },
    /// A required file is absent, or a present file is malformed / carries
    /// unknown keys.
    Malformed { path: String, message: String },
    /// `history.yaml` is empty, does not begin with `created(seq: 0)`, is
    /// noncontiguous, or carries a conflicting sequence.
    History { message: String },
    /// The item mirror, the history state-change tail, and the authoritative
    /// transition ledger disagree.
    Mismatch { message: String },
    /// A live byte value matched neither the manifest's expected old value nor
    /// its exact desired value. Never overwritten.
    Conflict { path: String, message: String },
    /// A prepared capability was built against a revision that has since moved,
    /// or was already consumed.
    Stale { message: String },
    /// The caller supplied a value the domain rejects.
    Invalid { message: String },
    /// A `backlog_item` reached a writer that is not the prepared-commit path.
    PreparedTransitionRequired { message: String },
    /// Debug-build test crash point fired (`ANVIL_TEST_BACKLOG_CRASH_AFTER`).
    /// Only constructible under `ANVIL_TEST_MODE=1` inside a temporary hearth.
    TestCrash { at: String },
}

impl fmt::Display for BacklogStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BacklogStoreError::Io { message } => write!(f, "I/O error: {message}"),
            BacklogStoreError::NotFound { id } => write!(f, "backlog item '{id}' not found"),
            BacklogStoreError::Containment { message } => {
                write!(f, "containment violation: {message}")
            }
            BacklogStoreError::Duplicate { message } => write!(f, "duplicate: {message}"),
            BacklogStoreError::Malformed { path, message } => {
                write!(f, "malformed '{path}': {message}")
            }
            BacklogStoreError::History { message } => write!(f, "history: {message}"),
            BacklogStoreError::Mismatch { message } => write!(f, "mismatch: {message}"),
            BacklogStoreError::Conflict { path, message } => {
                write!(f, "conflict at '{path}': {message}")
            }
            BacklogStoreError::Stale { message } => write!(f, "stale: {message}"),
            BacklogStoreError::Invalid { message } => write!(f, "invalid: {message}"),
            BacklogStoreError::PreparedTransitionRequired { message } => write!(
                f,
                "backlog_item requires a prepared K8 transition: {message}"
            ),
            BacklogStoreError::TestCrash { at } => {
                write!(f, "test crash point fired after '{at}'")
            }
        }
    }
}

impl std::error::Error for BacklogStoreError {}

/// One strictly loaded item plus every revision a preparation binds itself to.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedBacklogItem {
    pub id: String,
    /// Hearth-relative directory, always `backlog_items/<bi_id>`.
    pub relative_dir: String,
    pub item: BacklogItem,
    pub history: Vec<HistoryEntry>,
    /// The authoritative per-event ledger, ordered by the fold's causal order.
    pub events: Vec<TransitionRecord>,
    pub status_bytes: String,
    /// Content hashes of the exact live bytes a preparation must still see.
    pub item_hash: String,
    pub history_hash: String,
    pub ledger_hash: String,
    pub status_hash: String,
    /// The highest history sequence currently present.
    pub history_tail_seq: u64,
}

impl LoadedBacklogItem {
    /// The next history sequence a prepared append must claim.
    pub fn next_seq(&self) -> u64 {
        self.history_tail_seq + 1
    }
}

/// Journal phase. Written as its own atomic file so a crash between effects can
/// never leave the phase torn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalPhase {
    Prepared,
    Applying,
    Committed,
}

impl JournalPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            JournalPhase::Prepared => "prepared",
            JournalPhase::Applying => "applying",
            JournalPhase::Committed => "committed",
        }
    }
}

/// One file inside a staged exact-ID genesis directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedFile {
    /// Path relative to the item directory, e.g. `item.yaml` or
    /// `transitions/<preallocated>.yaml`.
    pub relative: String,
    pub contents: String,
}

/// One journaled effect. Every variant carries its target path, the expected
/// old hash (`None` = the target must be absent), and the exact desired bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum BacklogEffect {
    /// Publish an item directory that must not already exist. Genesis only.
    ///
    /// `files` is the complete exact-ID directory, staged inside the journal
    /// and renamed into place as ONE effect, so a crash before the rename
    /// leaves no partial `backlog_items/<bi_id>` and recovery can rebuild the
    /// staging from the manifest alone.
    Publish {
        bi_id: String,
        path: String,
        files: Vec<StagedFile>,
    },
    Status {
        bi_id: String,
        path: String,
        expected_old: Option<String>,
        desired: String,
    },
    Item {
        bi_id: String,
        path: String,
        expected_old: Option<String>,
        desired: String,
    },
    /// An append to `history.yaml`. `expected_seq` names the sequence this
    /// append claims; `desired` is the exact full file after the append so
    /// roll-forward is byte-checkable and idempotent.
    History {
        bi_id: String,
        path: String,
        expected_seq: u64,
        expected_old: Option<String>,
        desired: String,
    },
    /// A preallocated transition event file. The filename is allocated ONCE at
    /// preparation and replayed verbatim by recovery so the fold's causal
    /// same-second tie-break is preserved.
    Event {
        bi_id: String,
        path: String,
        expected_old: Option<String>,
        desired: String,
    },
    Registry {
        path: String,
        expected_old: Option<String>,
        desired: String,
    },
}

impl BacklogEffect {
    pub fn path(&self) -> &str {
        match self {
            BacklogEffect::Publish { path, .. }
            | BacklogEffect::Status { path, .. }
            | BacklogEffect::Item { path, .. }
            | BacklogEffect::History { path, .. }
            | BacklogEffect::Event { path, .. }
            | BacklogEffect::Registry { path, .. } => path,
        }
    }

    /// The `ANVIL_TEST_BACKLOG_CRASH_AFTER` token this effect completes.
    pub fn crash_token(&self) -> String {
        match self {
            BacklogEffect::Publish { bi_id, .. } => format!("after_publish:{bi_id}"),
            BacklogEffect::Status { bi_id, .. } => format!("after_status:{bi_id}"),
            BacklogEffect::Item { bi_id, .. } => format!("after_item:{bi_id}"),
            BacklogEffect::History {
                bi_id,
                expected_seq,
                ..
            } => format!("after_history:{bi_id}:{expected_seq}"),
            BacklogEffect::Event { bi_id, path, .. } => {
                let file = path.rsplit('/').next().unwrap_or(path);
                format!("after_event:{bi_id}:{file}")
            }
            BacklogEffect::Registry { .. } => "after_registry".to_string(),
        }
    }

    /// The coarse target class, used by conflict reporting.
    pub fn class(&self) -> &'static str {
        match self {
            BacklogEffect::Publish { .. } => "publish",
            BacklogEffect::Status { .. } => "status",
            BacklogEffect::Item { .. } => "item",
            BacklogEffect::History { .. } => "history",
            BacklogEffect::Event { .. } => "event",
            BacklogEffect::Registry { .. } => "registry",
        }
    }
}

/// A revision this transaction was prepared against. Verified before `applying`
/// so a mutation that raced another writer refuses instead of clobbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionExpectation {
    pub bi_id: String,
    pub kind: String,
    pub hash: Option<String>,
}

/// The full on-disk journal manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogJournalManifest {
    pub operation_id: String,
    pub operation: String,
    pub actor: ActorIdentity,
    /// Ordered per-target effect vector in byte-ID / path order.
    pub effects: Vec<BacklogEffect>,
    /// Every revision the preparation bound itself to.
    pub expected: Vec<RevisionExpectation>,
}

/// A consume-once compound write. The adapter journals it, verifies every
/// expectation, applies the ordered effect vector, strictly reconciles, and
/// cleans the journal.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedBacklogCommit {
    pub manifest: BacklogJournalManifest,
    /// DECIDE is the one frozen special case: authorization history entries are
    /// appended before any approved position bytes. The adapter asserts the
    /// manifest already encodes that ordering.
    pub decide_commit: bool,
}

/// The strict K8 store. Implementations hold the resolved-hearth lock for the
/// duration of every method.
pub trait BacklogItemPort: Send + Sync {
    /// Roll every interrupted K8 transaction forward (or discard it) before any
    /// read. Idempotent; returns loudly on a third-value conflict.
    fn recover(&self) -> Result<(), BacklogStoreError>;

    /// Strictly load one item by exact id. Recovers first.
    fn load_item(&self, bi_id: &str) -> Result<LoadedBacklogItem, BacklogStoreError>;

    /// Strictly load every published item. Recovers first, then fails loudly on
    /// a duplicate `backlog_item_id`.
    fn load_all(&self) -> Result<Vec<LoadedBacklogItem>, BacklogStoreError>;

    /// Exact-ID genesis: publish `backlog_items/<bi_id>` from a journaled
    /// staging directory. The id must not already exist.
    fn create_genesis(&self, prepared: PreparedBacklogCommit) -> Result<String, BacklogStoreError>;

    /// Consume a prepared compound write. Rejects a stale or already-consumed
    /// capability without any new byte.
    fn commit(&self, prepared: PreparedBacklogCommit) -> Result<(), BacklogStoreError>;

    /// The exact live bytes of the single K8 registry projection, or `None`
    /// when it does not yet exist. Every compound write journals this target,
    /// so its revision must be readable through the same strict seam.
    fn registry_bytes(&self) -> Result<Option<String>, BacklogStoreError>;
}
