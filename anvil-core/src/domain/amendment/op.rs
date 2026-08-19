//! The amendment op model + op log (KD-2 / KD-4).
//!
//! `OpLog` is a `Vec<OpLogEntry>`. `OpLog::push` assigns `seq` as the current
//! `len()` before insertion (0-indexed insertion index = FIFO source of truth).
//! `accepted_at` + `seq` together give the total order returned by `ordered()`:
//! sort by `accepted_at`, then by `seq` to break ties FIFO. Both fields are
//! serde-derived, so the total order is stable across serialization round-trips
//! (KD-2).
//!
//! Identity: each `OpLogEntry` carries an `op_id`. Conflict detection keys on
//! `target_id`; reversal keys on `op_id` (KD-4). Both are pure folds over the
//! log — no prose, no I/O.

use serde::{Deserialize, Serialize};

/// The kind of an amendment operation over a content element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    /// Introduce a new element (carries the new element kind + body + anchor).
    Add,
    /// Replace the body of an existing element.
    Revise,
    /// Omit an element from the rendered body (log entry retained; KD-4).
    Retire,
    /// Move an existing element to a new position via an anchor.
    Reorder,
}

/// Positioning anchor for `Add` and `Reorder` ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddAnchor {
    /// Place immediately after the element with this id.
    After(String),
    /// Place immediately before the element with this id.
    Before(String),
    /// Place at the start of the document.
    AtStart,
    /// Place at the end of the document.
    AtEnd,
}

/// A single amendment operation (in-memory model; KD-2 / KD-4).
///
/// Names its target element by author-assigned stable ID (`target_id`). For
/// `Add`, `target_id` is the NEW element's id (minted by the author) and
/// `new_kind` + `anchor` position it. For `Revise`/`Retire`, `target_id` names
/// the existing element. For `Reorder`, `target_id` names the existing element
/// and `anchor` is its new position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmendmentOp {
    /// The element this op targets (author-assigned stable ID; KD-4).
    pub target_id: String,
    /// The operation kind.
    pub kind: OpKind,
    /// New body text. Required for `Add`/`Revise`; ignored otherwise.
    #[serde(default)]
    pub body: Option<String>,
    /// Element-kind name for an `Add`. Required for `Add`; ignored otherwise.
    #[serde(default)]
    pub new_kind: Option<String>,
    /// Positioning anchor for `Add`/`Reorder`. Ignored for `Revise`/`Retire`.
    #[serde(default)]
    pub anchor: Option<AddAnchor>,
}

/// An op wrapped with its identity + ordering keys (KD-2 / KD-4).
///
/// `op_id` is the reversal key (KD-4). `accepted_at` is the primary sort key for
/// `ordered()`; `seq` is the FIFO tie-break assigned by `OpLog::push`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpLogEntry {
    /// Stable identity of this accepted op — the reversal key (KD-4).
    pub op_id: String,
    /// Accept timestamp — primary ordering key (AC-3).
    pub accepted_at: String,
    /// Insertion sequence — FIFO tie-break, assigned by `OpLog::push`.
    pub seq: u64,
    /// The operation itself.
    pub op: AmendmentOp,
}

/// An append-only ordered log of accepted amendment ops (KD-2).
///
/// Backed by a `Vec<OpLogEntry>` in insertion order. `seq` is the insertion
/// index assigned by `push`. `ordered()` yields the accept-timestamp-then-FIFO
/// ordering AC-3 requires. The log is truth; render is the projection.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpLog {
    entries: Vec<OpLogEntry>,
}

impl OpLog {
    /// An empty log.
    pub fn new() -> Self {
        OpLog {
            entries: Vec::new(),
        }
    }

    /// Append an op with its identity + accept timestamp, assigning `seq` as the
    /// current length (0-indexed insertion index = FIFO source of truth).
    pub fn push(
        &mut self,
        op_id: impl Into<String>,
        accepted_at: impl Into<String>,
        op: AmendmentOp,
    ) {
        let seq = self.entries.len() as u64;
        self.entries.push(OpLogEntry {
            op_id: op_id.into(),
            accepted_at: accepted_at.into(),
            seq,
            op,
        });
    }

    /// Append a fully-formed entry, preserving its existing `op_id`,
    /// `accepted_at`, and `seq` (does NOT reassign `seq`). Used by `reverse` to
    /// rebuild a log without renumbering survivors.
    pub fn push_entry(&mut self, entry: OpLogEntry) {
        self.entries.push(entry);
    }

    /// The raw entries in insertion order.
    pub fn entries(&self) -> &[OpLogEntry] {
        &self.entries
    }

    /// The entries ordered by `accepted_at`, then `seq` (FIFO tie-break).
    ///
    /// Stable across serde round-trips: both keys are persisted fields. Uses a
    /// stable sort so equal-`accepted_at` entries retain `seq` order even were
    /// the seqs ever non-monotonic.
    pub fn ordered(&self) -> Vec<OpLogEntry> {
        let mut ordered = self.entries.clone();
        ordered.sort_by(|a, b| {
            a.accepted_at
                .cmp(&b.accepted_at)
                .then_with(|| a.seq.cmp(&b.seq))
        });
        ordered
    }

    /// The number of entries in the log.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
