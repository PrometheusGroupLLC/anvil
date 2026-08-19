//! Standard telemetry taxonomy — the **shared vocabulary** every Foundry
//! producer uses so the fleet's rollups are ONE consistent, comparable shape.
//!
//! The [`Envelope`](crate::Envelope) shape is generic (categorical → `by_<field>`
//! buckets, numeric → `{count,mean,p50,p95}`), which is exactly what lets a kit
//! emit anything. That freedom is also how shapes DRIFT: two kits counting the
//! same thing under different field names produce series that can't be compared.
//! This module removes the freedom where it hurts — it pins the event kinds,
//! field names, and closed categorical domains that cross every source
//! (reliability AND product usage) to named constants, so
//! `fleet.<source>.by_status.ok`, `fleet.<source>.latency_ms.p95`, and
//! `fleet.<source>.by_page.<name>` mean the same thing everywhere.
//!
//! Every value here is a content-free safe token or a numeric measure — never
//! free text, a title, a name, a path, or any PII. Product usage is expressed as
//! *counts*, *durations*, and *categorical buckets* only (how many conversations,
//! how many recording seconds, which page — never what was said).
//!
//! A kit still uses [`Telemetry::record`](crate::Telemetry::record) for its own
//! domain-specific events; these helpers cover the cross-cutting ones so the
//! common denominator is identical fleet-wide.

use crate::MetricValue;

// ── Event kinds (the `event_kind` arg to `Telemetry::record`) ─────────────────

/// A process/component came up or went down (reliability).
pub const EVENT_LIFECYCLE: &str = "lifecycle";
/// One handled request / operation, with a status (reliability).
pub const EVENT_REQUEST: &str = "request";
/// A categorized failure (reliability).
pub const EVENT_ERROR: &str = "error";
/// A periodic health gauge sample (reliability).
pub const EVENT_UPTIME: &str = "uptime";
/// A screen/route was viewed (product).
pub const EVENT_PAGE: &str = "page";
/// A user took an action (product).
pub const EVENT_ACTION: &str = "action";
/// An intelligent artifact was generated — suggestion, recap, title, card (product).
pub const EVENT_GENERATION: &str = "generation";
/// A bounded activity with a duration — e.g. a recording session (product).
pub const EVENT_SESSION: &str = "session";
/// One chat / model turn (product).
pub const EVENT_CHAT: &str = "chat";
/// A conversation lifecycle event (product).
pub const EVENT_CONVERSATION: &str = "conversation";

// ── Standard categorical field names (roll up to `by_<field>`) ────────────────

pub const F_ACTION: &str = "action";
pub const F_COMPONENT: &str = "component";
pub const F_STATUS: &str = "status";
pub const F_OP: &str = "op";
pub const F_KIND: &str = "kind";
pub const F_PAGE: &str = "page";
pub const F_MODEL: &str = "model";
/// Which UI/engine surface the event is about (e.g. `chat`, `recording`, `engine`).
pub const F_SURFACE: &str = "surface";
/// A yes/no outcome expressed categorically, e.g. `accepted`/`dismissed`.
pub const F_OUTCOME: &str = "outcome";

// ── Standard numeric field names (roll up to `{count,mean,p50,p95}`) ──────────

/// Latency or short duration in **milliseconds**.
pub const N_MS: &str = "ms";
/// A longer duration in **seconds** (recording length, conversation length).
pub const N_DURATION_SEC: &str = "duration_sec";
/// A per-event magnitude/count (e.g. tokens, segments, messages in a turn).
pub const N_COUNT: &str = "n";
/// A byte size.
pub const N_BYTES: &str = "bytes";

// ── Standard closed categorical domains ───────────────────────────────────────
// Declare these via `TelemetryConfig::with_domain` so an undeclared label is
// rejected at record time — the domains stay closed and comparable fleet-wide.

/// Lifecycle actions.
pub const ACTIONS: [&str; 5] = ["start", "stop", "restart", "crash", "respawn"];
/// Supervised components.
pub const COMPONENTS: [&str; 5] = ["engine", "frontend", "ui", "mcp", "broker"];
/// Request/operation outcomes.
pub const STATUSES: [&str; 4] = ["ok", "error", "timeout", "rejected"];

// ── Constructors for the common cross-cutting events ──────────────────────────
// Each returns `(event_kind, fields)` ready for `Telemetry::record_event`.

/// A handled operation with an outcome and optional latency.
/// `fleet.<source>.by_status.<status>` + `fleet.<source>.by_op.<op>` (+ `.ms.p95`).
pub fn request(op: &str, status: &str, ms: Option<f64>) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    let mut f = vec![
        (F_OP, MetricValue::Cat(op.to_string())),
        (F_STATUS, MetricValue::Cat(status.to_string())),
    ];
    if let Some(ms) = ms {
        f.push((N_MS, MetricValue::Num(ms)));
    }
    (EVENT_REQUEST, f)
}

/// A categorized failure. `fleet.<source>.by_kind.<kind>`.
pub fn error(kind: &str) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    (EVENT_ERROR, vec![(F_KIND, MetricValue::Cat(kind.to_string()))])
}

/// A screen/route view. `fleet.<source>.by_page.<name>`.
pub fn page(name: &str) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    (EVENT_PAGE, vec![(F_PAGE, MetricValue::Cat(name.to_string()))])
}

/// A user action, optionally with an outcome. `fleet.<source>.by_action.<kind>`.
pub fn action(kind: &str, outcome: Option<&str>) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    let mut f = vec![(F_ACTION, MetricValue::Cat(kind.to_string()))];
    if let Some(o) = outcome {
        f.push((F_OUTCOME, MetricValue::Cat(o.to_string())));
    }
    (EVENT_ACTION, f)
}

/// An intelligent artifact generated. `fleet.<source>.by_kind.<kind>`.
pub fn generation(kind: &str) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    (EVENT_GENERATION, vec![(F_KIND, MetricValue::Cat(kind.to_string()))])
}

/// A bounded activity with a duration (seconds). `fleet.<source>.duration_sec.*`
/// + `fleet.<source>.by_kind.<kind>`.
pub fn session(kind: &str, duration_sec: f64) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    (
        EVENT_SESSION,
        vec![
            (F_KIND, MetricValue::Cat(kind.to_string())),
            (N_DURATION_SEC, MetricValue::Num(duration_sec)),
        ],
    )
}

/// One chat/model turn with a model bucket and latency (ms).
/// `fleet.<source>.by_model.<model>` + `fleet.<source>.ms.p95`.
pub fn chat(model: &str, ms: f64) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    (
        EVENT_CHAT,
        vec![
            (F_MODEL, MetricValue::Cat(model.to_string())),
            (N_MS, MetricValue::Num(ms)),
        ],
    )
}

/// A conversation lifecycle event with an optional length (seconds).
/// `fleet.<source>.by_kind.<kind>` (+ `.duration_sec.*`).
pub fn conversation(kind: &str, duration_sec: Option<f64>) -> (&'static str, Vec<(&'static str, MetricValue)>) {
    let mut f = vec![(F_KIND, MetricValue::Cat(kind.to_string()))];
    if let Some(d) = duration_sec {
        f.push((N_DURATION_SEC, MetricValue::Num(d)));
    }
    (EVENT_CONVERSATION, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_use_pinned_kinds_and_fields() {
        let (k, f) = request("ingest", "ok", Some(12.0));
        assert_eq!(k, "request");
        assert_eq!(f[0].0, "op");
        assert_eq!(f[1].0, "status");
        assert_eq!(f[2].0, "ms");

        let (k, f) = page("recording");
        assert_eq!(k, "page");
        assert_eq!(f[0].0, "page");
        assert!(matches!(&f[0].1, MetricValue::Cat(v) if v == "recording"));

        let (k, f) = session("recording", 90.0);
        assert_eq!(k, "session");
        assert_eq!(f[1].0, "duration_sec");

        let (k, _f) = conversation("ended", Some(300.0));
        assert_eq!(k, "conversation");
    }
}
