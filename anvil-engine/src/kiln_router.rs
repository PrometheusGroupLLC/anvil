//! The Kiln router client for the HYBRID ROUTER lever — the I/O half of the
//! hook-layer narrowing. Given the user message + the engine-granted access-scoped
//! candidate set, it asks a router model to commit to exactly one candidate or
//! ABSTAIN, raising routing PRECISION (validated offline at 0.72 hybrid vs 0.37
//! lexical).
//!
//! The engine stays LLM-free; this narrowing happens ENTIRELY in the hook binary
//! so the engine never depends on a model. The pure prompt build, reply parse,
//! and Pick/Abstain/Fallback mapping live in `anvil-core` route_turn (testable);
//! this module is only the time-boxed HTTP call + the fail-open envelope.
//!
//! ## Single-tier backend: Kiln → Fireworks (the ONLY path)
//! [`route`] makes exactly ONE call, under one bounded wall-clock cap, to the local
//! Kiln gateway, which routes to Fireworks. There is NO fallback tier and NO retry:
//! reliability for a flaky Fireworks is KILN's responsibility, not the router's.
//!
//! The router speaks PLAINTEXT HTTP/1.1 to loopback (the kiln gateway); kiln-serve
//! does the TLS/WAN hop to Fireworks, so anvil grows no HTTP-client/TLS dependency.
//! It presents the kiln GATEWAY-ACCESS shared bearer (never any provider key — see
//! key custody) plus the `x-kiln-*` steering headers.
//!
//! A parseable Pick/Abstain becomes the verdict; a timeout / transport error /
//! unparseable reply FAILS OPEN to [`RouterVerdict::Fallback`]. A single per-call
//! telemetry record ([`RouterAttempt`]) captures the outcome + latency so the hook
//! can log it.
//!
//! ## Key custody
//! anvil reads NO provider (Fireworks) key — provider auth lives entirely inside
//! kiln-serve. The ONLY secret anvil may present is the kiln gateway shared bearer,
//! resolved solely from `ANVIL_KILN_TOKEN` env → the file at `ANVIL_KILN_TOKEN_FILE`
//! (default `~/.foundry/run/shared-secrets/kiln-auth-token`) → none.
//!
//! ## Fail-open contract
//! The Kiln gateway unreachable, slow (past the time-box), or returning an
//! unparseable reply → [`RouterVerdict::Fallback`]. The hook NEVER blocks or errors
//! on the turn; the live hook folds `Fallback` into `NoMatch`, so routing simply does
//! not fire that turn and the turn proceeds unharmed.
//!
//! The HTTP request is a hand-rolled HTTP/1.1 POST over a raw `tokio` TCP socket to
//! loopback — no new HTTP-client dependency, no TLS (the local kiln gateway is
//! plaintext loopback).

use anvil_core::domain::hooks::route_turn::{
    build_router_prompt, parse_router_decision, CandidateBrief, InProgressSignal, RouterVerdict,
};
use anvil_core::domain::hooks::router_degradation::{classify_kiln_failure, KilnFailure};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The loopback host the router dials (the local kiln gateway).
pub const KILN_HOST: &str = "127.0.0.1";
/// Kiln gateway default port when neither `ANVIL_KILN_PORT` nor the
/// `kit-ports.json["kiln-kit"]` rendezvous resolves — the `:8470` stopgap bridge.
pub const KILN_DEFAULT_PORT: u16 = 8470;
/// Default routing model (Fireworks via kiln), overridable with `ANVIL_ROUTER_MODEL`
/// or the `model` key of `~/.anvil/router.json`.
pub const KILN_DEFAULT_MODEL: &str = "accounts/fireworks/models/deepseek-v4-flash";
/// Default bounded cap on the single Kiln round trip (connect + request + read). Past
/// this we return `Fallback` — the turn must never stall on a slow model. This is a
/// CAP, not a fixed cost: a fast model returns in well under it and the turn proceeds
/// immediately. Sized (12s) to give headroom for the currently-credentialed Kiln
/// models, which are all REASONING models measured at 5.5–13s (deepseek-v4-flash
/// ~5.7s, glm-5p2 ~13s) — 5s truncated them → Fallback → lexical over-match. Once a
/// fast NON-reasoning small model is on the router lane (Fireworks/Groq llama-3.1-8b,
/// sub-1s), the effective wait shrinks back to ~1s and this cap only bounds pathological
/// stalls. Override per-surface with `ANVIL_KILN_TIMEOUT_MS` (raise it further for the
/// measurement RPC where latency doesn't matter; keep it tight once routing is fast).
pub const KILN_TIMEOUT_MS: u64 = 12000;

/// Default kiln org header value (`x-kiln-org`), overridable with `ANVIL_KILN_ORG`.
const DEFAULT_KILN_ORG: &str = "local_only";
/// Path (relative to `$HOME`) of the Foundry-materialized kiln gateway shared
/// secret — the default when `ANVIL_KILN_TOKEN_FILE` is unset.
const DEFAULT_TOKEN_FILE_REL: &str = ".foundry/run/shared-secrets/kiln-auth-token";
/// Path (relative to `$HOME`) of the Foundry kit-port rendezvous file.
const KIT_PORTS_REL: &str = ".foundry/run/kit-ports.json";
/// Path (relative to `$HOME`) of the durable router config file — the
/// harness-agnostic knob source. ENV overrides it; it overrides the built-in
/// default. Overridable wholesale with `ANVIL_ROUTER_CONFIG_FILE` (used to keep tests
/// hermetic). Shape: `{"model":"…","enabled":"on"|"off","target":"…"}`.
const ROUTER_CONFIG_REL: &str = ".anvil/router.json";
/// The OpenAI-compatible chat-completions path the router POSTs to.
const CHAT_PATH: &str = "/v1/chat/completions";
/// Cap on the assistant reply we read — the router emits a tiny JSON object;
/// anything past this is noise we never need to parse.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Effective round-trip cap for the single Kiln call: `ANVIL_KILN_TIMEOUT_MS` if set +
/// parseable, else [`KILN_TIMEOUT_MS`]. Fails safe to the default on any parse gap.
fn timeout_ms() -> u64 {
    std::env::var("ANVIL_KILN_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&ms| ms > 0)
        .unwrap_or(KILN_TIMEOUT_MS)
}

/// The single call target the router dials: the Kiln gateway with its model, `x-kiln-*`
/// steering headers, and the gateway bearer.
struct KilnCall {
    host: String,
    port: u16,
    path: String,
    model: String,
    /// The `x-kiln-*` steering headers.
    headers: Vec<(String, String)>,
    /// The kiln GATEWAY bearer, when resolvable. NEVER a provider key.
    bearer: Option<String>,
}

/// The outcome of the one Kiln attempt, distinguishing the telemetry-relevant reasons
/// it did or did not produce a verdict.
enum Attempt {
    /// A parseable Pick / Abstain, with `usage.total_tokens` when the reply carried it.
    Verdict(RouterVerdict, u64),
    /// Kiln ANSWERED with a non-2xx, classified by cause. Never `TransportErr`:
    /// a reply that arrived is not a network fault, and calling it one is what
    /// sent four consecutive diagnoses after the wrong problem on 2026-08-16.
    Failed(KilnFailure),
    /// Connect / write / read / framing failure — no HTTP reply arrived at all.
    TransportErr,
    /// A 2xx reply whose body carried no parseable router decision → fail open.
    ParseGap,
}

/// The single call's attempt telemetry, folded into [`RouteOutcome`] so the hook can
/// log how the call fared and its latency. Additive/observability only — not persisted
/// into any on-disk schema.
#[derive(Debug, Clone, Serialize)]
pub struct RouterAttempt {
    /// Always `"kiln"` — the single backend the router dials.
    pub tier: String,
    /// WHICH of the two calls this was: `"engine"` (the Route RPC, carrying the full
    /// granted candidate set) or `"hook"` (the route-turn binary, carrying the
    /// transcript tail and in-progress signal).
    ///
    /// Anvil dials Kiln TWICE per turn from two different places with two different
    /// inputs. Until this field existed the telemetry could not tell them apart, so
    /// "which leg is slow" and "what does the second call actually cost" were
    /// unanswerable — and the standing argument about collapsing them was being had
    /// on inference rather than measurement.
    pub leg: String,
    /// `"hit"` | `"abstain"` | `"timeout"` | `"transport_err"` | `"parse_gap"`, or
    /// the cause kiln itself gave (`"budget_exhausted"` / `"model_unavailable"` /
    /// `"unauthorized"` / `"upstream_error"` / `"refused"`). One field, not two: a
    /// second that always duplicates it is two names for one fact, and the verdict
    /// bucket stays derivable — `hit`/`abstain` answered, everything else did not.
    pub outcome: String,
    /// Wall-clock latency of the attempt in milliseconds.
    pub elapsed_ms: u64,
    /// `usage.total_tokens` from the reply, 0 when the call produced no usable reply.
    /// Cost per leg is the whole point: a leg that is cheap and fast is not the one
    /// to cut.
    pub total_tokens: u64,
}

/// The router's verdict plus the per-call telemetry that produced it.
#[derive(Debug, Clone)]
pub struct RouteOutcome {
    /// The PURE verdict the caller folds into the outcome (fail-open on `Fallback`).
    pub verdict: RouterVerdict,
    /// `Some("kiln")` when the Kiln call produced the verdict, or `None` when the call
    /// missed / was disabled (fail-open).
    pub served_by: Option<String>,
    /// The per-call telemetry (0 or 1 attempts: none when routing is disabled).
    pub attempts: Vec<RouterAttempt>,
}

impl RouteOutcome {
    fn fallback(attempts: Vec<RouterAttempt>) -> Self {
        RouteOutcome {
            verdict: RouterVerdict::Fallback,
            served_by: None,
            attempts,
        }
    }
}

/// Ask the Kiln router to select from granted candidates with ONE Kiln→Fireworks call.
/// Returns the PURE [`RouterVerdict`] the caller folds into the outcome, plus the
/// per-call telemetry. ALWAYS fails open: the call missing / timing out / unparseable —
/// or routing disabled — → [`RouterVerdict::Fallback`]. There is NO fallback backend.
///
/// context_aware_routing: `recent_context` (a compact digest of prior turns) and
/// `in_progress` (the transcript-grounded in-progress playbook signal) are woven
/// into the router prompt. Both degrade to empty / `None` when the harness supplies
/// no transcript, leaving routing unchanged.
pub fn route(
    message: &str,
    recent_context: &str,
    in_progress: &InProgressSignal,
    candidates: &[CandidateBrief],
    leg: &str,
) -> RouteOutcome {
    // Kill switch: when routing is disabled, make no call and fail open silently.
    if !router_enabled() {
        return RouteOutcome::fallback(Vec::new());
    }

    let Some(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
    else {
        return RouteOutcome::fallback(Vec::new());
    };

    let budget = Duration::from_millis(timeout_ms());
    let call = build_call();

    let message = message.to_string();
    let recent_context = recent_context.to_string();
    let in_progress = in_progress.clone();
    let candidates = candidates.to_vec();

    rt.block_on(async move {
        let body = request_body(
            &call.model,
            &message,
            &recent_context,
            &in_progress,
            &candidates,
        );
        let started = Instant::now();
        let result = tokio::time::timeout(budget, attempt_call(&call, &body)).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;

        let (outcome_label, verdict, total_tokens) = match result {
            Ok(Attempt::Verdict(verdict, tokens)) => {
                let label = match &verdict {
                    RouterVerdict::Abstain => "abstain",
                    _ => "hit",
                };
                (label, Some(verdict), tokens)
            }
            Ok(Attempt::Failed(cause)) => (cause.as_str(), None, 0),
            Ok(Attempt::TransportErr) => ("transport_err", None, 0),
            Ok(Attempt::ParseGap) => ("parse_gap", None, 0),
            Err(_) => ("timeout", None, 0),
        };

        let attempts = vec![RouterAttempt {
            tier: "kiln".to_string(),
            leg: leg.to_string(),
            outcome: outcome_label.to_string(),
            elapsed_ms,
            total_tokens,
        }];

        match verdict {
            Some(verdict) => RouteOutcome {
                verdict,
                served_by: Some("kiln".to_string()),
                attempts,
            },
            None => RouteOutcome::fallback(attempts),
        }
    })
}

/// The kill switch for routing. Resolved with precedence ENV > `~/.anvil/router.json`
/// `.enabled` > default ON. Routing is on unless the resolved value is `off` / `false`
/// / `0`, so the default (no config) is byte-for-byte "routing fires".
fn router_enabled() -> bool {
    match router_config("ANVIL_ROUTER_ENABLED", "enabled") {
        Some(v) => {
            let v = v.trim();
            !(v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("false") || v == "0")
        }
        None => true,
    }
}

/// Read a non-empty, trimmed env var, else `None`.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Resolve a router knob with precedence **ENV > `~/.anvil/router.json` > `None`**.
/// `env_key` is the full env var name (e.g. `ANVIL_ROUTER_MODEL`); `file_key` is the
/// short JSON key in `router.json` (e.g. `model`).
///
/// When the env var is set (non-empty) it wins. When it is unset the file is consulted.
/// When neither resolves, `None` (the caller applies its built-in default). This makes
/// the durable config a file write (`~/.anvil/router.json`) rather than a fragile
/// per-harness env var.
pub fn router_config(env_key: &str, file_key: &str) -> Option<String> {
    if let Some(v) = env_nonempty(env_key) {
        return Some(v);
    }
    router_config_file(file_key)
}

/// Read one string field from the router config JSON file. FAIL-SAFE: a missing,
/// unreadable, or malformed file — or a missing / non-string / empty field — is
/// treated as absent (`None`), never a panic. So a bad file degrades to the built-in
/// default, never breaking a turn. The file path is `ANVIL_ROUTER_CONFIG_FILE` when
/// set, else `$HOME/.anvil/router.json`.
fn router_config_file(file_key: &str) -> Option<String> {
    let path = env_nonempty("ANVIL_ROUTER_CONFIG_FILE")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(ROUTER_CONFIG_REL)))?;
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    match json.get(file_key)? {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    }
}

/// Build the single Kiln gateway call target from the environment / config file.
fn build_call() -> KilnCall {
    let port = env_nonempty("ANVIL_KILN_PORT")
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|&p| p > 0)
        .or_else(kit_ports_kiln)
        .unwrap_or(KILN_DEFAULT_PORT);
    let model =
        router_config("ANVIL_ROUTER_MODEL", "model").unwrap_or_else(|| KILN_DEFAULT_MODEL.to_string());
    let org = env_nonempty("ANVIL_KILN_ORG").unwrap_or_else(|| DEFAULT_KILN_ORG.to_string());
    let mut headers = vec![
        ("x-kiln-org".to_string(), org),
        ("x-kiln-purpose".to_string(), "route".to_string()),
        ("x-kiln-sensitivity".to_string(), "public".to_string()),
        ("x-kiln-thinking".to_string(), "off".to_string()),
    ];
    // `x-kiln-target` is only attached when explicitly configured.
    if let Some(target) = router_config("ANVIL_ROUTER_TARGET", "target") {
        headers.push(("x-kiln-target".to_string(), target));
    }
    KilnCall {
        host: KILN_HOST.to_string(),
        port,
        path: CHAT_PATH.to_string(),
        model,
        headers,
        bearer: read_gateway_bearer(),
    }
}

/// Resolve the kiln gateway shared bearer (NEVER a provider key): `ANVIL_KILN_TOKEN`
/// env → the file at `ANVIL_KILN_TOKEN_FILE` (default
/// `~/.foundry/run/shared-secrets/kiln-auth-token`) → `None`. A missing/rotated token
/// simply means the gateway may reject the call and the router fails open —
/// fail-open intact.
fn read_gateway_bearer() -> Option<String> {
    if let Some(token) = env_nonempty("ANVIL_KILN_TOKEN") {
        return Some(token);
    }
    let path = env_nonempty("ANVIL_KILN_TOKEN_FILE")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(DEFAULT_TOKEN_FILE_REL)))?;
    let token = std::fs::read_to_string(path).ok()?.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Best-effort read of the live kiln gateway port from the Foundry kit-port
/// rendezvous `~/.foundry/run/kit-ports.json["kiln-kit"]`. `None` on any gap.
fn kit_ports_kiln() -> Option<u16> {
    let path = home_dir()?.join(KIT_PORTS_REL);
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let port = json.get("kiln-kit")?;
    // The rendezvous may store the port as a number or a string.
    port.as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .or_else(|| port.as_str().and_then(|s| s.trim().parse::<u16>().ok()))
        .filter(|&p| p > 0)
}

/// Completion budget for the single router call. Overridable with
/// `ANVIL_ROUTER_MAX_TOKENS` because the budget is COUPLED to candidate breadth: the
/// 1-candidate prompt answers in ~144 completion tokens, but the real 26-candidate prompt
/// (6,389 chars) hits `finish_reason: length` with EMPTY content at 256 and answers in
/// 772 at 1500. Without this override the wide-candidate arm is unevaluable — every wide
/// call truncates to silence and the arm reads as "no help" when it was never measured.
fn max_tokens() -> u32 {
    std::env::var("ANVIL_ROUTER_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_ROUTER_MAX_TOKENS)
}

/// 1500, raised from 256 on 2026-07-28. The old value was sized for a 1-candidate prompt
/// (~144 completion tokens). The full-candidate prompt is 6,389 chars and returns
/// `finish_reason: length` with EMPTY content at 256, answering in 772 at 1500 — so with
/// wide candidates the old budget truncated EVERY call into silence. Raising it alone
/// changes nothing measurable (+0.5 hits/40 rows, noise); it is a prerequisite for
/// breadth, not an improvement on its own.
const DEFAULT_ROUTER_MAX_TOKENS: u32 = 1500;

/// Build the chat-completions request JSON body — mirrors the validated offline
/// experiment: a system router prompt + the user message, `max_tokens: 256`,
/// `temperature: 0`, `reasoning_effort: "low"`, and
/// `chat_template_kwargs.enable_thinking: false` (no-think). `model` is supplied by
/// the caller.
fn request_body(
    model: &str,
    message: &str,
    recent_context: &str,
    in_progress: &InProgressSignal,
    candidates: &[CandidateBrief],
) -> String {
    let system = build_router_prompt(message, recent_context, in_progress, candidates);
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": message },
        ],
        "max_tokens": max_tokens(),
        "temperature": 0,
        // Cloud reasoning models (Fireworks glm/gpt-oss/deepseek) honor `reasoning_effort`.
        // `low` caps reasoning so the model answers within budget instead of thinking past
        // the cap + truncating (verified: glm-5p2 low = ~2.5s, finish=stop; default = >4s
        // timeout/empty). 256 tokens leaves room for the capped reasoning + the JSON answer
        // (120 truncated it → parse_gap). `enable_thinking` is sent for any backend that
        // honors that chat-template kwarg instead; each backend uses the one it understands.
        "reasoning_effort": "low",
        "chat_template_kwargs": { "enable_thinking": false },
    })
    .to_string()
}

/// Attempt the Kiln call: POST, read the reply, and map it to an [`Attempt`]. A reply
/// that never arrived is `TransportErr`; a non-2xx kiln ANSWERED with is `Failed` with
/// its classified cause; a 2xx with no parseable decision is a `ParseGap`; a parseable
/// Pick / Abstain is a `Verdict`.
async fn attempt_call(call: &KilnCall, body: &str) -> Attempt {
    // Diagnostic, default OFF. The router's own abstentions were unattributable: we could
    // see THAT it declined but never what it was asked or what it answered. Gated so the
    // hot path is unchanged.
    let diag = std::env::var("ANVIL_ROUTER_DIAGNOSE").is_ok_and(|v| !v.trim().is_empty() && v != "0");
    if diag {
        eprintln!("=== ROUTER REQUEST ===\n{body}");
    }
    let reply = match post_chat_completion(call, body).await {
        Ok(reply) => reply,
        Err(Some(cause)) => {
            if diag {
                eprintln!("=== ROUTER: kiln refused ({}) ===", cause.as_str());
            }
            return Attempt::Failed(cause);
        }
        Err(None) => {
            if diag {
                eprintln!("=== ROUTER: transport error ===");
            }
            return Attempt::TransportErr;
        }
    };
    if diag {
        eprintln!("=== ROUTER REPLY ===\n{reply}");
    }
    let Some(content) = extract_assistant_content(&reply) else {
        return Attempt::ParseGap;
    };
    let total_tokens = extract_total_tokens(&reply).unwrap_or(0);
    match parse_router_decision(&content) {
        Some(decision) if decision.kind.eq_ignore_ascii_case("abstain") => {
            Attempt::Verdict(RouterVerdict::Abstain, total_tokens)
        }
        Some(decision) => Attempt::Verdict(
            RouterVerdict::Pick {
                kind: decision.kind,
                why: decision.why,
            },
            total_tokens,
        ),
        None => Attempt::ParseGap,
    }
}

/// Hand-rolled HTTP/1.1 POST to the Kiln gateway's chat-completions endpoint over a raw
/// loopback TCP socket. Injects the gateway bearer + `x-kiln-*` headers.
///
/// The response BODY on a 2xx; `Err(Some(cause))` when kiln ANSWERED with a non-2xx,
/// classified; `Err(None)` when no reply arrived (connect / write / read / framing).
/// The two error arms are the whole point: this used to return a bare `None` for both,
/// so the caller could only ever say `transport_err`.
async fn post_chat_completion(
    call: &KilnCall,
    body: &str,
) -> Result<String, Option<KilnFailure>> {
    let mut stream = TcpStream::connect((call.host.as_str(), call.port))
        .await
        .map_err(|_| None)?;

    // Base headers, then the optional gateway bearer, then the `x-kiln-*` steering
    // headers.
    let mut extra = String::new();
    if let Some(bearer) = &call.bearer {
        extra.push_str(&format!("Authorization: Bearer {}\r\n", bearer));
    }
    for (name, value) in &call.headers {
        extra.push_str(&format!("{}: {}\r\n", name, value));
    }

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         {extra}\
         Connection: close\r\n\
         \r\n\
         {body}",
        path = call.path,
        host = call.host,
        port = call.port,
        len = body.len(),
        extra = extra,
        body = body
    );
    stream.write_all(request.as_bytes()).await.map_err(|_| None)?;
    stream.flush().await.map_err(|_| None)?;

    // `Connection: close` → read to EOF, capped.
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = stream.read(&mut chunk).await.map_err(|_| None)?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        if raw.len() > MAX_RESPONSE_BYTES {
            break;
        }
    }

    let text = String::from_utf8_lossy(&raw);
    // Body follows the blank line separating headers from body; a reply with no
    // such separator is unframed, which is a TRANSPORT failure and not a refusal.
    let reply_body = text.find("\r\n\r\n").map(|i| &text[i + 4..]).ok_or(None)?;
    // Status line: HTTP/1.1 <code> … — a status we cannot even read is likewise
    // not something kiln said.
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(None)?;
    if !(200..300).contains(&status) {
        // Kiln ANSWERED. Name what it said.
        return Err(Some(classify_kiln_failure(status, reply_body)));
    }
    Ok(reply_body.to_string())
}

/// Pull the assistant message content out of the OpenAI-shaped chat-completions
/// envelope (`choices[0].message.content`). Tolerates a chunked-framed body by
/// slicing from the first `{` (the JSON envelope) — Kiln returns a single,
/// un-streamed JSON object. `None` on any shape gap.
/// `usage.total_tokens` from an OpenAI-shaped reply envelope.
///
/// Best-effort by design: a gateway that omits `usage` yields `None` and the attempt
/// records 0 tokens rather than failing the route. A missing cost number must never
/// cost us a routing decision.
fn extract_total_tokens(body: &str) -> Option<u64> {
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    if end < start {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&body[start..=end]).ok()?;
    json.get("usage")?.get("total_tokens")?.as_u64()
}

fn extract_assistant_content(body: &str) -> Option<String> {
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    if end < start {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&body[start..=end]).ok()?;
    match json.pointer("/choices/0/message/content")? {
        serde_json::Value::String(content) => Some(content.clone()),
        content => Some(content.to_string()),
    }
}
