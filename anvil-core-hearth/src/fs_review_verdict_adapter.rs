//! Filesystem adapter for the durable, redacted review-verdict sink.
//!
//! Persists one JSON object per line to `<hearth>/review-verdict.jsonl`. Each
//! line carries only allowlisted public labels, the light structured verdict,
//! timestamp, and optional hashed/labelled join keys. The append is
//! open-with-append (O_APPEND), atomic per small single-line record. A missing
//! file reads as an empty stream.

use crate::artifact_kind_field::read_artifact_kind;
use anvil_core::ports::review_verdict_port::{
    ReviewVerdictError, ReviewVerdictReadPort, ReviewVerdictRecord, ReviewVerdictWritePort,
    VerdictFinding,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "review-verdict.jsonl";

pub struct FileSystemReviewVerdictAdapter {
    sink_path: PathBuf,
}

impl FileSystemReviewVerdictAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
        }
    }
}

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

/// Read a key that MOVED: the canonical name first, the retired name second.
///
/// This is a MIGRATION READ, not an alias. Nothing writes `legacy` any more —
/// the canonical-keys contract asserts that the production writers emit
/// the canonical key and no retired key at all. But ~316,000 rows were written
/// before the rename and no migration has rewritten them, and
/// the schema-migration contract forbids rewriting them in place. A
/// reader that looked only for the canonical key would silently resolve `""`
/// for every one of those rows and every historical fold would quietly lose
/// them. Retired by C-d.2, which migrates the rows; classified in
/// `residual-tokens.allowlist.yaml` as `legacy_adapter` until then.
fn extract_migrated_field(line: &str, canonical: &str, legacy: &str) -> Option<String> {
    extract_field(line, canonical).or_else(|| extract_field(line, legacy))
}

fn extract_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
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

fn extract_bool(line: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

impl ReviewVerdictWritePort for FileSystemReviewVerdictAdapter {
    fn append_review_verdict(
        &self,
        record: &ReviewVerdictRecord,
    ) -> Result<(), ReviewVerdictError> {
        let mut line = format!(
            "{{\"kind\":\"{}\",\"artifact_kind\":\"{}\"",
            json_escape(&record.kind),
            json_escape(&record.artifact_kind),
        );
        if let Some(value) = &record.playbook_run_id {
            line.push_str(&format!(
                ",\"playbook_run_id\":\"{}\"",
                json_escape(value)
            ));
        }
        line.push_str(&format!(
            ",\"gate_state\":\"{}\",\"satisfaction\":\"{}\",\"is_final_gate\":{}",
            json_escape(&record.gate_state),
            json_escape(&record.satisfaction),
            record.is_final_gate,
        ));
        line.push_str(",\"findings\":[");
        for (i, finding) in record.findings.iter().enumerate() {
            if i > 0 {
                line.push(',');
            }
            line.push_str(&format!(
                "{{\"dimension\":\"{}\",\"severity\":\"{}\"",
                json_escape(&finding.dimension),
                json_escape(&finding.severity),
            ));
            // origin_phase is additive: emitted only when the finder attributed
            // one, so existing (empty) findings lines are byte-for-byte unchanged.
            if let Some(origin_phase) = &finding.origin_phase {
                line.push_str(&format!(
                    ",\"origin_phase\":\"{}\"",
                    json_escape(origin_phase)
                ));
            }
            line.push('}');
        }
        line.push(']');
        match &record.intent_confidence {
            Some(value) => line.push_str(&format!(
                ",\"intent_confidence\":\"{}\"",
                json_escape(value)
            )),
            None => line.push_str(",\"intent_confidence\":null"),
        }
        line.push_str(&format!(
            ",\"outcome\":\"{}\",\"at\":\"{}\"",
            json_escape(&record.outcome),
            json_escape(&record.at),
        ));
        if let Some(value) = &record.conversation_hash {
            line.push_str(&format!(
                ",\"conversation_hash\":\"{}\"",
                json_escape(value)
            ));
        }
        if let Some(value) = &record.project_label {
            line.push_str(&format!(",\"project_label\":\"{}\"", json_escape(value)));
        }
        // playbook_version is additive: emitted only when the machine content
        // hash was resolvable, so existing lines are byte-for-byte unchanged and
        // older lines (which lack it) read back as None.
        if let Some(value) = &record.playbook_version {
            line.push_str(&format!(",\"playbook_version\":\"{}\"", json_escape(value)));
        }
        line.push_str("}\n");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| ReviewVerdictError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| ReviewVerdictError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl ReviewVerdictReadPort for FileSystemReviewVerdictAdapter {
    fn read_review_verdicts(&self) -> Result<Vec<ReviewVerdictRecord>, ReviewVerdictError> {
        let contents = match std::fs::read_to_string(&self.sink_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ReviewVerdictError::IoError {
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
            let kind =
                extract_field(line, "kind").ok_or_else(|| ReviewVerdictError::MalformedRecord {
                    message: format!("missing kind in line: {}", line),
                })?;
            // The governed artifact kind, whichever spelling this row uses.
            // Both live on disk permanently (existing rows are never rewritten);
            // a row carrying both is refused, not silently resolved.
            let kind_read = read_artifact_kind(line, extract_field);
            if kind_read.is_ambiguous() {
                return Err(ReviewVerdictError::MalformedRecord {
                    message: kind_read.ambiguity_message(line),
                });
            }
            records.push(ReviewVerdictRecord {
                kind,
                artifact_kind: kind_read.or_empty(),
                playbook_run_id: extract_migrated_field(line, "playbook_run_id", "workflow_instance_id"),
                gate_state: extract_field(line, "gate_state").unwrap_or_default(),
                satisfaction: extract_field(line, "satisfaction").unwrap_or_default(),
                is_final_gate: extract_bool(line, "is_final_gate").unwrap_or(false),
                // Findings are not reconstructed by the read path (empty in this
                // phase; downstream jsonl consumers read the raw line).
                findings: Vec::<VerdictFinding>::new(),
                intent_confidence: extract_field(line, "intent_confidence"),
                outcome: extract_field(line, "outcome").unwrap_or_default(),
                at: extract_field(line, "at").unwrap_or_default(),
                conversation_hash: extract_field(line, "conversation_hash"),
                project_label: extract_field(line, "project_label"),
                // Additive: absent on older lines → None (serde(default) parity).
                playbook_version: extract_field(line, "playbook_version"),
            });
        }
        Ok(records)
    }
}
