/// The raw top-level `state:` value as it reads in the file text.
///
/// TEXTUAL, not serde: only a line beginning at column 0 with `state:` counts.
/// The `activity:` markers each carry an INDENTED `state:` and must never be
/// mistaken for the header. The first top-level occurrence wins (serde would
/// reject a duplicate key outright; this reader answers what a human sees).
pub fn declared_state_header(content: &str) -> Option<String> {
    content
        .lines()
        .find(|line| is_state_header_line(line))
        .map(|line| line["state:".len()..].trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A top-level `state:` header line — column 0, key `state`, value separated by
/// `:` (with or without a value on the line).
fn is_state_header_line(line: &str) -> bool {
    line.strip_prefix("state:")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// Set the top-level `state:` header in raw status.yaml text, leaving every
/// other byte untouched.
///
/// A **line-level textual edit**, deliberately not a `FullStatusYaml`
/// re-serialization — the same discipline `fs_activity_write_adapter` and
/// `fs_actor_write_adapter` follow. Re-serializing would reorder and reformat
/// the `actors:`, `transitions:` and `activity:` blocks of every artifact in
/// the hearth, turning a one-line repair into an unreviewable diff.
///
/// When the header is absent it is INSERTED immediately after the `kind:` line
/// (else after `version:`, else at the top) so the repaired file has the same
/// key order the engine's own `build_initial_artifact_status_yaml` writes.
pub fn set_state_header(content: &str, state: &str) -> String {
    // Byte-identity on a no-op. `lines()`/`join` would otherwise normalize
    // trailing whitespace on files that need no repair at all, and a caller
    // that decides "write?" by comparing bytes (the K8 journal does) would
    // journal a pointless effect.
    if declared_state_header(content).as_deref() == Some(state) {
        return content.to_string();
    }
    // `split('\n')` (not `lines()`): the trailing empty segment of a
    // newline-terminated file is preserved, so `join` round-trips the input
    // byte-for-byte apart from the one line this function touches. `lines()`
    // silently ate a file's trailing blank line — measured on three
    // foundry-hearth tracks, a repair that should be `+1 -1` came out `+1 -2`.
    let mut lines: Vec<String> = content.split('\n').map(|l| l.to_string()).collect();
    let header = format!("state: {state}");
    match lines.iter().position(|l| is_state_header_line(l)) {
        Some(idx) => lines[idx] = header,
        None => {
            let anchor = lines
                .iter()
                .position(|l| l.starts_with("kind:"))
                .or_else(|| lines.iter().position(|l| l.starts_with("version:")));
            let insert_at = anchor.map(|i| i + 1).unwrap_or(0);
            lines.insert(insert_at, header);
        }
    }
    let out = lines.join("\n");
    // An input with no trailing newline gets one (the engine writes newline-
    // terminated YAML); an input that had one keeps exactly the one it had.
    if out.ends_with('\n') || out.is_empty() {
        out
    } else {
        format!("{out}\n")
    }
}
