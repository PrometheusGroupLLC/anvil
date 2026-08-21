//! `anvil-engine::telemetry` — anvil's **content-free** runtime telemetry,
//! emitted through the cross-kit [`crate::kit_telemetry`] contract as kit_id
//! **`"anvil"`**.
//!
//! The fleet-telemetry loop (emitter → broker → relay → Cloud Logging → Temper)
//! sweeps every kit's `~/.<kit>/<kit>-telemetry.jsonl`. This module makes anvil a
//! first-class producer: the routing plane records a small, content-free set of
//! events to `~/.anvil/anvil-telemetry.jsonl` (the same `~/.anvil` rendezvous dir
//! the engine.json record and `router.json` already live under), which the
//! emitter then rolls up alongside the other kits.
//!
//! ## Privacy invariant (do NOT violate)
//!
//! Every field name AND every categorical label is a **safe token**
//! (`[A-Za-z0-9_.:-]`, ≤64) — the underlying [`crate::kit_telemetry`] module
//! structurally rejects anything else. We emit only enumerable categorical labels
//! (`outcome`, `target_tier`, `reason`, `close_reason`,
//! `dominant_fallback_used`) plus numeric measures (`confidence`, `latency_ms`,
//! `candidate_count`, `session_duration_ms`, …). NO message text, NO recent
//! context, NO conversation ids, NO paths ever ride through here. Any identifier
//! (an actor / space / tenant) reaches a row ONLY via [`Telemetry::hash_id`]
//! (salted SHA-256, truncated) — never the raw value. There is no `Text` metric
//! variant, so free text cannot be recorded even by mistake.
//!
//! ## Local-first, single consent gate (contract invariant 4)
//!
//! Anvil records to `~/.anvil/anvil-telemetry.jsonl` **unconditionally** — there
//! is NO anvil-owned opt-in. Consent lives in exactly ONE place: Foundry's
//! emitter (`~/.foundry/telemetry-consent.json`), which decides whether the
//! locally-recorded rollups ever *egress*. A kit ALWAYS records locally; whether
//! that leaves the machine is Foundry's decision, not anvil's. This matches the
//! other kits (Lore / Kiln / Temper / Forge / Accumulate), which all record
//! unconditionally and defer egress to the emitter.
//!
//! Recording stays strictly best-effort: a recorder that can't be built (e.g.
//! `$HOME` unset) simply no-ops. The append target is `~/.anvil` in production;
//! a test-only `ANVIL_TELEMETRY_DIR` override redirects it to a throwaway dir so
//! hermetic Brine scenarios never touch the real `~/.anvil` (mirroring Lore's
//! `LORE_TELEMETRY_DIR` / Forge's `FORGE_TELEMETRY_DIR`). The dir is redirected
//! in tests; recording itself is always ON.
//!
//! ## Never panics, never blocks routing
//!
//! Every `record_*` entry point is strictly best-effort: it resolves a
//! process-wide [`Telemetry`] lazily (a single `~/.anvil` append target), and any
//! failure (HOME unset, salt error, unsafe token, I/O) is logged at
//! `debug`/`warn` and swallowed. A record call is a cheap jsonl append and must
//! never delay or fail the routing decision it annotates.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// Re-export the pieces the Brine steps (and any downstream reader) need, so the
// step crate can drive + validate egress through this module rather than
// reaching into `crate::kit_telemetry` itself.
pub use crate::kit_telemetry::{
    validate_envelope, Envelope, MetricValue, Telemetry, TelemetryConfig, TelemetryError, Window,
};

/// The kit id anvil reports under (a safe kebab-case token).
pub const KIT_ID: &str = "anvil";

// ── Event kinds (anvil's own routing-plane taxonomy) ─────────────────────────

/// One routing decision: `routed` / `abstained` / `fallback`, a target tier, and
/// the call latency. `fleet.anvil.by_outcome.*`, `fleet.anvil.by_target_tier.*`,
/// `fleet.anvil.latency_ms.*`.
pub const EVENT_ROUTE_DECISION: &str = "route_decision";
/// A genuine abstention (no playbook fit). `fleet.anvil.by_reason.*`.
pub const EVENT_ABSTENTION: &str = "abstention";
/// A semantic-route ranking pass. `fleet.anvil.candidate_count.*`,
/// `fleet.anvil.by_dominant_fallback_used.*`.
pub const EVENT_SEMANTIC_RANK: &str = "semantic_rank";
/// A ws-bridge session lifecycle sample. `fleet.anvil.by_close_reason.*`,
/// `fleet.anvil.session_duration_ms.*`.
pub const EVENT_WS_SESSION: &str = "ws_session";

// ── Categorical field names (roll up to `by_<field>`) ────────────────────────

pub const F_OUTCOME: &str = "outcome";
pub const F_TARGET_TIER: &str = "target_tier";
pub const F_REASON: &str = "reason";
pub const F_DOMINANT_FALLBACK_USED: &str = "dominant_fallback_used";
pub const F_CLOSE_REASON: &str = "close_reason";
/// The salted-hash actor bucket (never a raw id). No declared domain — a hash is
/// not an enumerable label, only a safe token.
pub const F_ACTOR: &str = "actor";

// ── Numeric field names (roll up to `{count,mean,p50,p95}`) ──────────────────

pub const N_CONFIDENCE: &str = "confidence";
pub const N_LATENCY_MS: &str = "latency_ms";
pub const N_SESSIONS: &str = "sessions";
pub const N_SESSION_DURATION_MS: &str = "session_duration_ms";
pub const N_TOP_SCORE: &str = "top_score";
pub const N_CANDIDATE_COUNT: &str = "candidate_count";

// ── Closed categorical domains (undeclared labels rejected at record time) ────

/// Route-decision outcomes.
pub const OUTCOMES: [&str; 3] = ["routed", "abstained", "fallback"];
/// Router tiers (single-tier backend: kiln, else none/unknown).
pub const TARGET_TIERS: [&str; 3] = ["kiln", "none", "unknown"];
/// Abstention reasons.
pub const REASONS: [&str; 4] = ["no_match", "router_abstain", "fallback", "no_candidate"];
/// A yes/no expressed categorically.
pub const YES_NO: [&str; 2] = ["yes", "no"];
/// ws-bridge close reasons.
pub const CLOSE_REASONS: [&str; 3] = ["peer_close", "transport_err", "server_close"];

// ── Configuration + recorder construction ────────────────────────────────────

/// Build the `"anvil"` telemetry config targeting `<dir>/anvil-telemetry.jsonl`
/// with all closed categorical domains declared. Shared by the process-wide
/// recorder and the hermetic Brine steps so both enforce the SAME contract.
pub fn build_config(dir: PathBuf) -> TelemetryConfig {
    TelemetryConfig::new(KIT_ID, env!("CARGO_PKG_VERSION"), dir)
        .with_domain(F_OUTCOME, OUTCOMES)
        .with_domain(F_TARGET_TIER, TARGET_TIERS)
        .with_domain(F_REASON, REASONS)
        .with_domain(F_DOMINANT_FALLBACK_USED, YES_NO)
        .with_domain(F_CLOSE_REASON, CLOSE_REASONS)
}

/// Build a recorder rooted at `dir` (best-effort). Returns `None` (logged) rather
/// than propagating, so a telemetry init failure can never take down routing.
/// Exposed so hermetic tests can drive egress against a throwaway dir.
pub fn build_recorder_at(dir: &Path) -> Option<Telemetry> {
    match Telemetry::new(build_config(dir.to_path_buf())) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(
                target: "anvil::telemetry",
                error = %e,
                "anvil telemetry disabled (recorder init failed)"
            );
            None
        }
    }
}

/// Resolve `$HOME` (never hardcodes a path). `None` when unset.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Resolve the telemetry append dir. A test-only `ANVIL_TELEMETRY_DIR` override
/// (mirroring Lore's `LORE_TELEMETRY_DIR` / Forge's `FORGE_TELEMETRY_DIR`)
/// redirects the target to a throwaway dir so hermetic Brine scenarios never
/// write to the real `~/.anvil`. In production the override is unset and the
/// target is `~/.anvil` (the rendezvous dir the engine.json record and
/// router.json already live under). `None` only when NEITHER the override nor
/// `$HOME` resolves — recording is then a silent no-op.
fn telemetry_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ANVIL_TELEMETRY_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    home_dir().map(|h| h.join(".anvil"))
}

/// Process-wide, lazily-initialized recorder for kit_id `"anvil"`. `None` when the
/// recorder could not be built (e.g. `$HOME` unset); recording is then a silent
/// no-op.
static ANVIL_TELEMETRY: OnceLock<Option<Telemetry>> = OnceLock::new();

/// Resolve (once) the process-wide anvil recorder targeting
/// `~/.anvil/anvil-telemetry.jsonl`.
fn recorder() -> Option<&'static Telemetry> {
    ANVIL_TELEMETRY
        .get_or_init(|| {
            let Some(dir) = telemetry_dir() else {
                tracing::debug!(
                    target: "anvil::telemetry",
                    "anvil telemetry disabled (could not resolve ~/.anvil base)"
                );
                return None;
            };
            build_recorder_at(&dir)
        })
        .as_ref()
}

// ── Hermetic emitters (take an explicit recorder; return the Result) ──────────
// These carry the field taxonomy. The best-effort `record_*` entry points below
// resolve the global recorder and swallow the Result; tests drive these directly
// against a throwaway dir.

/// Emit a `route_decision` row. `actor`, when present, is hashed with
/// [`Telemetry::hash_id`] before it ever reaches a field — the raw id never lands
/// on disk. `confidence` / `latency_ms` are omitted when absent (honest rollups).
pub fn emit_route_decision(
    tel: &Telemetry,
    actor: Option<&str>,
    outcome: &str,
    target_tier: &str,
    confidence: Option<f64>,
    latency_ms: Option<f64>,
) -> Result<(), TelemetryError> {
    let mut fields: Vec<(&str, MetricValue)> = vec![
        (F_OUTCOME, MetricValue::Cat(outcome.to_string())),
        (F_TARGET_TIER, MetricValue::Cat(target_tier.to_string())),
    ];
    if let Some(actor) = actor {
        // NEVER raw: the only sanctioned way an identifier appears in telemetry.
        fields.push((F_ACTOR, MetricValue::Cat(tel.hash_id(actor))));
    }
    if let Some(c) = confidence {
        fields.push((N_CONFIDENCE, MetricValue::Num(c)));
    }
    if let Some(ms) = latency_ms {
        fields.push((N_LATENCY_MS, MetricValue::Num(ms)));
    }
    tel.record(EVENT_ROUTE_DECISION, &fields)
}

/// Emit an `abstention` row carrying only a closed categorical `reason`.
pub fn emit_abstention(tel: &Telemetry, reason: &str) -> Result<(), TelemetryError> {
    tel.record(
        EVENT_ABSTENTION,
        &[(F_REASON, MetricValue::Cat(reason.to_string()))],
    )
}

/// Emit a `semantic_rank` row. `top_score` is omitted when the pass computed none.
pub fn emit_semantic_rank(
    tel: &Telemetry,
    top_score: Option<f64>,
    candidate_count: u64,
    dominant_fallback_used: &str,
) -> Result<(), TelemetryError> {
    let mut fields: Vec<(&str, MetricValue)> = vec![
        (N_CANDIDATE_COUNT, MetricValue::Count(candidate_count)),
        (
            F_DOMINANT_FALLBACK_USED,
            MetricValue::Cat(dominant_fallback_used.to_string()),
        ),
    ];
    if let Some(s) = top_score {
        fields.push((N_TOP_SCORE, MetricValue::Num(s)));
    }
    tel.record(EVENT_SEMANTIC_RANK, &fields)
}

/// Emit a `ws_session` row: one session, its duration, and the close reason.
pub fn emit_ws_session(
    tel: &Telemetry,
    session_duration_ms: f64,
    close_reason: &str,
) -> Result<(), TelemetryError> {
    tel.record(
        EVENT_WS_SESSION,
        &[
            (N_SESSIONS, MetricValue::Count(1)),
            (N_SESSION_DURATION_MS, MetricValue::Num(session_duration_ms)),
            (F_CLOSE_REASON, MetricValue::Cat(close_reason.to_string())),
        ],
    )
}

// ── Best-effort call-site entry points (always record; never fail routing) ────
// Recording is unconditional — there is no anvil-owned opt-in. A missing recorder
// (e.g. `$HOME` unset) makes each call a silent no-op; egress consent is Foundry's
// emitter decision, applied downstream, not gated here.

/// Record a routing decision, best-effort. No-op only if the recorder can't be
/// built (e.g. `$HOME` unset); egress is Foundry's decision, not gated here.
pub fn record_route_decision(
    actor: Option<&str>,
    outcome: &str,
    target_tier: &str,
    confidence: Option<f64>,
    latency_ms: Option<f64>,
) {
    let Some(tel) = recorder() else { return };
    if let Err(e) = emit_route_decision(tel, actor, outcome, target_tier, confidence, latency_ms) {
        tracing::debug!(target: "anvil::telemetry", error = %e, "route_decision record dropped (non-fatal)");
    }
}

/// Record a genuine abstention, best-effort. No-op only if the recorder can't be built.
pub fn record_abstention(reason: &str) {
    let Some(tel) = recorder() else { return };
    if let Err(e) = emit_abstention(tel, reason) {
        tracing::debug!(target: "anvil::telemetry", error = %e, "abstention record dropped (non-fatal)");
    }
}

/// Record a semantic-rank pass, best-effort. No-op only if the recorder can't be built.
pub fn record_semantic_rank(top_score: Option<f64>, candidate_count: u64, dominant_fallback_used: &str) {
    let Some(tel) = recorder() else { return };
    if let Err(e) = emit_semantic_rank(tel, top_score, candidate_count, dominant_fallback_used) {
        tracing::debug!(target: "anvil::telemetry", error = %e, "semantic_rank record dropped (non-fatal)");
    }
}

/// Record a ws-bridge session, best-effort. No-op only if the recorder can't be built.
pub fn record_ws_session(session_duration_ms: f64, close_reason: &str) {
    let Some(tel) = recorder() else { return };
    if let Err(e) = emit_ws_session(tel, session_duration_ms, close_reason) {
        tracing::debug!(target: "anvil::telemetry", error = %e, "ws_session record dropped (non-fatal)");
    }
}
