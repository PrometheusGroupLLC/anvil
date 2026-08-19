//! Append-only durable sink for the UNIVERSAL, redacted per-turn activity log.
//!
//! Where the routing-activity sink records ONLY a single resolved route, this
//! sink records EVERY command turn the engine serves (begin/checkin/complete/
//! route/snapshot/amend/persist_playbook/intake_candidate_playbook/describe/
//! catalog) so usage can be measured end-to-end — not just the routed slice.
//! The engine emits per-command `tracing` events on stderr, but that sink is not
//! queryable; this narrow port persists ONE redacted record per command turn and
//! reads them back for aggregation (the `activity_summary` fold).
//!
//! ## Redaction (Part-3 allowlist)
//!
//! An [`ActivityLogRecord`] carries ONLY the allowlisted fields — the `command`
//! name (a verb label), the `outcome` (a variant-name label, not free text), the
//! resolved `workflow_kind` where the command has one (else empty), a salted,
//! non-reversible `actor_hash` (`None` when no salt is configured), and an
//! ISO-8601 `at` timestamp. Join-participant records may also carry optional
//! hashed/labelled correlation keys. It NEVER carries the surface message text,
//! identities (the actor name is salted, never raw), paths, or any other raw
//! input.
//!
//! Two single-responsibility traits keep the CQRS split clean: every command
//! handler holds the write port; the `activity_summary` query holds the read port.

use std::fmt;

/// A redacted, append-only universal-activity record — one per command turn.
///
/// Every field is on the Part-3 allowlist. There is intentionally no field for
/// message text, raw actor identity, or paths. `actor_hash` is salted and
/// non-reversible; `None` means no salt was configured (fail-safe — never a raw
/// or unsalted actor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityLogRecord {
    /// The command verb (e.g. "route", "begin", "complete", "catalog").
    pub command: String,
    /// Per-command outcome label (variant name): for route this is the route
    /// resolution outcome ("single" | "candidates" | "no_match"); for other
    /// commands a meaningful per-command label ("ok" | "error" | ...).
    pub outcome: String,
    /// The resolved playbook kind where the command has one (route/begin/
    /// complete/snapshot). Empty when the command carries no kind.
    pub artifact_kind: String,
    /// The transition's SOURCE state where the command carries one (the active
    /// step a complete/snapshot left). Empty for commands with no transition
    /// (catalog/describe/route) and for begin (no prior state). State labels are
    /// public (already in the routing/step sinks), so this is not redacted.
    pub from_state: String,
    /// The transition's DESTINATION state where the command carries one (the
    /// step begin entered, or complete/snapshot advanced to). Empty for commands
    /// with no transition (catalog/describe/route). Public, not redacted.
    pub to_state: String,
    /// Salted, non-reversible, truncated SHA-256 of the actor name. `None` when
    /// no salt is configured (fail-safe) or no actor is associated.
    pub actor_hash: Option<String>,
    /// ISO-8601 timestamp stamped by the engine at command time.
    pub at: String,
    /// The originating harness id for a `route` turn ("claude-code" | "codex" |
    /// "kiln" | "hermes" | "grok" | "opencode"), tagged by the per-harness
    /// route-turn hook. Empty for non-route commands and for route turns from an
    /// older hook (no `--source`). A public label (a harness id, not an
    /// identity), so it is not redacted.
    pub source: String,
    /// Salted, non-reversible hash of the surface conversation/session id.
    /// Missing on legacy records and explicitly omitted for checkin.
    pub conversation_hash: Option<String>,
    /// Basename/registered label of the caller's project root. Missing on
    /// legacy/unattributed records; full paths are never persisted.
    pub project_label: Option<String>,
    /// Opaque per-run artifact instance id. Missing on legacy records and
    /// commands without a playbook instance.
    pub playbook_run_id: Option<String>,
    /// The route turn's call-state classification ("no_playbook_run" |
    /// "start_opportunity" | "mid_playbook_run" | "resume"). Set only for `route` turns; `None`
    /// for non-route commands and for records written before this field existed
    /// (those fold into the `unknown` bucket). Additive — a public label, not
    /// redacted.
    pub call_state: Option<String>,
}

/// Errors surfaced by the activity-log ports. Adapter-level I/O and parse
/// failures only — callers own argument validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityLogError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for ActivityLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivityLogError::IoError { message } => write!(f, "I/O error: {}", message),
            ActivityLogError::MalformedRecord { message } => {
                write!(f, "Malformed activity-log record: {}", message)
            }
        }
    }
}

impl std::error::Error for ActivityLogError {}

/// Append-only write port. Appends one redacted record to the durable sink,
/// creating it on first write. Implementations must apply the append atomically
/// per call.
pub trait ActivityLogWritePort: Send + Sync {
    fn append_activity_log(&self, record: &ActivityLogRecord) -> Result<(), ActivityLogError>;
}

/// Read port. Returns every recorded record in append order. A missing sink
/// reads as an empty stream (no error) — usage is simply zero.
pub trait ActivityLogReadPort: Send + Sync {
    fn read_activity_log(&self) -> Result<Vec<ActivityLogRecord>, ActivityLogError>;
}
