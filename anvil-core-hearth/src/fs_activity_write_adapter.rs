//! Filesystem implementation of `ActivityWritePort`.
//!
//! Appends a begin-marker to the artifact's status.yaml `activity:` block
//! via a **textual line-level edit** — it deliberately does NOT
//! re-serialize `FullStatusYaml`, which would reorder or re-format the
//! `actors:` and `transitions:` sections (F-7). The write mirrors the
//! line-level edit discipline in `fs_actor_write_adapter.rs`: read the raw
//! YAML, locate (or create) the `activity:` key, append the new entry
//! lines as the last sibling, and write back atomically.

use anvil_core::domain::shared_types::ActivityEntry;
use anvil_core::ports::activity_write_port::{ActivityWriteError, ActivityWritePort};
use std::path::PathBuf;

pub struct FileSystemActivityWriteAdapter {
    hearth_path: PathBuf,
}

impl FileSystemActivityWriteAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl ActivityWritePort for FileSystemActivityWriteAdapter {
    fn append_activity(
        &self,
        artifact_path: &str,
        entry: &ActivityEntry,
    ) -> Result<(), ActivityWriteError> {
        let status_path = self.hearth_path.join(artifact_path).join("status.yaml");
        if !status_path.exists() {
            return Err(ActivityWriteError::NotFound {
                artifact_path: artifact_path.to_string(),
            });
        }
        let content =
            std::fs::read_to_string(&status_path).map_err(|e| ActivityWriteError::IoError {
                message: format!("Failed to read status.yaml: {}", e),
            })?;

        let new_content = append_activity_entry(&content, entry);

        crate::atomic_write::atomic_write(&status_path, new_content.as_bytes()).map_err(
            |e| ActivityWriteError::IoError {
                message: format!("Failed to write status.yaml: {}", e),
            },
        )
    }
}

/// Append one `activity:` entry to the raw status.yaml text. Creates the
/// `activity:` key (at end of file) when absent; otherwise inserts the new
/// entry as the last sibling under the existing `activity:` block. All
/// other sections are left byte-for-byte unchanged.
fn append_activity_entry(content: &str, entry: &ActivityEntry) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let block = build_activity_entry(entry);

    match lines.iter().position(|l| is_activity_header(l)) {
        Some(activity_idx) => {
            // Normalize inline-empty shapes (`activity: []` / `activity: null`)
            // into block style before inserting.
            let value_after = lines[activity_idx]
                .trim_start()
                .strip_prefix("activity:")
                .map(|s| s.trim())
                .unwrap_or("");
            if !value_after.is_empty() {
                lines[activity_idx] = "activity:".to_string();
            }
            // Insert just before the next top-level (non-indented, non-empty)
            // line, keeping the new entry as the last sibling of the block.
            let mut insert_at = lines.len();
            for j in (activity_idx + 1)..lines.len() {
                if !lines[j].starts_with(' ') && !lines[j].is_empty() {
                    insert_at = j;
                    break;
                }
            }
            for (k, line) in block.into_iter().enumerate() {
                lines.insert(insert_at + k, line);
            }
        }
        None => {
            // No activity block present — create one at end of file.
            lines.push("activity:".to_string());
            lines.extend(block);
        }
    }

    ensure_trailing_newline(lines.join("\n"))
}

/// Render one `activity:` list entry. Quotes `at` to match the on-disk
/// idiom used for transition timestamps. `conversation_id` (resume-aware
/// routing) is rendered only when non-empty so existing-shape markers (no
/// conversation) stay byte-identical to the pre-track output; a present
/// conversation_id is quoted to survive arbitrary id text.
fn build_activity_entry(entry: &ActivityEntry) -> Vec<String> {
    let mut lines = vec![
        format!("  - kind: {}", entry.kind),
        format!("    actor: {}", entry.actor),
        format!("    state: {}", entry.state),
        format!("    at: \"{}\"", entry.at),
    ];
    if !entry.conversation_id.is_empty() {
        lines.push(format!(
            "    conversation_id: \"{}\"",
            entry.conversation_id
        ));
    }
    lines
}

/// `activity:` line in any legal YAML shape (block-style header, inline
/// empty sequence `activity: []`, or explicit null `activity: null`/`~`).
fn is_activity_header(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("activity:") {
        return false;
    }
    let after = &trimmed["activity:".len()..];
    if after.is_empty() {
        return true;
    }
    let first = after.chars().next().unwrap();
    matches!(first, ' ' | '\t')
}

fn ensure_trailing_newline(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}
