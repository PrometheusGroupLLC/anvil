//! Append-only durable sink for the per-turn GUIDANCE DELIVERY record.
//!
//! One row per route-turn hook process: what the hook produced, for which
//! playbook kind, in which conversation. Where the activity log records what
//! the ENGINE served, this sink records what the HOOK wrote to stdout — the
//! producer side of the suggestion, which is the left-hand leg of the
//! delivered-kind→begin join.
//!
//! ## Redaction (the R1.4 allowlist)
//!
//! A [`DeliveryLogRecord`] carries ONLY [`DELIVERY_RECORD_KEYS`]. There is
//! deliberately no raw `conversation_id` — the pre-migration writer persisted
//! one, and keeping it beside the hash would leave the raw field as the easy
//! join key and the sink in violation of the standing no-raw-identities
//! constraint. There is no `project_root` either: the never-persisted raw root
//! reduces to a basename before the record exists.
//!
//! ## One constructor, enforced by the compiler
//!
//! [`DeliveryLogRecord`] is `#[non_exhaustive]`, so no crate outside
//! `anvil-core` can build one with a struct literal. The only way in is
//! [`project_delivery_record`], which is where the three rules that carry the
//! redaction contract live: the sentinel for an absent engine hash, the
//! basename for the raw project root, and the empty guidance kind for a
//! non-delivering turn. This turns "the hook must not assemble the record
//! field by field" from a review convention into a compile error, which matters
//! because a hand-assembled literal would bypass exactly the two rules the
//! redaction depends on.
//!
//! ## The reader is tolerant, and tolerance means COUNTED
//!
//! 2,500+ rows are already on disk carrying `conversation_id` and no
//! `conversation_hash`, and the oldest carry no `resume_source` either. A
//! strict reader errors on the whole file. A lenient reader that skips what it
//! cannot parse loses the rows silently, which is worse: the join's coverage
//! number would then improve by dropping its own denominator. So every absent
//! field reads as its documented default, a row with no `conversation_hash`
//! reads as [`UNKNOWN_CONVERSATION_HASH`] flagged `pre_migration`, and a line
//! that genuinely does not parse increments `read_defects` rather than
//! disappearing. The raw id is never hashed (there is no salt in a read path,
//! and guessing one produces a disjoint keyspace with no visible cause) and
//! never reaches a record field, a report, or a defect message.
//!
//! Two single-responsibility traits keep the CQRS split clean: the hook holds
//! the write port; the `JoinCoverage` query holds the read port.

use crate::domain::telemetry_salt::UNKNOWN_CONVERSATION_HASH;
use std::fmt;

/// Every key a delivery line may carry, and no other. Exported so the absence
/// proof is "every key present is on this list" rather than a hand-listed set
/// of forbidden names: `conversation_id` is proven gone because it is not here,
/// and so is any key a future change adds without amending this constant.
pub const DELIVERY_RECORD_KEYS: &[&str] = &[
    "at",
    "source",
    "conversation_hash",
    "project_label",
    "guidance_kind",
    "engine_candidates",
    "guidance_produced",
    "guidance_bytes",
    "outcome",
    "resume_source",
    "router_cause",
];

/// A redacted, append-only delivery record — one per route-turn hook process.
///
/// `#[non_exhaustive]`: [`project_delivery_record`] is the only constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeliveryLogRecord {
    /// ISO-8601 timestamp stamped by the hook at record time.
    pub at: String,
    /// The originating harness id ("claude-code" | "codex" | ...). A public
    /// label, not an identity.
    pub source: String,
    /// Salted, non-reversible hash of the surface conversation id, taken
    /// VERBATIM from the engine's answer. [`UNKNOWN_CONVERSATION_HASH`] when
    /// the engine had no answer (unreachable) or no salt. Never a raw id and
    /// never an omitted field.
    pub conversation_hash: String,
    /// Basename of the caller's project root. Full paths are never persisted.
    pub project_label: String,
    /// The single playbook kind whose guidance this hook process wrote to
    /// stdout. Empty for a menu, for no match, and for every non-delivering
    /// path. A vocabulary term, not an identity, so it is written raw.
    ///
    /// Deliberately not named `artifact_kind`: this is a producer-side claim,
    /// not the resolved artifact's kind, and the name difference is what keeps
    /// this sink out of the all-sinks `artifact_kind` assertion.
    pub guidance_kind: String,
    /// How many candidates the engine returned for the turn.
    pub engine_candidates: u64,
    /// Whether this process wrote guidance to stdout. Producer-side: the record
    /// is written BEFORE `println!`, so it cannot witness delivery.
    pub guidance_produced: bool,
    /// Byte length of the guidance written.
    pub guidance_bytes: u64,
    /// Why the turn resolved as it did.
    pub outcome: String,
    /// Which path decided the turn, straight from the engine. Set
    /// independently of `outcome` — the two answer different questions.
    pub resume_source: String,
    /// WHY the router was absent (`budget_exhausted`, `model_unavailable`,
    /// `unauthorized`, `upstream_error`, `refused`, `transport_err`, `timeout`,
    /// `parse_gap`); empty when it answered or was never called. NOT derivable
    /// from `outcome`, which answers what the turn PRODUCED — a turn can degrade
    /// to lexical and still produce guidance, and that disagreement is the row
    /// worth counting.
    pub router_cause: String,
    /// READ-SIDE ONLY, never persisted and not on [`DELIVERY_RECORD_KEYS`].
    /// True when the row on disk carried no `conversation_hash` key at all —
    /// a row written before the migration. Such a row is unjoinable, but it is
    /// unjoinable INSIDE the denominator: the fold buckets it as
    /// `PreMigrationRow` rather than dropping it.
    ///
    /// Distinct from a post-migration row whose hash IS the sentinel: that row
    /// had an engine that could not answer, which is a different fact and a
    /// different bucket.
    pub pre_migration: bool,
}

/// What the hook actually holds at the moment it records a turn.
///
/// `project_root` is the RAW root from `std::env::current_dir()` and is never
/// persisted; `engine_conversation_hash` is the engine's answer verbatim, empty
/// when there is no answer. The hook never computes a hash: 19 `.telemetry-salt`
/// files holding 6 distinct values exist across the fleet, so a second hasher
/// produces a disjoint keyspace whose only symptom is a low coverage number
/// with no visible cause.
#[derive(Debug, Clone, Copy)]
pub struct DeliveryObservation<'a> {
    pub at: &'a str,
    pub source: &'a str,
    pub project_root: &'a str,
    pub engine_conversation_hash: &'a str,
    pub guidance_kind: &'a str,
    pub engine_candidates: u64,
    pub guidance_produced: bool,
    pub guidance_bytes: u64,
    pub outcome: &'a str,
    pub resume_source: &'a str,
    pub router_cause: &'a str,
}

/// The one basename rule. `project_label` is `basename(project_root)` and
/// nothing else; empty when there is no root or no final component.
///
/// It lives here, once, and the engine's `AnvilServer::project_label` delegates
/// to it. Two copies of a projection that must agree is the drift pattern this
/// repository has already paid for.
pub fn project_label_from_root(project_root: &str) -> String {
    std::path::Path::new(project_root)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// The whole redaction contract, in exactly one function.
///
/// Applies the sentinel rule (absent engine hash → [`UNKNOWN_CONVERSATION_HASH`]),
/// the basename rule ([`project_label_from_root`]) and the empty-guidance-kind
/// rule, and is the only constructor of a [`DeliveryLogRecord`].
pub fn project_delivery_record(obs: &DeliveryObservation) -> DeliveryLogRecord {
    let conversation_hash = if obs.engine_conversation_hash.is_empty() {
        // No engine answer, or an engine with no salt. NEVER the raw id, and
        // never an omitted field: the fold's unjoinable bucket keys off this
        // exact string, so an absent field would read as "not applicable"
        // rather than as an unjoinable row inside the denominator.
        UNKNOWN_CONVERSATION_HASH.to_string()
    } else {
        obs.engine_conversation_hash.to_string()
    };
    DeliveryLogRecord {
        at: obs.at.to_string(),
        source: obs.source.to_string(),
        conversation_hash,
        // The raw root dies here. It is a `&str` on the observation and a
        // basename on the record; there is no field for it to survive into.
        project_label: project_label_from_root(obs.project_root),
        guidance_kind: obs.guidance_kind.to_string(),
        engine_candidates: obs.engine_candidates,
        guidance_produced: obs.guidance_produced,
        guidance_bytes: obs.guidance_bytes,
        outcome: obs.outcome.to_string(),
        resume_source: obs.resume_source.to_string(),
        router_cause: obs.router_cause.to_string(),
        // A record the WRITER produces is by definition post-migration: it
        // carries a conversation_hash key. Only the reader ever sets this.
        pre_migration: false,
    }
}

/// Every row in append order, plus the count of lines that did not parse.
///
/// The defects are RETURNED, not logged, so a caller cannot report a row count
/// it did not measure. `defect_messages` carries positions only — never the
/// offending line, which on this sink still holds raw conversation ids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeliveryLogScan {
    pub records: Vec<DeliveryLogRecord>,
    pub read_defects: u64,
    pub defect_messages: Vec<String>,
}

/// Errors surfaced by the delivery-log ports. Adapter-level I/O only — an
/// unparseable ROW is a counted defect, never an error, because erroring the
/// whole file over one glued line is how 2,500 readable rows become zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryLogError {
    IoError { message: String },
}

impl fmt::Display for DeliveryLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeliveryLogError::IoError { message } => write!(f, "I/O error: {}", message),
        }
    }
}

impl std::error::Error for DeliveryLogError {}

/// Append-only write port. Appends one redacted record to the durable sink,
/// creating it on first write. Implementations must apply the append atomically
/// per call.
pub trait DeliveryLogWritePort: Send + Sync {
    fn append_delivery_log(&self, record: &DeliveryLogRecord) -> Result<(), DeliveryLogError>;
}

/// Read port. Returns every parseable record in append order plus the read
/// defects. A missing sink reads as an empty stream (no error) — a fresh hearth
/// simply has no rows.
pub trait DeliveryLogReadPort: Send + Sync {
    fn read_delivery_log(&self) -> Result<DeliveryLogScan, DeliveryLogError>;
}
