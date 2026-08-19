//! Filesystem adapter for the durable, redacted routing-activity sink.
//!
//! Persists one JSON object per line to `<hearth>/routing-activity.jsonl`. Each
//! line carries ONLY the allowlisted fields — `kind`, `outcome`, `at`, and
//! optional hashed/labelled correlation keys. The append is open-with-append
//! (O_APPEND), which is atomic per write for the small single-line records this
//! sink produces. Reads parse every line in append order; a missing file reads
//! as an empty stream.
//!
//! The JSON is hand-rendered (and hand-parsed) over the three known string
//! fields rather than pulling a serde dependency into the hot path — this keeps
//! the redaction guarantee auditable (only allowlisted fields are ever written)
//! and the adapter dependency-free.

use anvil_core::ports::routing_activity_port::{
    RoutingActivityError, RoutingActivityReadPort, RoutingActivityRecord, RoutingActivityWritePort,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "routing-activity.jsonl";

pub struct FileSystemRoutingActivityAdapter {
    sink_path: PathBuf,
}

impl FileSystemRoutingActivityAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
        }
    }
}

/// Escape the minimal set of JSON string characters. The values written here
/// are machine-controlled (kind/outcome are identifiers/labels, `at` is an
/// ISO-8601 timestamp), but escaping keeps the line well-formed regardless.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Extract one string field's value from a flat single-line JSON object.
/// Returns the unescaped value, or `None` if the key is absent.
fn extract_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    // Find the closing unescaped quote.
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(esc) = chars.next() {
                    match esc {
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        other => value.push(other),
                    }
                }
            }
            '"' => return Some(value),
            other => value.push(other),
        }
    }
    None
}

impl RoutingActivityWritePort for FileSystemRoutingActivityAdapter {
    fn append_routing_activity(
        &self,
        record: &RoutingActivityRecord,
    ) -> Result<(), RoutingActivityError> {
        let mut line = format!(
            "{{\"kind\":\"{}\",\"outcome\":\"{}\",\"at\":\"{}\"",
            json_escape(&record.kind),
            json_escape(&record.outcome),
            json_escape(&record.at),
        );
        if let Some(value) = &record.conversation_hash {
            line.push_str(&format!(
                ",\"conversation_hash\":\"{}\"",
                json_escape(value)
            ));
        }
        if let Some(value) = &record.project_label {
            line.push_str(&format!(",\"project_label\":\"{}\"", json_escape(value)));
        }
        line.push_str("}\n");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| RoutingActivityError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| RoutingActivityError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl RoutingActivityReadPort for FileSystemRoutingActivityAdapter {
    fn read_routing_activity(&self) -> Result<Vec<RoutingActivityRecord>, RoutingActivityError> {
        let contents = match std::fs::read_to_string(&self.sink_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(RoutingActivityError::IoError {
                    message: e.to_string(),
                })
            }
        };

        let mut records = Vec::new();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let kind = extract_field(line, "kind").ok_or_else(|| {
                RoutingActivityError::MalformedRecord {
                    message: format!("missing kind in line: {}", line),
                }
            })?;
            let outcome = extract_field(line, "outcome").unwrap_or_default();
            let at = extract_field(line, "at").unwrap_or_default();
            let conversation_hash = extract_field(line, "conversation_hash");
            let project_label = extract_field(line, "project_label");
            records.push(RoutingActivityRecord {
                kind,
                outcome,
                at,
                conversation_hash,
                project_label,
            });
        }
        Ok(records)
    }
}
