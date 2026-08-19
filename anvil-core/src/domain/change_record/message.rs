//! The commit message: subject, blank line, trailer block — a pure fold, plus
//! the inverse read of a rendered commit's trailers.
//!
//! The trailer key set is a DECLARED ALLOWLIST and is asserted over the
//! RENDERED commit, never over [`CommitMetadata`]: an assertion over a Rust
//! struct is a compile-time no-op, and the publication-log envelope (raw actor
//! name, raw approver, raw conversation id, artifact paths as ids) is one
//! `impl` away from being inherited here.
//!
//! [`parse_trailers`] is production code, not a test helper: the recovery pass
//! reads `Anvil-Operation-Id` back off the lineage to decide whether an
//! interrupted transaction's commit already landed. That read is the
//! idempotency key.

/// Every trailer key the change record may emit, in emission order. The first
/// [`REQUIRED_TRAILER_COUNT`] are always present; the rest are OMITTED when
/// absent — never emitted empty, never emitted raw.
pub const DECLARED_TRAILER_KEYS: &[&str] = &[
    "Anvil-Operation-Id",
    "Anvil-Command",
    "Anvil-Artifact-Kind",
    "Anvil-Event-Kinds",
    "Anvil-Repository-Label",
    "Anvil-Paths-Recorded",
    "Anvil-At",
    "Anvil-Actor-Hash",
    "Anvil-Conversation-Hash",
    "Anvil-Project-Label",
    "Anvil-Playbook-Run-Id",
];

/// How many leading entries of [`DECLARED_TRAILER_KEYS`] are mandatory.
pub const REQUIRED_TRAILER_COUNT: usize = 7;

/// What the engine ADDS to a commit. The tree is the transaction's own content
/// and is governed separately; nothing here may carry a raw actor name, a raw
/// conversation id, an approver, an absolute path, or user prose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitMetadata {
    pub operation_id: String,
    pub command: String,
    pub artifact_kind: String,
    pub event_kinds: Vec<String>,
    pub repository_label: String,
    pub paths_recorded: usize,
    pub at: String,
    pub actor_hash: Option<String>,
    pub conversation_hash: Option<String>,
    pub project_label: Option<String>,
    pub playbook_run_id: Option<String>,
}

/// Render the whole message. Deterministic in its input, which is what makes a
/// rolled-forward commit byte-identical to the one an uninterrupted run would
/// have written.
pub fn render_commit_message(meta: &CommitMetadata) -> String {
    let mut trailers: Vec<(&str, String)> = vec![
        (DECLARED_TRAILER_KEYS[0], meta.operation_id.clone()),
        (DECLARED_TRAILER_KEYS[1], meta.command.clone()),
        (DECLARED_TRAILER_KEYS[2], meta.artifact_kind.clone()),
        (DECLARED_TRAILER_KEYS[3], meta.event_kinds.join(",")),
        (DECLARED_TRAILER_KEYS[4], meta.repository_label.clone()),
        (DECLARED_TRAILER_KEYS[5], meta.paths_recorded.to_string()),
        (DECLARED_TRAILER_KEYS[6], meta.at.clone()),
    ];
    for (key, value) in [
        (DECLARED_TRAILER_KEYS[7], &meta.actor_hash),
        (DECLARED_TRAILER_KEYS[8], &meta.conversation_hash),
        (DECLARED_TRAILER_KEYS[9], &meta.project_label),
        (DECLARED_TRAILER_KEYS[10], &meta.playbook_run_id),
    ] {
        if let Some(present) = value.as_deref().filter(|v| !v.is_empty()) {
            trailers.push((key, present.to_string()));
        }
    }
    let mut subject = format!("anvil: {}", one_line(&meta.command));
    if !meta.artifact_kind.is_empty() {
        subject.push(' ');
        subject.push_str(&one_line(&meta.artifact_kind));
    }
    let body: String = trailers
        .iter()
        .map(|(key, value)| format!("{}: {}\n", key, one_line(value)))
        .collect();
    format!("{}\n\n{}", subject, body)
}

/// The subject and every trailer value are one line. A newline in a value
/// would FORGE a trailer, and these values arrive from an RPC — so this is a
/// boundary, not a formality.
fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// The trailer block of a rendered commit object (`git cat-file -p <sha>`):
/// the final contiguous run of `Key: value` lines in the message body.
///
/// Duplicates are returned in order rather than collapsed into a map, so a
/// key emitted twice is visible to the caller instead of silently winning.
pub fn parse_trailers(commit_object: &str) -> Vec<(String, String)> {
    let body = match commit_object.split_once("\n\n") {
        Some((_headers, body)) => body,
        None => commit_object,
    };
    let lines: Vec<&str> = body.lines().collect();
    let mut start = lines.len();
    while start > 0 && is_trailer_line(lines[start - 1]) {
        start -= 1;
    }
    lines[start..]
        .iter()
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// One trailer's value, or `None`. First occurrence wins, matching the order
/// [`parse_trailers`] returns.
pub fn trailer_value(commit_object: &str, key: &str) -> Option<String> {
    parse_trailers(commit_object)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, value)| value)
}

fn is_trailer_line(line: &str) -> bool {
    match line.split_once(": ") {
        Some((key, _)) => {
            !key.is_empty()
                && key.starts_with(|c: char| c.is_ascii_alphabetic())
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        }
        None => false,
    }
}
