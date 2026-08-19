//! The rendezvous record — the on-disk contract written by an engine and read
//! by clients.

use serde::{Deserialize, Serialize};

/// Schema version embedded in every [`RendezvousRecord`]. Bump when the wire
/// shape changes incompatibly; readers can branch on it.
pub const SCHEMA_VERSION: u32 = 1;

/// The contents of `<dir>/engine.json`.
///
/// JSON field names are camelCase where the design specifies them
/// (`startedAt`) and snake_case for `schema_version`; the rest map 1:1.
///
/// ```json
/// {
///   "schema_version": 1,
///   "url": "http://127.0.0.1:14312",
///   "pid": 80123,
///   "version": "0.4.1",
///   "startedAt": 1718205600
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendezvousRecord {
    /// On-disk schema version. Always [`SCHEMA_VERSION`] for records this crate
    /// writes.
    pub schema_version: u32,
    /// Full base URL the engine is reachable at, e.g. `http://127.0.0.1:14312`.
    /// Stored as a complete string (no separate host/port split) so future
    /// transports (unix sockets, alternate schemes) don't break the contract.
    pub url: String,
    /// OS process id of the engine. Advisory only — used as a stale-file hint,
    /// never as authoritative liveness (see crate docs).
    pub pid: u32,
    /// Engine/kit version string, for diagnostics.
    pub version: String,
    /// Epoch seconds at which the engine published this record.
    #[serde(rename = "startedAt")]
    pub started_at: u64,
    /// Optional backing-store identity (e.g. the engine's `db_path`). When set,
    /// a client can guard against port reuse: if the live `/health` body reports
    /// a different `db_path`, the rendezvous is a stale file pointing at a port a
    /// DIFFERENT engine now owns, and must not be trusted. Absent → no guard
    /// (back-compat with v1/v2 records, which omit the field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_path: Option<String>,
}

impl RendezvousRecord {
    /// Build a record stamped with the current [`SCHEMA_VERSION`] and the
    /// current process id. `started_at` should be epoch seconds. No `db_path`
    /// guard — use [`RendezvousRecord::with_db_path`] to add one.
    pub fn new(url: impl Into<String>, version: impl Into<String>, started_at: u64) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            url: url.into(),
            // SAFETY: getpid never fails and always fits a u32 on our targets.
            pid: std::process::id(),
            version: version.into(),
            started_at,
            db_path: None,
        }
    }

    /// Attach a backing-store identity (e.g. `db_path`) so clients can guard
    /// against port reuse via the `/health` body. Returns `self` for chaining.
    pub fn with_db_path(mut self, db_path: impl Into<String>) -> Self {
        self.db_path = Some(db_path.into());
        self
    }
}
