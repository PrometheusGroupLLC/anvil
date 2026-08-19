//! The ABSTENTION LEDGER (opt-in, local-only) — a durable, append-only,
//! conversation-aware record of the router's FINAL abstentions.
//!
//! When the route-turn hook's final outcome for a user turn is
//! [`RouteTurnOutcome::NoMatch`] (a genuine abstention — no playbook fit), and the
//! opt-in flag `ANVIL_ABSTENTION_LEDGER` is on, one JSON line is appended to
//! `<hearth>/abstentions/ledger.jsonl`. This is the demand-signal substrate for
//! later new-playbook suggestions (Phase 2/3, eval-hearth-side): abstentions are
//! demand for playbooks that don't exist yet.
//!
//! The unit of demand is the CONVERSATION, not the message — routing already
//! decides on the conversation (the hook feeds the router `recent_context` + the
//! in-progress signal), so the record carries `recent_context` and
//! `conversation_id` alongside the bare `message` and the `candidate_set` the
//! router saw. Later clustering counts DISTINCT conversations per theme.
//!
//! ## Discipline
//! - **Default OFF** (privacy): nothing is written unless the user explicitly opts
//!   in. NEVER baked into the published kit — same discipline as
//!   `ANVIL_ENFORCE_MEASUREMENT` / `ANVIL_SEMANTIC_ROUTE_RPC`.
//! - **Local-only**: it's the user's own data on their own box.
//! - **Append-only**: the file is opened in append mode; records are never mutated.
//! - **Fail-open**: any error (no hearth, missing dir, write failure, serialize
//!   gap) is logged best-effort to stderr and swallowed. The route-turn hook is
//!   ADVISORY and must NEVER fail a turn.
//! - **LLM-free**: this is a pure disk write. Clustering / theme-labeling is an
//!   eval-hearth Kiln job (Phase 2), never the engine.

use anvil_core::domain::hooks::route_turn::RouteTurnOutcome;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

/// One conversation-aware abstention record — the JSON shape appended to the
/// ledger. `candidate_set` is the router's turn-relevant MATCHING kinds (captured
/// before the routing decision was consumed); `at` is an RFC3339 timestamp;
/// `source` is the originating harness tag (`--source`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstentionRecord {
    /// The bare user message this turn abstained on.
    pub message: String,
    /// The conversation digest the router saw (the prior-turns context). This is
    /// what makes the record conversation-aware — clustering keys on
    /// message + recent_context for a richer theme id.
    pub recent_context: String,
    /// The conversation / session id. The high-water mark counts DISTINCT
    /// conversation_ids per theme (breadth of demand), so this must ride along.
    pub conversation_id: String,
    /// The turn-relevant matching kinds the router chose from (may be empty when
    /// the engine surfaced no candidates at all).
    pub candidate_set: Vec<String>,
    /// RFC3339 timestamp of the abstention.
    pub at: String,
    /// The originating harness tag (`--source`), e.g. `claude-code`.
    pub source: String,
}

/// The opt-in gate for the abstention ledger. Resolved with the SAME precedence as
/// the other router knobs (ENV `ANVIL_ABSTENTION_LEDGER` > `~/.anvil/router.json`
/// `abstention_ledger` > built-in default), so it honors the hermetic-test
/// `ANVIL_ROUTER_CONFIG_FILE` override too. Default OFF (the privacy default) —
/// enabled ONLY when the resolved value is `on` / `1` / `true`.
pub fn abstention_ledger_enabled() -> bool {
    ledger_flag_on(crate::kiln_router::router_config("ANVIL_ABSTENTION_LEDGER", "abstention_ledger").as_deref())
}

/// The PURE flag-resolution: map a resolved config value to on/off. `None` (unset)
/// and any value that is not `on` / `true` / `1` (case-insensitive) is OFF — the
/// privacy default. Extracted so the on/off decision is directly unit/brine-testable
/// without touching the environment or the config file.
pub fn ledger_flag_on(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim();
            v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true") || v == "1"
        }
        None => false,
    }
}

/// An RFC3339 timestamp for "now" (UTC). The record carries `at` as a plain string
/// so the append path stays clock-injectable in tests (they build the record).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Append `record` to the abstention ledger IFF (a) the ledger is `enabled` AND
/// (b) `outcome` is the router's final abstention ([`RouteTurnOutcome::NoMatch`]).
/// Any other outcome (Single / Candidates / Resume) or a disabled flag writes
/// NOTHING and returns `false`.
///
/// FAIL-OPEN: a missing hearth, an unwritable directory/file, or a serialize gap is
/// logged best-effort to stderr and swallowed (returns `false`) — the advisory hook
/// must never fail a turn. Returns `true` only when a line was appended.
pub fn record_abstention(
    enabled: bool,
    outcome: &RouteTurnOutcome,
    hearth: Option<&Path>,
    record: &AbstentionRecord,
) -> bool {
    // Privacy default + parity: no opt-in → never write.
    if !enabled {
        return false;
    }
    // Only genuine abstentions are demand signal. A routed / resume turn found a
    // home; a candidate turn is offered to the agent — neither is unmet demand.
    if !matches!(outcome, RouteTurnOutcome::NoMatch) {
        return false;
    }
    // A harness block is a system event, not a person wanting something. It
    // still ROUTES (all-messages policy is deliberate); it is simply not
    // evidence of unmet demand.
    if is_harness_generated(&record.message) {
        return false;
    }
    let Some(hearth) = hearth else {
        eprintln!(
            "anvil abstention-ledger: no hearth resolved for this turn; skipping append (fail-open)"
        );
        return false;
    };
    let line = match serde_json::to_string(record) {
        Ok(line) => line,
        Err(err) => {
            eprintln!("anvil abstention-ledger: serialize failed ({err}); skipping append (fail-open)");
            return false;
        }
    };
    match append_line(hearth, &line) {
        Ok(()) => true,
        Err(err) => {
            eprintln!("anvil abstention-ledger: append failed ({err}); skipping (fail-open)");
            false
        }
    }
}

/// Append one line (plus a trailing newline) to `<hearth>/abstentions/ledger.jsonl`,
/// creating the `abstentions/` directory when missing. Opens in APPEND mode so the
/// ledger is strictly append-only — existing records are never rewritten.
///
/// LINE-ATOMIC: many `anvil-hooks route-turn` PROCESSES append to this one file
/// concurrently. We build the whole physical line (`<json>` + its terminating
/// `\n`) FIRST and hand the kernel a SINGLE `write_all` on the `O_APPEND` handle.
/// POSIX guarantees each `write()` to an `O_APPEND` file atomically seeks to EOF
/// and writes, so one write of a whole line can never interleave with a
/// concurrent appender. The record is far below `PIPE_BUF`, so the kernel does
/// not split the single write. The previous code issued TWO writes (record, then
/// `"\n"`), which opened a window where another process's append landed BETWEEN
/// them — gluing two JSON objects onto one physical line (observed 3/14 lines in
/// the field). Keep this a single `write_all` of a pre-assembled line.
fn append_line(hearth: &Path, line: &str) -> std::io::Result<()> {
    let dir = hearth.join("abstentions");
    fs::create_dir_all(&dir)?;
    let path = dir.join("ledger.jsonl");
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut record_line = String::with_capacity(line.len() + 1);
    record_line.push_str(line);
    record_line.push('\n');
    file.write_all(record_line.as_bytes())?;
    Ok(())
}

/// Whether this turn's text is HARNESS-GENERATED rather than a human ask.
///
/// The ledger exists to answer one question: what work do people want that no
/// playbook covers? A `<task-notification>` block is a system event, not a
/// person wanting something — recording it as unmet demand puts noise in the
/// numerator of exactly the measurement that decides which playbooks get built.
///
/// Measured on the live fleet before this filter existed: 1,217 of 3,206 ledger
/// rows (38.0%) were harness blocks. Anyone mining the ledger for demand was
/// mining a corpus that was more than a third machine chatter.
///
/// This does NOT change routing. Every message still routes — the routing policy
/// is deliberately all-messages, and suppressing a turn from the ROUTER would be
/// a different and much larger decision. This only governs what counts as
/// evidence of unmet demand.
pub fn is_harness_generated(message: &str) -> bool {
    const MARKERS: &[&str] = &[
        "<task-notification>",
        "<system-reminder>",
        "<local-command-stdout>",
        "<local-command-name>",
        "<command-name>",
        "[SYSTEM NOTIFICATION",
        "Caveat: The messages below were generated by the user while running local commands",
        "hook success:",
    ];
    let head: String = message.chars().take(4096).collect();
    MARKERS.iter().any(|m| head.contains(m))
}
