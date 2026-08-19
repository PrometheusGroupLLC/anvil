//! Filesystem adapter for the durable, redacted transition-measurement sink.
//!
//! Persists one JSON object per line to `<hearth>/transition-measurement.jsonl`.
//! Each line carries only the allowlisted transition labels, outcome/success,
//! timestamp, optional review satisfaction, and optional hashed/labelled join
//! keys. The append is open-with-append (O_APPEND), atomic per small single-line
//! record. A missing file reads as an empty stream.

use crate::artifact_kind_field::read_artifact_kind;
use anvil_core::ports::transition_measurement_port::{
    TransitionMeasurementError, TransitionMeasurementReadPort, TransitionMeasurementRecord,
    TransitionMeasurementWritePort,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "transition-measurement.jsonl";

pub struct FileSystemTransitionMeasurementAdapter {
    sink_path: PathBuf,
}

impl FileSystemTransitionMeasurementAdapter {
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

impl TransitionMeasurementWritePort for FileSystemTransitionMeasurementAdapter {
    fn append_transition_measurement(
        &self,
        record: &TransitionMeasurementRecord,
    ) -> Result<(), TransitionMeasurementError> {
        let mut line = format!(
            "{{\"kind\":\"{}\",\"artifact_kind\":\"{}\",\"from_state\":\"{}\",\"to_state\":\"{}\",\"role\":\"{}\"",
            json_escape(&record.kind),
            json_escape(&record.artifact_kind),
            json_escape(&record.from_state),
            json_escape(&record.to_state),
            json_escape(&record.role),
        );
        if let Some(value) = &record.satisfaction {
            line.push_str(&format!(",\"satisfaction\":\"{}\"", json_escape(value)));
        }
        line.push_str(&format!(
            ",\"outcome\":\"{}\",\"success\":{},\"at\":\"{}\"",
            json_escape(&record.outcome),
            record.success,
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
        if let Some(value) = &record.playbook_run_id {
            line.push_str(&format!(
                ",\"playbook_run_id\":\"{}\"",
                json_escape(value)
            ));
        }
        // Omitted entirely when absent, so a row from before these fields is
        // byte-identical to one written now with nothing to say.
        if let Some(value) = &record.claimed_evidence_status {
            line.push_str(&format!(
                ",\"claimed_evidence_status\":\"{}\"",
                json_escape(value)
            ));
        }
        if let Some(value) = &record.artifact_assessment {
            line.push_str(&format!(
                ",\"artifact_assessment\":\"{}\"",
                json_escape(value)
            ));
        }
        line.push_str("}\n");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| TransitionMeasurementError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| TransitionMeasurementError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl TransitionMeasurementReadPort for FileSystemTransitionMeasurementAdapter {
    fn read_transition_measurements(
        &self,
    ) -> Result<Vec<TransitionMeasurementRecord>, TransitionMeasurementError> {
        let contents = match std::fs::read_to_string(&self.sink_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(TransitionMeasurementError::IoError {
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
                TransitionMeasurementError::MalformedRecord {
                    message: format!("missing kind in line: {}", line),
                }
            })?;
            // The governed artifact kind, whichever spelling this row uses. A row
            // carrying BOTH is refused, not silently resolved.
            let kind_read = read_artifact_kind(line, extract_field);
            if kind_read.is_ambiguous() {
                return Err(TransitionMeasurementError::MalformedRecord {
                    message: kind_read.ambiguity_message(line),
                });
            }
            records.push(TransitionMeasurementRecord {
                kind,
                artifact_kind: kind_read.or_empty(),
                from_state: extract_field(line, "from_state").unwrap_or_default(),
                to_state: extract_field(line, "to_state").unwrap_or_default(),
                role: extract_field(line, "role").unwrap_or_default(),
                satisfaction: extract_field(line, "satisfaction"),
                outcome: extract_field(line, "outcome").unwrap_or_default(),
                success: extract_bool(line, "success").unwrap_or(false),
                at: extract_field(line, "at").unwrap_or_default(),
                conversation_hash: extract_field(line, "conversation_hash"),
                project_label: extract_field(line, "project_label"),
                playbook_run_id: extract_migrated_field(line, "playbook_run_id", "workflow_instance_id"),
                // Optional on read: every record written before these fields
                // existed decodes unchanged, which is the whole point of
                // appending rather than versioning the schema.
                claimed_evidence_status: extract_field(line, "claimed_evidence_status"),
                artifact_assessment: extract_field(line, "artifact_assessment"),
            });
        }
        Ok(records)
    }
}
