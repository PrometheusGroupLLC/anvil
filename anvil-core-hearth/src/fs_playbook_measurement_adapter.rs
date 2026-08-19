//! Filesystem adapter for the durable, redacted playbook-measurement sink.
//!
//! Persists one JSON object per line to `<hearth>/playbook-measurement.jsonl`.
//! Rows written before the rename live in `<hearth>/workflow-measurement.jsonl`;
//! the read path concatenates that file FIRST (it is strictly older) so no
//! history is lost, and nothing ever writes it again. C-d.2 retires it by
//! migrating the rows; the schema-migration contract forbids renaming
//! or rewriting a sink file in place, which is why the legacy file is read
//! rather than moved.
//! Each line carries only the allowlisted playbook labels, terminal outcome,
//! timestamp, and optional hashed/labelled join keys. The append is
//! open-with-append (O_APPEND), atomic per small single-line record. A missing
//! file reads as an empty stream.

use crate::artifact_kind_field::read_artifact_kind;
use anvil_core::ports::playbook_measurement_port::{
    QualitySignal, PlaybookMeasurementError, PlaybookMeasurementReadPort,
    PlaybookMeasurementRecord, PlaybookMeasurementWritePort,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "playbook-measurement.jsonl";

/// The pre-rename sink file. READ ONLY — never written, never renamed.
pub const LEGACY_SINK_FILENAME: &str = "workflow-measurement.jsonl";

pub struct FileSystemPlaybookMeasurementAdapter {
    sink_path: PathBuf,
    legacy_sink_path: PathBuf,
}

impl FileSystemPlaybookMeasurementAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
            legacy_sink_path: hearth_path.join(LEGACY_SINK_FILENAME),
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

impl PlaybookMeasurementWritePort for FileSystemPlaybookMeasurementAdapter {
    fn append_playbook_measurement(
        &self,
        record: &PlaybookMeasurementRecord,
    ) -> Result<(), PlaybookMeasurementError> {
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
            ",\"terminal_state\":\"{}\",\"terminal_reached\":{},\"outcome\":\"{}\",\"success\":{},\"at\":\"{}\"",
            json_escape(&record.terminal_state),
            record.terminal_reached,
            json_escape(&record.outcome),
            record.success,
            json_escape(&record.at),
        ));
        // Quality vector (additive; distinct from the `success` completion
        // floor). Scores are empty/None in this phase — the shape is emitted so
        // downstream consumers can rely on the fields existing.
        line.push_str(",\"quality_dimension_scores\":[");
        for (i, ds) in record.quality_dimension_scores.iter().enumerate() {
            if i > 0 {
                line.push(',');
            }
            line.push_str(&format!(
                "{{\"dimension\":\"{}\",\"score\":{}}}",
                json_escape(&ds.dimension),
                ds.score
            ));
        }
        line.push(']');
        match record.quality_overall {
            Some(value) => line.push_str(&format!(",\"quality_overall\":{}", value)),
            None => line.push_str(",\"quality_overall\":null"),
        }
        match &record.quality_grader {
            Some(value) => {
                line.push_str(&format!(",\"quality_grader\":\"{}\"", json_escape(value)))
            }
            None => line.push_str(",\"quality_grader\":null"),
        }
        line.push_str(&format!(
            ",\"quality_signal\":\"{}\"",
            record.quality_signal.as_str()
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
            .map_err(|e| PlaybookMeasurementError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| PlaybookMeasurementError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl PlaybookMeasurementReadPort for FileSystemPlaybookMeasurementAdapter {
    fn read_playbook_measurements(
        &self,
    ) -> Result<Vec<PlaybookMeasurementRecord>, PlaybookMeasurementError> {
        // Legacy rows first: that file stopped growing at the rename, so its
        // rows all predate every row in the canonical file, and concatenating
        // in this order keeps the stream chronological.
        let mut contents = String::new();
        for path in [&self.legacy_sink_path, &self.sink_path] {
            match std::fs::read_to_string(path) {
                Ok(c) => {
                    contents.push_str(&c);
                    if !c.ends_with('\n') && !c.is_empty() {
                        contents.push('\n');
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(PlaybookMeasurementError::IoError {
                        message: e.to_string(),
                    })
                }
            }
        }

        let mut records = Vec::new();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let kind = extract_field(line, "kind").ok_or_else(|| {
                PlaybookMeasurementError::MalformedRecord {
                    message: format!("missing kind in line: {}", line),
                }
            })?;
            // The governed artifact kind, whichever spelling this row uses.
            // Both live on disk permanently (existing rows are never rewritten);
            // a row carrying both is refused, not silently resolved.
            let kind_read = read_artifact_kind(line, extract_field);
            if kind_read.is_ambiguous() {
                return Err(PlaybookMeasurementError::MalformedRecord {
                    message: kind_read.ambiguity_message(line),
                });
            }
            records.push(PlaybookMeasurementRecord {
                kind,
                artifact_kind: kind_read.or_empty(),
                playbook_run_id: extract_migrated_field(line, "playbook_run_id", "workflow_instance_id"),
                terminal_state: extract_field(line, "terminal_state").unwrap_or_default(),
                terminal_reached: extract_bool(line, "terminal_reached").unwrap_or(false),
                outcome: extract_field(line, "outcome").unwrap_or_default(),
                success: extract_bool(line, "success").unwrap_or(false),
                // Quality vector: per-dimension scores are not reconstructed by
                // the read path (only the engine's dedup + downstream jsonl
                // consumers read this sink; both need the labels, not the empty
                // score vector). `quality_grader` is a string-or-null; a null
                // yields None via `extract_field`. `quality_signal` defaults to
                // Leading when absent (older records) or unrecognised.
                quality_dimension_scores: Vec::new(),
                quality_overall: None,
                quality_grader: extract_field(line, "quality_grader"),
                quality_signal: match extract_field(line, "quality_signal").as_deref() {
                    Some("lagging") => QualitySignal::Lagging,
                    _ => QualitySignal::Leading,
                },
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
