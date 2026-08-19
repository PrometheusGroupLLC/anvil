//! Filesystem adapter for the durable, redacted per-turn DELIVERY sink.
//!
//! Persists one JSON object per line to `<hearth>/delivery-log.jsonl`. Each
//! line carries only `DELIVERY_RECORD_KEYS`. The append is open-with-append
//! (O_APPEND) and a SINGLE `write_all` of a buffer that already ends in `\n`.
//!
//! `writeln!` is forbidden on this path. It is not one syscall — `write_fmt`
//! can flush the payload and the newline separately, so two hook processes
//! appending concurrently interleave and produce GLUED JSON (`{...}{...}` on
//! one line), which breaks every reader of this log. That defect has been found
//! and fixed twice in this repository — `abstention_ledger.rs` and the hook's
//! own inline writer — and was observed for real on line 110 of the live sink.
//!
//! The JSON is hand-rendered on the write path, like every other sink adapter
//! here: it keeps the redaction guarantee auditable, because the set of keys
//! that can ever be written is the literal in one format string.
//!
//! The READ path is the asymmetry, and it is deliberate. It parses with
//! `serde_json`, because it must answer two questions a hand-rolled field
//! scanner cannot: "is this line exactly one JSON object" (the glued-line
//! defect) and "which keys does this line carry" (the allowlist proof). A
//! hand-rolled `find("\"key\":\"")` scanner reports a glued line as a valid
//! record and cannot enumerate keys at all.

use anvil_core::domain::telemetry_salt::UNKNOWN_CONVERSATION_HASH;
use anvil_core::ports::delivery_log_port::{
    project_delivery_record, DeliveryLogError, DeliveryLogReadPort, DeliveryLogRecord,
    DeliveryLogScan, DeliveryLogWritePort, DeliveryObservation,
};
use std::io::Write;
use std::path::{Path, PathBuf};

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

fn string_field(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    map.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn u64_field(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> u64 {
    map.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

fn bool_field(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    map.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

pub const SINK_FILENAME: &str = "delivery-log.jsonl";

pub struct FileSystemDeliveryLogAdapter {
    sink_path: PathBuf,
}

impl FileSystemDeliveryLogAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
        }
    }
}

impl DeliveryLogWritePort for FileSystemDeliveryLogAdapter {
    fn append_delivery_log(&self, record: &DeliveryLogRecord) -> Result<(), DeliveryLogError> {
        // The key set of this format string IS the allowlist. There is no
        // branch that can add a key, which is what makes "every key on the
        // line is on DELIVERY_RECORD_KEYS" a property of the writer rather
        // than of the caller.
        let mut line = format!(
            "{{\"at\":\"{}\",\"source\":\"{}\",\"conversation_hash\":\"{}\",\"project_label\":\"{}\",\"guidance_kind\":\"{}\",\"engine_candidates\":{},\"guidance_produced\":{},\"guidance_bytes\":{},\"outcome\":\"{}\",\"resume_source\":\"{}\",\"router_cause\":\"{}\"}}",
            json_escape(&record.at),
            json_escape(&record.source),
            json_escape(&record.conversation_hash),
            json_escape(&record.project_label),
            json_escape(&record.guidance_kind),
            record.engine_candidates,
            record.guidance_produced,
            record.guidance_bytes,
            json_escape(&record.outcome),
            json_escape(&record.resume_source),
            json_escape(&record.router_cause),
        );
        // ONE buffer, already newline-terminated, ONE `write_all`. See the
        // module doc: `writeln!` here is the glued-JSON defect.
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| DeliveryLogError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| DeliveryLogError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl DeliveryLogReadPort for FileSystemDeliveryLogAdapter {
    fn read_delivery_log(&self) -> Result<DeliveryLogScan, DeliveryLogError> {
        let contents = match std::fs::read_to_string(&self.sink_path) {
            Ok(c) => c,
            // A missing sink is an empty stream, not an error. A fresh hearth
            // simply has no rows.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DeliveryLogScan::default()),
            Err(e) => {
                return Err(DeliveryLogError::IoError {
                    message: e.to_string(),
                })
            }
        };

        let mut scan = DeliveryLogScan::default();
        for (index, raw) in contents.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let map = match serde_json::from_str::<serde_json::Value>(line) {
                Ok(serde_json::Value::Object(map)) => map,
                // COUNTED, never dropped. `filter_map`ing a parse error away is
                // how a coverage number improves by losing its own denominator,
                // and a glued line (live line 110) is the precedent. The message
                // carries the POSITION and nothing else: this sink's
                // pre-migration rows hold raw conversation ids, so echoing the
                // offending line would leak the one value the reader exists to
                // keep out.
                _ => {
                    scan.read_defects += 1;
                    scan.defect_messages.push(format!(
                        "line {} is not exactly one JSON object",
                        index + 1
                    ));
                    continue;
                }
            };

            // THE MIGRATION READ. A row with no `conversation_hash` key was
            // written before the hash existed. It reads as the sentinel and is
            // flagged, so the fold can bucket it as `PreMigrationRow` INSIDE
            // the denominator. The raw `conversation_id` such a row carries is
            // never read, never hashed (there is no salt in a read path, and
            // guessing one produces a disjoint keyspace with no visible cause)
            // and has no field on the record to survive into.
            //
            // A row that DOES carry the key, even when its value is the
            // sentinel, is post-migration: that is an engine that could not
            // answer, which is a different fact and a different bucket.
            let pre_migration = !map.contains_key("conversation_hash");
            let conversation_hash = if pre_migration {
                UNKNOWN_CONVERSATION_HASH.to_string()
            } else {
                let value = string_field(&map, "conversation_hash");
                if value.is_empty() {
                    UNKNOWN_CONVERSATION_HASH.to_string()
                } else {
                    value
                }
            };

            // Every absent field is its documented default. The oldest live
            // rows carry no `resume_source` at all; they parse unchanged.
            let at = string_field(&map, "at");
            let source = string_field(&map, "source");
            let project_label = string_field(&map, "project_label");
            let guidance_kind = string_field(&map, "guidance_kind");
            let outcome = string_field(&map, "outcome");
            let resume_source = string_field(&map, "resume_source");
            let router_cause = string_field(&map, "router_cause");
            let observation = DeliveryObservation {
                at: &at,
                source: &source,
                // Already a label on disk. The projection's basename rule is
                // idempotent over a bare basename, so routing the read back
                // through the ONE constructor costs nothing and keeps the
                // record's single-constructor guarantee true on both paths.
                project_root: &project_label,
                engine_conversation_hash: &conversation_hash,
                guidance_kind: &guidance_kind,
                engine_candidates: u64_field(&map, "engine_candidates"),
                guidance_produced: bool_field(&map, "guidance_produced"),
                guidance_bytes: u64_field(&map, "guidance_bytes"),
                outcome: &outcome,
                resume_source: &resume_source,
                // Absent on every row written before this key existed, and the
                // empty string is its documented default — the same tolerance
                // the oldest rows get for `resume_source`. A missing key here
                // must never become a read defect.
                router_cause: &router_cause,
            };
            let mut record = project_delivery_record(&observation);
            record.pre_migration = pre_migration;
            scan.records.push(record);
        }
        Ok(scan)
    }
}
