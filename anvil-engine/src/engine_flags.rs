//! HEARTH-LOCAL ENGINE FLAGS (durable, local opt-ins that survive kit updates).
//!
//! The router opt-ins (`ANVIL_ENFORCE_MEASUREMENT_DEFINITION`,
//! `ANVIL_SEMANTIC_ROUTE_RPC`, `ANVIL_ABSTENTION_LEDGER`) are LOCAL, opt-in knobs
//! — enabled only on Nick's backfilled hearth, NEVER baked into the published kit
//! (baking them flips the ecosystem onto unbackfilled hearths). Until now they
//! lived only in the installed kit's cached `foundry-manifest.json` `engine.env`,
//! so every `update_kit` silently reverted the engine to lexical / no-ledger /
//! no-enforcement until someone re-flipped them by hand.
//!
//! This module makes those opt-ins DURABLE: at engine startup, after process env
//! is read, the engine merges OPTIONAL overrides from a hearth-local flags file at
//! `<global_playbooks_hearth>/engine-flags.env`. The file is simple `KEY=VALUE`
//! lines (`#` comments + blank lines ignored). Precedence:
//!
//!   real process env  >  the flags file  >  built-in defaults
//!
//! i.e. an explicit process-env var always wins (explicit operator intent beats
//! stored config); the file only fills in keys the env does NOT set; and a key set
//! by neither keeps the code's built-in default. Only `ANVIL_`-prefixed keys are
//! honored — any other key is ignored (and warned about once). A missing or
//! unreadable file — or no global hearth — is a SILENT no-op (fail-open): the
//! engine must never fail to start over this optional config.
//!
//! ## Where the merge happens
//! The merge is applied ONCE at startup via [`install_hearth_local_flags`], which
//! `set_var`s each resolved override into the process env. Every existing
//! per-callsite read (`kiln_router::router_config`, `abstention_ledger`,
//! `semantic_route`, `enforce_measurement_definition`) then reads the merged value
//! with ZERO refactor — the env it reads already reflects the file. The pure
//! resolution ([`resolve_hearth_local_flags`], [`parse_flags`], [`merge`]) is
//! separated from the `set_var` I/O so the precedence logic is directly testable
//! without mutating the global process environment.
//!
//! One flag is deliberately excluded from that startup merge:
//! [`ENFORCE_CLAIMED_EVIDENCE_FLAG`] is a fail-closed, per-request-lane gate. It
//! is resolved non-mutatively from the request hearth for each transition; only
//! an explicit process value may opt an intentionally single-lane process into
//! an all-lanes override.
//!
//! ## SOUNDNESS: the merge must run while the process is SINGLE-THREADED
//! `std::env::set_var` is only sound on Unix while no other thread can be reading
//! the environment concurrently (a concurrent `getenv`/`set_var` is a data race —
//! this is why the 2024 edition marks `set_var` `unsafe`). The Tokio multi-thread
//! runtime and the tracing subscriber's background workers BOTH spawn threads, so
//! [`install_hearth_local_flags`] MUST be called BEFORE either is constructed — as
//! the lexically first work each binary's `main` does. It therefore does NO
//! logging itself (no subscriber exists yet); it returns the [`ResolvedFlags`] so
//! the caller can emit the human-readable line via [`log_installed_flags`] AFTER
//! the tracing subscriber is up.
//!
//! ## Every consumer merges its OWN startup
//! The three flags are read by TWO processes, not one: `enforce_measurement_definition`
//! and `semantic_route` are read inside the ENGINE process, but `ANVIL_ABSTENTION_LEDGER`
//! is read by the separate `anvil-hooks route-turn` PROCESS (which owns the ledger
//! append). A `set_var` in the engine can never reach another process, so EACH
//! binary that reads a flag calls [`install_hearth_local_flags`] against the hearth
//! it resolves, at its own single-threaded startup.

use std::path::{Path, PathBuf};

/// The file name of the hearth-local flags file, resolved relative to the global
/// playbooks hearth: `<global_playbooks_hearth>/engine-flags.env`.
pub const FLAGS_FILE_NAME: &str = "engine-flags.env";

/// Per-request-lane Phase-3 evidence-completeness gate. Unlike the startup
/// flags, this key is read from the RESOLVED request hearth on every lifecycle
/// transition and is never installed from a global hearth into process state.
pub const ENFORCE_CLAIMED_EVIDENCE_FLAG: &str = "ANVIL_ENFORCE_CLAIMED_EVIDENCE";

/// Only keys with this prefix are honored; every other key is ignored + warned.
const ANVIL_PREFIX: &str = "ANVIL_";

/// One honored `KEY=VALUE` override parsed from the flags file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagEntry {
    /// The env-var key (always `ANVIL_`-prefixed for an honored entry).
    pub key: String,
    /// The raw value (trimmed).
    pub value: String,
}

/// The parse of a flags file: honored `ANVIL_`-prefixed entries plus the keys we
/// ignored because they are not `ANVIL_`-prefixed (surfaced for the warn-once).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedFlags {
    /// The `ANVIL_`-prefixed `KEY=VALUE` entries, in file order.
    pub honored: Vec<FlagEntry>,
    /// Non-`ANVIL_` keys present in the file, ignored (warned once).
    pub ignored: Vec<String>,
}

/// The resolved outcome of consulting the hearth-local flags file: the path we
/// looked at (when a global hearth was configured), the overrides to APPLY (the
/// honored entries whose key is NOT already set in the process env), and the
/// ignored non-`ANVIL_` keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedFlags {
    /// The flags-file path consulted, or `None` when no global hearth is set.
    pub file: Option<PathBuf>,
    /// The honored entries to `set_var` — those absent from the process env.
    pub to_apply: Vec<FlagEntry>,
    /// Non-`ANVIL_` keys seen in the file, ignored (warned once).
    pub ignored: Vec<String>,
}

/// Parse the flags-file text into honored + ignored keys. Simple `KEY=VALUE` lines;
/// blank lines and lines whose first non-whitespace char is `#` are skipped; a line
/// with no `=` is malformed and skipped. Keys/values are trimmed. Only `ANVIL_`-
/// prefixed keys are honored; other keys are collected in `ignored`. FAIL-OPEN:
/// this never errors — a garbled file yields whatever parsed cleanly.
pub fn parse_flags(text: &str) -> ParsedFlags {
    let mut honored = Vec::new();
    let mut ignored = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue; // malformed (no `=`) — skip, fail-open.
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            continue;
        }
        if key.starts_with(ANVIL_PREFIX) {
            honored.push(FlagEntry {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else {
            ignored.push(key.to_string());
        }
    }
    ParsedFlags { honored, ignored }
}

/// Compute the overrides to APPLY from the honored entries, given a lookup of the
/// current process env. Precedence: real process env WINS over the file, so an
/// entry whose key is already present in the env (`env_get` returns `Some`) is
/// dropped; the rest are applied (the file winning over built-in defaults). A key
/// repeated in the file keeps its LAST occurrence (last write wins).
pub fn merge(honored: &[FlagEntry], env_get: impl Fn(&str) -> Option<String>) -> Vec<FlagEntry> {
    let mut out: Vec<FlagEntry> = Vec::new();
    for entry in honored {
        // Real process env wins — never override an explicitly-set var.
        if env_get(&entry.key).is_some() {
            continue;
        }
        // Last occurrence in the file wins.
        if let Some(existing) = out.iter_mut().find(|e| e.key == entry.key) {
            existing.value = entry.value.clone();
        } else {
            out.push(entry.clone());
        }
    }
    out
}

/// Read + parse the hearth-local flags file and compute the overrides to apply,
/// given an env lookup. FAIL-OPEN throughout: no global hearth → `file: None` and
/// nothing to apply; a missing or unreadable file → the path is recorded but
/// nothing is applied. The `env_get` closure supplies the current process env (real
/// `std::env::var` in production; an in-memory map in tests) so the precedence logic
/// is testable without mutating global state.
pub fn resolve_hearth_local_flags(
    global_playbooks_hearth: Option<&Path>,
    env_get: impl Fn(&str) -> Option<String>,
) -> ResolvedFlags {
    let Some(hearth) = global_playbooks_hearth else {
        return ResolvedFlags::default();
    };
    let path = hearth.join(FLAGS_FILE_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        // Missing / unreadable file → silent no-op (fail-open).
        return ResolvedFlags {
            file: Some(path),
            to_apply: Vec::new(),
            ignored: Vec::new(),
        };
    };
    let parsed = parse_flags(&text);
    let to_apply = merge(&parsed.honored, env_get);
    ResolvedFlags {
        file: Some(path),
        to_apply,
        ignored: parsed.ignored,
    }
}

/// Pure value parser for [`ENFORCE_CLAIMED_EVIDENCE_FLAG`]. The gate is dark by
/// default: only trimmed `1`, `true`, or `on` values enable it.
pub fn claimed_evidence_gate_value_is_on(value: Option<&str>) -> bool {
    match value {
        Some(value) => {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("on")
        }
        None => false,
    }
}

/// Resolve the claimed-evidence transition gate for ONE request lane without
/// mutating process-global state. An explicit process value wins and therefore
/// applies to every lane served by that process; operators may use that override
/// only for a declared single-lane engine. Otherwise the last matching entry in
/// `<request_hearth>/engine-flags.env` wins. Missing/unreadable files and every
/// unrecognized value remain OFF.
pub fn claimed_evidence_gate_enabled_for_hearth(
    request_hearth: &Path,
    env_get: impl Fn(&str) -> Option<String>,
) -> bool {
    let value = env_get(ENFORCE_CLAIMED_EVIDENCE_FLAG).or_else(|| {
        let text = std::fs::read_to_string(request_hearth.join(FLAGS_FILE_NAME)).ok()?;
        parse_flags(&text)
            .honored
            .into_iter()
            .rev()
            .find(|entry| entry.key == ENFORCE_CLAIMED_EVIDENCE_FLAG)
            .map(|entry| entry.value)
    });
    claimed_evidence_gate_value_is_on(value.as_deref())
}

/// Startup entry point: resolve the hearth-local flags against the REAL process env
/// and `set_var` each override into the process environment, so every downstream
/// per-callsite env read sees the merged value. Returns the [`ResolvedFlags`] that
/// were consulted/applied so the caller can log them once a tracing subscriber is
/// up (see [`log_installed_flags`]). FAIL-OPEN: any gap is a no-op.
///
/// # Ordering invariant (soundness)
/// Call this ONCE, as the LEXICALLY FIRST work in the binary's `main`, BEFORE the
/// Tokio runtime is built and BEFORE the tracing subscriber is initialized. On Unix
/// `set_var` is only sound while the process is single-threaded; the runtime and
/// tracing workers both spawn threads. This function must therefore NOT log (no
/// subscriber exists yet) and must NOT be preceded by any thread-spawning setup.
/// The merged env is then in place before any RPC / consumer read.
///
/// Called by BOTH the engine `main` (for `ANVIL_ENFORCE_MEASUREMENT_DEFINITION` /
/// `ANVIL_SEMANTIC_ROUTE_RPC`, read in-process) and the `anvil-hooks route-turn`
/// startup (for `ANVIL_ABSTENTION_LEDGER`, read in THAT process) — a `set_var` in
/// one process never reaches another, so every reading binary merges its own.
#[must_use]
pub fn install_hearth_local_flags(global_playbooks_hearth: Option<&Path>) -> ResolvedFlags {
    let mut resolved =
        resolve_hearth_local_flags(global_playbooks_hearth, |k| std::env::var(k).ok());
    // This key is lane-local. Installing it from the global playbooks hearth
    // would let one lane silently opt every request hearth into fail-closed
    // transition behavior. A deliberate process-env value remains the explicit
    // all-lanes/single-lane-process override documented above.
    resolved
        .to_apply
        .retain(|entry| entry.key != ENFORCE_CLAIMED_EVIDENCE_FLAG);
    for entry in &resolved.to_apply {
        std::env::set_var(&entry.key, &entry.value);
    }
    resolved
}

/// Emit the human-readable outcome of [`install_hearth_local_flags`]: ONE `INFO`
/// line naming the keys loaded from the file (only when at least one was applied),
/// and ONE `WARN` if any non-`ANVIL_` keys were present and ignored. Deferred from
/// `install` so it can run AFTER the tracing subscriber is initialized (install
/// itself runs before any thread/subscriber exists — see the ordering invariant).
/// A no-op when nothing was applied or ignored.
pub fn log_installed_flags(resolved: &ResolvedFlags) {
    if !resolved.to_apply.is_empty() {
        let keys: Vec<&str> = resolved.to_apply.iter().map(|e| e.key.as_str()).collect();
        tracing::info!(
            file = resolved.file.as_ref().map(|p| p.display().to_string()),
            keys = keys.join(","),
            "merged hearth-local engine flags from engine-flags.env"
        );
    }
    if !resolved.ignored.is_empty() {
        tracing::warn!(
            file = resolved.file.as_ref().map(|p| p.display().to_string()),
            ignored = resolved.ignored.join(","),
            "ignored non-ANVIL_ keys in hearth-local engine-flags.env"
        );
    }
}

// ---------------------------------------------------------------------------
// K8 backlog policy (plan Task 7). STRICT and PER-REQUEST: unlike the fail-open
// startup flags above, a PRESENT malformed value is a loud error, never a
// default, and one hearth's policy is never installed process-wide.
// ---------------------------------------------------------------------------

/// The ordered comparator specification key.
pub const BACKLOG_COMPARATOR_FLAG: &str = "ANVIL_BACKLOG_COMPARATOR";
/// The positive event-count age budget key.
pub const BACKLOG_AGE_BUDGET_FLAG: &str = "ANVIL_BACKLOG_AGE_BUDGET";

/// Resolve one K8 policy key for ONE request lane: explicit process value, then
/// the RESOLVED REQUEST hearth's `engine-flags.env`, then absent. A present K8
/// key is never routed through the fail-open flag path, so a malformed value
/// cannot silently become the default and one hearth's value can never leak
/// into another.
fn backlog_flag_for_hearth(
    key: &str,
    request_hearth: &Path,
    env_get: &impl Fn(&str) -> Option<String>,
) -> Option<String> {
    env_get(key).or_else(|| {
        let text = std::fs::read_to_string(request_hearth.join(FLAGS_FILE_NAME)).ok()?;
        parse_flags(&text)
            .honored
            .into_iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value)
    })
}

/// Strictly resolve the K8 [`BacklogPolicy`] for one request hearth. A present
/// malformed comparator or age budget returns the exact parser error; an absent
/// key uses the frozen default.
pub fn resolve_backlog_policy_for_hearth(
    request_hearth: &Path,
    env_get: impl Fn(&str) -> Option<String>,
) -> Result<
    anvil_core::domain::backlog_item::BacklogPolicy,
    anvil_core::domain::backlog_item::BacklogItemError,
> {
    use anvil_core::domain::backlog_item::{parse_age_budget, parse_comparator, BacklogPolicy};
    let mut policy = BacklogPolicy::default();
    if let Some(raw) = backlog_flag_for_hearth(BACKLOG_COMPARATOR_FLAG, request_hearth, &env_get) {
        policy.comparator = parse_comparator(&raw)?;
    }
    if let Some(raw) = backlog_flag_for_hearth(BACKLOG_AGE_BUDGET_FLAG, request_hearth, &env_get) {
        policy.age_budget = parse_age_budget(&raw)?;
    }
    Ok(policy)
}
