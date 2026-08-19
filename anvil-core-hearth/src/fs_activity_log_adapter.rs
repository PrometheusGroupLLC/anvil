//! Filesystem adapter for the durable, redacted UNIVERSAL activity-log sink.
//!
//! Persists one JSON object per line to `<hearth>/activity-log.jsonl`. Each line
//! carries ONLY the Part-3 allowlisted fields — `command`, `outcome`,
//! `workflow_kind`, the optional salted `actor_hash`, `at`, and optional
//! hashed/labelled correlation keys. The append is open-with-append (O_APPEND),
//! atomic per write for the small single-line records this sink produces. Reads
//! parse every line in append order; a missing file reads as an empty stream.
//!
//! The JSON is hand-rendered (and hand-parsed) over the known fields rather than
//! pulling a serde dependency into the hot path — this keeps the redaction
//! guarantee auditable (only these fields are ever written) and the adapter
//! dependency-free. Mirrors `fs_routing_activity_adapter.rs` exactly.

use crate::artifact_kind_field::read_artifact_kind;
use anvil_core::ports::activity_log_port::{
    ActivityLogError, ActivityLogReadPort, ActivityLogRecord, ActivityLogWritePort,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "activity-log.jsonl";

pub struct FileSystemActivityLogAdapter {
    sink_path: PathBuf,
}

impl FileSystemActivityLogAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
        }
    }
}

/// Escape the minimal set of JSON string characters. The values written here
/// are machine-controlled (labels/identifiers/timestamps), but escaping keeps
/// the line well-formed regardless.
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

impl ActivityLogWritePort for FileSystemActivityLogAdapter {
    fn append_activity_log(&self, record: &ActivityLogRecord) -> Result<(), ActivityLogError> {
        // `actor_hash` is emitted as a JSON string when present, or JSON null
        // when None — so a reader can distinguish "no salt" from a real hash.
        let actor_hash_json = match &record.actor_hash {
            Some(h) => format!("\"{}\"", json_escape(h)),
            None => "null".to_string(),
        };
        let mut line = format!(
            "{{\"command\":\"{}\",\"outcome\":\"{}\",\"artifact_kind\":\"{}\",\"from_state\":\"{}\",\"to_state\":\"{}\",\"actor_hash\":{},\"at\":\"{}\",\"source\":\"{}\"",
            json_escape(&record.command),
            json_escape(&record.outcome),
            json_escape(&record.artifact_kind),
            json_escape(&record.from_state),
            json_escape(&record.to_state),
            actor_hash_json,
            json_escape(&record.at),
            json_escape(&record.source),
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
        if let Some(value) = &record.playbook_run_id {
            line.push_str(&format!(
                ",\"playbook_run_id\":\"{}\"",
                json_escape(value)
            ));
        }
        if let Some(value) = &record.call_state {
            line.push_str(&format!(",\"call_state\":\"{}\"", json_escape(value)));
        }
        line.push_str("}\n");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| ActivityLogError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| ActivityLogError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

/// What a streaming read measured: the retained records in append order, every
/// non-empty line it parsed, and how many it kept. Returned rather than logged,
/// so a caller cannot report a retained count it did not measure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActivityLogScan {
    pub records: Vec<ActivityLogRecord>,
    pub rows_scanned: u64,
    pub rows_retained: u64,
}

impl FileSystemActivityLogAdapter {
    /// The SINGLE parser for this sink, streamed line by line.
    ///
    /// The predicate is a caller-supplied closure evaluated per record as it is
    /// parsed, never a filter baked into the reader: one caller keeps the
    /// join-relevant rows, another a superset, another accumulates and returns
    /// `false` for every row so nothing is materialised at all. A consumer that
    /// needed its own filter would otherwise open its own reader, and a second
    /// parse of a 46 MB sink is both the memory hazard and the way two
    /// instruments come to disagree about the same rows.
    ///
    /// `rows_scanned` and `rows_retained` are RETURNED rather than logged, so a
    /// caller cannot report a count it did not measure.
    pub fn read_activity_log_where(
        &self,
        keep: &dyn Fn(&ActivityLogRecord) -> bool,
    ) -> Result<ActivityLogScan, ActivityLogError> {
        let file = match std::fs::File::open(&self.sink_path) {
            Ok(f) => f,
            // A missing sink reads as an empty stream — a fresh hearth simply
            // has no rows.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ActivityLogScan::default())
            }
            Err(e) => {
                return Err(ActivityLogError::IoError {
                    message: e.to_string(),
                })
            }
        };
        let mut scan = ActivityLogScan::default();
        for line in std::io::BufRead::lines(std::io::BufReader::new(file)) {
            let line = line.map_err(|e| ActivityLogError::IoError {
                message: e.to_string(),
            })?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let record = parse_line(line)?;
            scan.rows_scanned += 1;
            if keep(&record) {
                scan.rows_retained += 1;
                scan.records.push(record);
            }
        }
        Ok(scan)
    }
}

/// Parse one line into a record. Extracted so there is exactly one parser for
/// this sink; every reader on it delegates here.
fn parse_line(line: &str) -> Result<ActivityLogRecord, ActivityLogError> {
    let command =
        extract_field(line, "command").ok_or_else(|| ActivityLogError::MalformedRecord {
            message: format!("missing command in line: {}", line),
        })?;
    let outcome = extract_field(line, "outcome").unwrap_or_default();
    // The governed artifact kind, whichever spelling this row uses. Both live on
    // disk permanently (existing rows are never rewritten); a row carrying both
    // is refused, not silently resolved.
    let kind_read = read_artifact_kind(line, extract_field);
    if kind_read.is_ambiguous() {
        return Err(ActivityLogError::MalformedRecord {
            message: kind_read.ambiguity_message(line),
        });
    }
    let artifact_kind = kind_read.or_empty();
    // Additive: records written before from_state/to_state existed lack these
    // keys; they read as empty strings (no error) — old records parse unchanged.
    let from_state = extract_field(line, "from_state").unwrap_or_default();
    let to_state = extract_field(line, "to_state").unwrap_or_default();
    // `actor_hash` is a string when present, JSON null otherwise. `extract_field`
    // only matches the `"key":"value"` string form, so a null reads as None —
    // exactly the desired fail-safe semantics.
    let actor_hash = extract_field(line, "actor_hash");
    let at = extract_field(line, "at").unwrap_or_default();
    // Additive: records written before `source` existed lack the key; they read
    // as the empty source (no error) and surface under the "unknown" bucket.
    let source = extract_field(line, "source").unwrap_or_default();
    let conversation_hash = extract_field(line, "conversation_hash");
    let project_label = extract_field(line, "project_label");
    let playbook_run_id = extract_migrated_field(line, "playbook_run_id", "workflow_instance_id");
    // Additive: records written before `call_state` existed lack the key; they
    // read as None and fold into the "unknown" bucket.
    //
    // MIGRATION READ, same contract as `extract_migrated_field`: the two route
    // classifications were persisted as `no_workflow` / `mid_workflow` before the
    // ratification. Nothing writes those spellings any more (see
    // `CallState::as_str`), but rows already on disk carry them, and
    // `by_call_state` groups on the raw string — an un-normalised read would
    // split one classification across two buckets and quietly break the coverage
    // ratio. Retired by C-d.2, which migrates the rows.
    let call_state = extract_field(line, "call_state").map(|v| match v.as_str() {
        "no_workflow" => "no_playbook_run".to_string(),
        "mid_workflow" => "mid_playbook_run".to_string(),
        _ => v,
    });
    Ok(ActivityLogRecord {
        command,
        outcome,
        artifact_kind,
        from_state,
        to_state,
        actor_hash,
        at,
        source,
        conversation_hash,
        project_label,
        playbook_run_id,
        call_state,
    })
}

impl ActivityLogReadPort for FileSystemActivityLogAdapter {
    /// Delegates to the streaming reader with a retain-everything closure, so
    /// there is one parser and not two. The shipped behavior is unchanged: every
    /// record in append order.
    fn read_activity_log(&self) -> Result<Vec<ActivityLogRecord>, ActivityLogError> {
        Ok(self.read_activity_log_where(&|_| true)?.records)
    }
}
