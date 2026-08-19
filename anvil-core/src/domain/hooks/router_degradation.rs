//! Naming WHY the router produced no verdict — the pure half of the degradation
//! contract. The engine owns the socket and delegates the judgement here.
//!
//! Before this module every non-2xx kiln reply was recorded as `transport_err`, so
//! a rotated bearer, an exhausted budget, a retired model and a genuine connect
//! failure were one label — which sends every reader hunting a network problem.
//! Measured 2026-08-16: kiln refused hours of routing calls with 403
//! `budget_ceiling_reached` while the log said `transport_err`; four wrong turns.
//!
//! Kiln answers with `{"error":{"message":…,"type":"kiln","code":"<code>"}}`, so the
//! code is preferred and the status class is the fallback. No `unknown` arm: an
//! unmodelled refusal is `refused`, which is still not a network fault.

/// The named cause of a kiln reply that carried no routing verdict. Every variant
/// is a DIFFERENT fix, which is the whole reason they are no longer one string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KilnFailure {
    /// The gateway rejected our credential (missing / rotated bearer).
    Unauthorized,
    /// A spend ceiling is reached. Nothing is broken; the budget is spent.
    BudgetExhausted,
    /// The routing model is not served — often an alias kiln advertises and the
    /// provider no longer answers for.
    ModelUnavailable,
    /// Kiln reached its upstream and the upstream failed.
    UpstreamError,
    /// A policy refusal this taxonomy does not model — named, not folded into a
    /// network fault, so it stays visibly unmodelled.
    Refused,
}

impl KilnFailure {
    /// The wire label carried on the attempt record and the delivery row.
    pub fn as_str(self) -> &'static str {
        match self {
            KilnFailure::Unauthorized => "unauthorized",
            KilnFailure::BudgetExhausted => "budget_exhausted",
            KilnFailure::ModelUnavailable => "model_unavailable",
            KilnFailure::UpstreamError => "upstream_error",
            KilnFailure::Refused => "refused",
        }
    }
}

/// Kiln's `error.code`, lowercased, when the body carries the envelope.
fn error_code(body: &str) -> Option<String> {
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    let json: serde_json::Value = serde_json::from_str(body.get(start..=end)?).ok()?;
    Some(json.pointer("/error/code")?.as_str()?.to_ascii_lowercase())
}

fn names_budget(lower: &str) -> bool {
    lower.contains("budget") || lower.contains("ceiling")
}

fn names_missing_model(lower: &str) -> bool {
    lower.contains("404") || lower.contains("not found")
}

/// Classify a non-2xx kiln reply. `status` is the status line's code; `body` is
/// whatever followed the headers (possibly empty, possibly not JSON at all). Code
/// first, status class second. Never `transport_err`: this only ever sees a reply
/// that ARRIVED, and calling an arrived reply a transport failure is the defect
/// this module exists to end.
pub fn classify_kiln_failure(status: u16, body: &str) -> KilnFailure {
    let lower = body.to_ascii_lowercase();
    match error_code(body).unwrap_or_default().as_str() {
        "unauthorized" | "invalid_api_key" | "forbidden" => return KilnFailure::Unauthorized,
        "budget_ceiling_reached" => return KilnFailure::BudgetExhausted,
        "no_target_for_model" | "no_target_for_modality" | "model_not_found" => {
            return KilnFailure::ModelUnavailable
        }
        // The live 2026-08-16 shape: `upstream_error` covers BOTH a broken upstream
        // and a model kiln no longer serves, and only the message separates them.
        // A provider 404 is a config fix, not a retry — so the message is read.
        "upstream_error" if names_missing_model(&lower) => return KilnFailure::ModelUnavailable,
        "upstream_error" => return KilnFailure::UpstreamError,
        _ => {}
    }
    match status {
        401 | 407 => KilnFailure::Unauthorized,
        402 | 403 if names_budget(&lower) => KilnFailure::BudgetExhausted,
        404 => KilnFailure::ModelUnavailable,
        s if (500..600).contains(&s) && names_missing_model(&lower) => KilnFailure::ModelUnavailable,
        s if (500..600).contains(&s) => KilnFailure::UpstreamError,
        _ => KilnFailure::Refused,
    }
}

/// The human sentence for a cause. Falls through to the raw label for the causes
/// this module does not own (`timeout`, `transport_err`, `parse_gap`) and for any
/// label a future change adds without amending it — an unfamiliar cause still
/// reaches the reader rather than being swallowed.
fn cause_phrase(cause: &str) -> &str {
    match cause {
        "unauthorized" => "kiln rejected the gateway credential",
        "budget_exhausted" => "kiln budget exhausted",
        "model_unavailable" => "the routing model is not served",
        "upstream_error" => "kiln's upstream failed",
        "refused" => "kiln refused the routing call",
        "transport_err" => "the kiln gateway was unreachable",
        "timeout" => "the kiln call timed out",
        "parse_gap" => "the router's reply was unparseable",
        other => other,
    }
}

/// The one-line notice a degraded turn writes to stdout. Visibility, not failure:
/// the fail-open contract says a hook NEVER breaks a turn, so "the lexical fallback
/// returns an error" can only mean the reader is told. The tail is two different
/// facts and says which one happened.
pub fn degradation_notice(cause: &str, degraded_to_lexical: bool) -> String {
    let tail = if degraded_to_lexical {
        "routing fell back to lexical"
    } else {
        "no playbook was suggested this turn"
    };
    format!("playbook selection degraded: {}; {}", cause_phrase(cause), tail)
}
