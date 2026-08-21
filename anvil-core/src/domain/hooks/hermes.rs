//! The Kiln + Hermes native pre-tool adapters — our own harnesses, full
//! force+validate (HARD gate).
//!
//! Kiln (`~/.kiln`, config.toml) is OUR harness with no discoverable native
//! pre-tool hook schema, so it gets a clearly DELIMITED, anvil-managed fenced
//! block (see [`super::append_fenced_block`]) declaring the gate — idempotent
//! (prior block stripped first) and reversible (uninstall removes only the block).
//!
//! Hermes (`~/.hermes`, config.yaml) has a REAL, documented hook schema: a
//! top-level `hooks:` map keyed by EVENT, each event a list of
//! `{ matcher, command, timeout }` entries (see `cli-config.yaml.example` and
//! `agent/shell_hooks.py`). The pre-mutation gate is the `pre_tool_call` event;
//! Hermes ACCEPTS the Claude-Code-style `{"decision":"block","reason":...}` our
//! `gate-check` already emits, so only the INSTALL format changes.
//!
//! ## Preserving the user's hand-maintained config
//!
//! `~/.hermes/config.yaml` is a large, comment-bearing YAML the user maintains by
//! hand. A serde round-trip would drop comments and reorder keys, so the Hermes
//! adapter NEVER round-trips. Instead it surgically inserts (and removes) ONLY an
//! anvil-managed list entry under `hooks.pre_tool_call`, leaving every other line,
//! key, and comment byte-for-byte intact. The anvil entry is stamped with the
//! [`HERMES_ENTRY_MARKER`] comment so install is idempotent (re-run finds and
//! replaces the prior anvil entry, never duplicating it) and uninstall removes
//! ONLY the anvil entry (collapsing an emptied `pre_tool_call:` / `hooks:` that
//! anvil itself created).

use super::{
    append_fenced_block, has_fenced_block, strip_fenced_block, GateCapability, Harness,
    HookAdapter, InstallSpec,
};

/// The Hermes tool-name matcher (regex, fullmatched against the tool name) for
/// the file-MUTATING tools `write_file` and `patch` — the documented set the
/// `cli-config.yaml.example` auto-format hook gates.
pub const HERMES_MATCHER: &str = "write_file|patch";

/// The marker comment stamped on the anvil-managed `pre_tool_call` entry so it is
/// found and removed unambiguously without disturbing the user's own entries.
pub const HERMES_ENTRY_MARKER: &str = "# anvil-hooks managed (do not edit)";

const HOOKS_KEY: &str = "hooks:";
const EVENT_KEY: &str = "pre_tool_call:";
/// The Hermes hook event for the per-turn router: fires before each LLM call,
/// carrying the user prompt. The anvil-managed entry here ships every turn into
/// `anvil-hooks route-turn`; its stdout is injected back to the agent. No matcher.
const TURN_EVENT_KEY: &str = "pre_llm_call:";

// ---------------------------------------------------------------------------
// Kiln — fenced-block native adapter (TOML, no documented schema).
// ---------------------------------------------------------------------------

/// The fenced-block native adapter used by Kiln. It declares an anvil-managed
/// pre-tool gate inside a clearly delimited block in the harness's native config.
pub struct FencedNativeAdapter {
    harness: Harness,
}

impl FencedNativeAdapter {
    pub fn new(harness: Harness) -> Self {
        FencedNativeAdapter { harness }
    }
}

/// The fenced body declaring a native pre-tool gate hook for `harness`.
fn native_body(harness: Harness, spec: &InstallSpec) -> String {
    format!(
        "# Anvil native pre-tool gate for {name} (force+validate).\n\
         # The {name} pre-tool hook invokes the gate command before any mutation;\n\
         # it BLOCKS edits to hard-enforced forge artifacts that lack an open begin.\n\
         pre_tool_use = {{ command = \"{cmd}\", timeout = {timeout} }}",
        name = harness.id(),
        cmd = spec.command,
        timeout = spec.timeout_ms,
    )
}

impl HookAdapter for FencedNativeAdapter {
    fn harness(&self) -> Harness {
        self.harness
    }

    fn gate_capability(&self) -> GateCapability {
        GateCapability::Hard
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        Ok(append_fenced_block(
            existing,
            &native_body(self.harness, spec),
        ))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        Ok(strip_fenced_block(existing))
    }
}

// ---------------------------------------------------------------------------
// Hermes — real `hooks.pre_tool_call` YAML adapter (comment-preserving).
// ---------------------------------------------------------------------------

/// The Hermes adapter: surgically inserts/removes an anvil-managed
/// `pre_tool_call` entry in the user's `~/.hermes/config.yaml`, preserving every
/// other line and comment.
pub struct HermesAdapter;

impl HookAdapter for HermesAdapter {
    fn harness(&self) -> Harness {
        Harness::Hermes
    }

    fn gate_capability(&self) -> GateCapability {
        // Hermes is our own harness: its `pre_tool_call` hook can BLOCK.
        GateCapability::Hard
    }

    fn install(&self, existing: &str, spec: &InstallSpec) -> Result<String, String> {
        // Install BOTH the pre-tool gate entry and the per-turn router entry.
        let gated = install_entry(existing, EVENT_KEY, &gate_entry_lines(spec));
        Ok(install_entry(
            &gated,
            TURN_EVENT_KEY,
            &turn_entry_lines(Harness::Hermes, spec),
        ))
    }

    fn uninstall(&self, existing: &str) -> Result<String, String> {
        // Remove BOTH anvil entries, collapsing any keys anvil emptied.
        let ungated = strip_anvil_entry(existing, EVENT_KEY);
        Ok(strip_anvil_entry(&ungated, TURN_EVENT_KEY))
    }
}

/// Hermes hook timeouts are in SECONDS (the spec carries milliseconds, so the
/// binary's `--timeout 5000` becomes `timeout: 5`).
fn timeout_secs(spec: &InstallSpec) -> u64 {
    (spec.timeout_ms / 1000).max(1)
}

/// The ROUTE-TURN entry's timeout in Hermes' seconds. Separate from
/// [`timeout_secs`] because the route turn makes model calls and the gate does
/// not; see [`InstallSpec::turn_timeout_ms`].
fn turn_timeout_secs(spec: &InstallSpec) -> u64 {
    (spec.turn_timeout_ms / 1000).max(1)
}

/// The lines of the anvil-managed `pre_tool_call` GATE entry. Opens with a
/// `- matcher:` line carrying the marker comment.
fn gate_entry_lines(spec: &InstallSpec) -> Vec<String> {
    vec![
        format!(
            "    - matcher: \"{}\"  {}",
            HERMES_MATCHER, HERMES_ENTRY_MARKER
        ),
        format!("      command: \"{}\"", spec.command),
        format!("      timeout: {}", timeout_secs(spec)),
    ]
}

/// The lines of the anvil-managed `pre_llm_call` per-TURN entry. No matcher —
/// every turn is routed — so the list item opens with the `- command:` line
/// carrying the marker comment.
fn turn_entry_lines(harness: Harness, spec: &InstallSpec) -> Vec<String> {
    vec![
        format!(
            "    - command: \"{}\"  {}",
            harness.turn_command_with_source(spec),
            HERMES_ENTRY_MARKER
        ),
        format!("      timeout: {}", turn_timeout_secs(spec)),
    ]
}

/// The command prefix every anvil-installed hook entry invokes. This is the
/// evidence of ownership that SURVIVES a comment-stripping rewrite, which the
/// marker does not.
const ANVIL_COMMAND_PREFIX: &str = "anvil-hooks";

/// Whether the list item spanning `item` (its opening `- …` line plus its
/// continuation lines) is one anvil installed.
///
/// The marker comment is the primary evidence, but it CANNOT be the only one.
/// Foundry's Hermes MCP writer round-trips the whole config through serde
/// (`read_or_empty_yaml_mapping` → `write_yaml_atomic`), and serde carries no
/// comments — so every `foundry kit install` silently strips the markers off
/// anvil's own entries and re-emits them in serde's sequence style (the `-` at
/// the PARENT key's indent rather than nested under it).
///
/// Recognizing only the marker made that unrecoverable: the next install found
/// no anvil entry, inserted a fresh nested one, and the file was left holding
/// BOTH — a mapping with sequence items as siblings of its keys, which is not
/// valid YAML at all. Every subsequent read of the config then failed to parse.
/// Judging ownership by the entry's `command:` instead makes the re-install
/// idempotent again across the round-trip.
fn item_is_anvil_owned(item: &[&str]) -> bool {
    if item.first().is_some_and(|l| l.contains(HERMES_ENTRY_MARKER)) {
        return true;
    }
    item.iter().any(|l| {
        let t = l.trim_start();
        // The command sits on the opening line for the matcher-less turn entry
        // (`- command: …`) and on a continuation line for the gate entry.
        let t = t.strip_prefix("- ").unwrap_or(t);
        t.strip_prefix("command:")
            .is_some_and(|rest| unquote(rest.trim()).starts_with(ANVIL_COMMAND_PREFIX))
    })
}

/// The exclusive end index of the list item opening at `open` — its
/// continuation lines, stopping at the next item, a line at/above the item's
/// indent, or a blank line.
fn item_end(lines: &[&str], open: usize) -> usize {
    let item_indent = indent_of(lines[open]);
    let mut end = open + 1;
    while end < lines.len() {
        let l = lines[end];
        if l.trim().is_empty() {
            break;
        }
        if indent_of(l) <= item_indent || l.trim_start().starts_with("- ") {
            break;
        }
        end += 1;
    }
    end
}

/// Whether `line` is a top-level YAML key (no leading whitespace, has a `key:`)
/// — the boundary that ends the `hooks:` block.
fn is_top_level_key(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with([' ', '\t'])
        && !line.trim_start().starts_with('#')
        && line.contains(':')
}

/// Whether `line` (trimmed) is exactly the `hooks:` top-level key, optionally an
/// empty flow map `hooks: {}`.
fn is_hooks_key(line: &str) -> bool {
    let t = line.trim_end();
    t == HOOKS_KEY || t == "hooks: {}"
}

/// Install an anvil-managed entry under `hooks.<event_key>`, replacing any prior
/// anvil entry for that event first (idempotent) and preserving every unrelated
/// line/comment. Serves both the `pre_tool_call` gate and `pre_llm_call` turn
/// events.
fn install_entry(existing: &str, event_key: &str, entry: &[String]) -> String {
    // Always strip a prior anvil entry for THIS event first so re-install is a
    // pure no-op.
    let base = strip_anvil_entry(existing, event_key);
    let lines: Vec<&str> = base.lines().collect();

    // Case A: there is a `hooks:` block (block-style or empty flow map).
    if let Some(hooks_idx) = lines.iter().position(|l| is_hooks_key(l)) {
        return insert_into_hooks(&lines, hooks_idx, event_key, entry);
    }

    // Case B: no `hooks:` key at all — append a fresh block at the end.
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    out.push(HOOKS_KEY.to_string());
    out.push(format!("  {}", event_key));
    out.extend(entry.iter().cloned());
    finalize(out)
}

/// Insert the anvil entry into an existing `hooks:` block at `hooks_idx` under
/// `event_key`.
fn insert_into_hooks(
    lines: &[&str],
    hooks_idx: usize,
    event_key: &str,
    entry: &[String],
) -> String {
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + entry.len() + 2);

    // Everything before the hooks key is untouched.
    out.extend(lines[..hooks_idx].iter().map(|s| s.to_string()));

    // Normalise an empty flow map `hooks: {}` into a block-style `hooks:`.
    let hooks_line = lines[hooks_idx];
    if hooks_line.trim_end() == "hooks: {}" {
        out.push(HOOKS_KEY.to_string());
    } else {
        out.push(hooks_line.to_string());
    }

    // Find the extent of the hooks block (until the next top-level key/EOF) and
    // whether it already declares `<event_key>`.
    let mut end = lines.len();
    let mut event_idx: Option<usize> = None;
    for (i, l) in lines.iter().enumerate().skip(hooks_idx + 1) {
        if is_top_level_key(l) {
            end = i;
            break;
        }
        if l.trim_start().starts_with(event_key) {
            event_idx = Some(i);
        }
    }

    match event_idx {
        // `<event_key>` already present — splice the entry right after it.
        Some(ev) => {
            for (i, l) in lines.iter().enumerate().take(end).skip(hooks_idx + 1) {
                out.push(l.to_string());
                if i == ev {
                    out.extend(entry.iter().cloned());
                }
            }
        }
        // No `<event_key>` yet — add the key + entry at the top of the block.
        None => {
            out.push(format!("  {}", event_key));
            out.extend(entry.iter().cloned());
            out.extend(lines[(hooks_idx + 1)..end].iter().map(|s| s.to_string()));
        }
    }

    // Everything after the hooks block is untouched.
    out.extend(lines[end..].iter().map(|s| s.to_string()));
    finalize(out)
}

/// Drop the anvil entry's contiguous lines (the marker list item and its
/// indented continuation lines) WITHIN the `<event_key>` block only, then
/// collapse a `<event_key>` / `hooks:` that became empty as a result. Scoping the
/// removal to `event_key` lets the gate (`pre_tool_call`) and turn
/// (`pre_llm_call`) entries be installed/removed independently without one
/// stripping the other.
fn strip_anvil_entry(existing: &str, event_key: &str) -> String {
    let lines: Vec<&str> = existing.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());

    // Track whether we are inside the target `<event_key>` block: enter on the
    // event-key line, leave when a line at/above the event key's indent appears
    // (another event, or back out to `hooks:`/top level).
    let mut event_indent: Option<usize> = None;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed_empty = line.trim().is_empty();

        // Update event-block membership.
        if let Some(ev_ind) = event_indent {
            if !trimmed_empty && indent_of(line) <= ev_ind && !line.trim_start().starts_with("- ") {
                // A sibling key at/above the event key's indent ends the block.
                event_indent = None;
            }
        }
        if line.trim_start().starts_with(event_key)
            && line.trim_start().trim_end() == event_key.trim_end()
        {
            event_indent = Some(indent_of(line));
            out.push(line.to_string());
            i += 1;
            continue;
        }

        if event_indent.is_some() && line.trim_start().starts_with("- ") {
            // Resolve the item's extent FIRST, then judge ownership from the whole
            // item: an entry stripped of its marker only proves it is anvil's by
            // the `command:` on a continuation line.
            let end = item_end(&lines, i);
            if item_is_anvil_owned(&lines[i..end]) {
                // Drop the entry: its opening line plus its continuation lines.
                i = end;
                continue;
            }
        }
        out.push(line.to_string());
        i += 1;
    }

    let collapsed = collapse_empty_hooks(out, event_key);
    finalize(collapsed)
}

/// Collapse a `<event_key>` with no remaining entries, and a `hooks:` with no
/// remaining children, that anvil emptied. Only collapses keys that have NOTHING
/// indented under them (so the user's other events/entries are never touched).
fn collapse_empty_hooks(lines: Vec<String>, event_key: &str) -> Vec<String> {
    // First pass: drop an empty `<event_key>` (no indented child after it).
    let pruned = drop_empty_key(lines, event_key);
    // Second pass: a `hooks:` with no indented child becomes the empty flow map.
    restore_empty_hooks(pruned)
}

/// Remove a `<key>` line that has no more-indented child line following it.
fn drop_empty_key(lines: Vec<String>, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.trim_start().starts_with(key) && l.trim_start() == key {
            let key_indent = indent_of(l);
            // Peek the next non-blank line: if it is NOT more-indented, this key
            // is childless → drop it.
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            let has_child = j < lines.len() && indent_of(&lines[j]) > key_indent;
            if !has_child {
                i += 1;
                continue;
            }
        }
        out.push(l.clone());
        i += 1;
    }
    out
}

/// If a `hooks:` (block-style) line now has no indented child, restore it to the
/// empty flow map `hooks: {}` so the file stays valid YAML.
fn restore_empty_hooks(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.trim_end() == HOOKS_KEY {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            let has_child = j < lines.len() && indent_of(&lines[j]) > indent_of(l);
            if !has_child {
                out.push("hooks: {}".to_string());
                i += 1;
                continue;
            }
        }
        out.push(l.clone());
        i += 1;
    }
    out
}

/// Leading-space count of `line`.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Join lines and apply a single trailing newline iff the result has content.
fn finalize(lines: Vec<String>) -> String {
    let mut joined = lines.join("\n");
    while joined.ends_with('\n') {
        joined.pop();
    }
    if joined.is_empty() {
        joined
    } else {
        joined.push('\n');
        joined
    }
}

// ---------------------------------------------------------------------------
// Inspection helpers (used by the brine seam + reporting).
// ---------------------------------------------------------------------------

/// Whether the config carries an anvil-managed native pre-tool hook block.
///
/// For Hermes this is the anvil-managed `pre_tool_call` entry; for Kiln this is
/// the fenced block. The single function serves both so the shared brine steps
/// keep working across both harnesses.
pub fn has_managed_pretool(existing: &str) -> bool {
    has_fenced_block(existing) || managed_pretool_count(existing) > 0
}

/// True when `line` opens the anvil-managed GATE entry (a `- matcher:` list item
/// carrying the marker).
fn is_gate_entry_open(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- matcher:") && line.contains(HERMES_ENTRY_MARKER)
}

/// True when `line` opens the anvil-managed per-TURN entry (a `- command:` list
/// item carrying the marker — the matcher-less `pre_llm_call` entry).
fn is_turn_entry_open(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- command:") && line.contains(HERMES_ENTRY_MARKER)
}

/// The number of anvil-managed `pre_tool_call` GATE entries (identified by the
/// marker comment on a `- matcher:` line). Should be 0 or 1.
pub fn managed_pretool_count(existing: &str) -> usize {
    existing.lines().filter(|l| is_gate_entry_open(l)).count()
}

/// The number of anvil-managed `pre_llm_call` per-TURN entries (identified by the
/// marker comment on a `- command:` line). Should be 0 or 1.
pub fn managed_turn_count(existing: &str) -> usize {
    existing.lines().filter(|l| is_turn_entry_open(l)).count()
}

/// The (command, timeout_seconds) of the anvil-managed `pre_llm_call` per-TURN
/// entry, if present.
pub fn managed_turn_detail(existing: &str) -> Option<(String, u64)> {
    let lines: Vec<&str> = existing.lines().collect();
    let open = lines.iter().position(|l| is_turn_entry_open(l))?;
    let command = extract_quoted_value(lines[open], "command:")?;
    let item_indent = indent_of(lines[open]);

    let mut timeout: Option<u64> = None;
    for l in lines.iter().skip(open + 1) {
        if l.trim().is_empty() {
            break;
        }
        let ind = indent_of(l);
        if ind <= item_indent || l.trim_start().starts_with("- ") {
            break;
        }
        if let Some(rest) = l.trim_start().strip_prefix("timeout:") {
            timeout = rest.trim().parse().ok();
        }
    }
    Some((command, timeout?))
}

/// The (matcher, command, timeout_seconds) of the anvil-managed `pre_tool_call`
/// entry, if present.
pub fn managed_pretool_detail(existing: &str) -> Option<(String, String, u64)> {
    let lines: Vec<&str> = existing.lines().collect();
    let open = lines.iter().position(|l| is_gate_entry_open(l))?;

    let matcher = extract_quoted_value(lines[open], "matcher:")?;
    let item_indent = indent_of(lines[open]);

    let mut command: Option<String> = None;
    let mut timeout: Option<u64> = None;
    for l in lines.iter().skip(open + 1) {
        if l.trim().is_empty() {
            break;
        }
        let ind = indent_of(l);
        if ind <= item_indent || l.trim_start().starts_with("- ") {
            break;
        }
        let t = l.trim_start();
        if let Some(rest) = t.strip_prefix("command:") {
            command = Some(unquote(rest.trim()));
        } else if let Some(rest) = t.strip_prefix("timeout:") {
            timeout = rest.trim().parse().ok();
        }
    }

    Some((matcher, command?, timeout?))
}

/// Extract a quoted scalar following `key` on `line` (e.g. `matcher: "x"` → `x`).
fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    let after = line.split_once(key)?.1.trim();
    // Strip a trailing marker comment if present.
    let value = after.split("  #").next().unwrap_or(after).trim();
    Some(unquote(value))
}

/// Remove surrounding double quotes from a scalar, if present.
fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}
