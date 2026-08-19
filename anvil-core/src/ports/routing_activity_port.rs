//! Append-only durable sink for redacted routing-activity records.
//!
//! The engine emits a `routing_decision` *tracing* event on every route
//! resolution, but that sink is stderr — not queryable. The PlaybookActivity
//! read-side needs a durable, aggregatable source of per-kind call counts, so
//! this narrow port persists ONE redacted record per resolution and reads them
//! back for aggregation.
//!
//! ## Redaction (Part-3 allowlist)
//!
//! A [`RoutingActivityRecord`] carries ONLY the allowlisted fields — the
//! resolved playbook `kind`, the resolver `outcome` (a variant-name label, not
//! free text), an ISO-8601 `at` timestamp, and optional hashed/labelled
//! correlation keys. It NEVER carries the surface message text, paths,
//! identities, confidence, or any other raw input. The write is a typed event +
//! variant-name log, never message text.
//!
//! Two single-responsibility traits keep the CQRS split clean: the route path
//! holds the write port; the PlaybookActivity query holds the read port.

use std::fmt;

/// A redacted, append-only routing-activity record.
///
/// Every field is on the Part-3 allowlist. There is intentionally no field for
/// message text, surface, actor identity, or paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingActivityRecord {
    /// The resolved/selected playbook kind. Empty when no kind resolved.
    pub kind: String,
    /// Resolver outcome label (variant name): "single" | "candidates" | "no_match".
    pub outcome: String,
    /// ISO-8601 timestamp stamped by the engine at route time.
    pub at: String,
    /// Salted, non-reversible hash of the surface conversation/session id.
    /// Missing on legacy/unjoinable records; raw ids are never persisted.
    pub conversation_hash: Option<String>,
    /// Basename/registered label of the caller's project root. Missing on
    /// legacy/unattributed records; full paths are never persisted.
    pub project_label: Option<String>,
}

/// Errors surfaced by the routing-activity ports. Adapter-level I/O and parse
/// failures only — callers own argument validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingActivityError {
    IoError { message: String },
    MalformedRecord { message: String },
}

impl fmt::Display for RoutingActivityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoutingActivityError::IoError { message } => write!(f, "I/O error: {}", message),
            RoutingActivityError::MalformedRecord { message } => {
                write!(f, "Malformed routing-activity record: {}", message)
            }
        }
    }
}

impl std::error::Error for RoutingActivityError {}

/// Append-only write port. Appends one redacted record to the durable sink,
/// creating it on first write. Implementations must apply the append atomically
/// per call.
pub trait RoutingActivityWritePort: Send + Sync {
    fn append_routing_activity(
        &self,
        record: &RoutingActivityRecord,
    ) -> Result<(), RoutingActivityError>;
}

/// Read port. Returns every recorded record in append order. A missing sink
/// reads as an empty stream (no error) — call counts are simply zero.
pub trait RoutingActivityReadPort: Send + Sync {
    fn read_routing_activity(&self) -> Result<Vec<RoutingActivityRecord>, RoutingActivityError>;
}
