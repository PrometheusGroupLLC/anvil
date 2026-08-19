//! Filesystem adapter for the temper-consumed full §0 step-measurement stream.
//!
//! Writes one JSON object per line to
//! `<temper_home>/.temper/step-measurements/<workflow_kind>/events.jsonl`. The
//! file is append-only, never rewritten; partitioned by `workflow_kind`. Each
//! line carries the full §0 field set plus a stable `event_id` the sink dedupes
//! on: a line whose `event_id` already exists in the partition file is a no-op
//! (idempotent replay).
//!
//! The JSON is hand-rendered over the known fields (no serde in the hot path),
//! which keeps the privacy guarantee auditable — only these fields are ever
//! written, and `tokens`/`duration_ms` keys are OMITTED (not blanked) when the
//! policy redacts them or the runtime did not surface usage.

use anvil_core::ports::step_measurement_stream_port::{
    Step0Event, Step0StreamError, Step0StreamWritePort,
};
use std::io::Write;
use std::path::PathBuf;

/// The temper storage root segment under the resolved temper home.
pub const TEMPER_DIR: &str = ".temper";
/// The per-kind partition root.
pub const STREAM_SUBDIR: &str = "step-measurements";
/// The per-kind event file.
pub const EVENTS_FILENAME: &str = "events.jsonl";

pub struct FileSystemStep0StreamAdapter {
    /// The resolved temper home (e.g. `$HOME` or `$ANVIL_TEMPER_HOME`). The
    /// stream is rooted at `<temper_home>/.temper/step-measurements/`.
    temper_home: PathBuf,
}

impl FileSystemStep0StreamAdapter {
    pub fn new(temper_home: PathBuf) -> Self {
        Self { temper_home }
    }

    /// The per-kind partition directory.
    fn partition_dir(&self, kind: &str) -> PathBuf {
        self.temper_home
            .join(TEMPER_DIR)
            .join(STREAM_SUBDIR)
            .join(kind)
    }

    /// The per-kind events file.
    pub fn events_path(&self, kind: &str) -> PathBuf {
        self.partition_dir(kind).join(EVENTS_FILENAME)
    }
}

/// Escape the minimal set of JSON string characters. Intent/expected_output are
/// machine-author prose; escaping keeps every line well-formed.
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

/// Render one §0 event as a single JSON line. `tokens`/`duration_ms` keys are
/// OMITTED when `None` (policy-redacted or no usage surfaced). The line carries
/// the stable `event_id` for sink-side dedupe.
fn render_line(event: &Step0Event) -> String {
    let mut fields = vec![
        format!("\"event_id\":\"{}\"", json_escape(&event.event_id())),
        // Serialized key stays `workflow_id`: this stream is temper's ingest
        // contract (step_measurement_ingest) — renaming the key is the DEFERRED
        // temper-coordinated measurement-contract stage.
        format!("\"workflow_id\":\"{}\"", json_escape(&event.playbook_id)),
        format!("\"track_id\":\"{}\"", json_escape(&event.track_id)),
        format!("\"from_state\":\"{}\"", json_escape(&event.from_state)),
        format!("\"to_state\":\"{}\"", json_escape(&event.to_state)),
        format!("\"role\":\"{}\"", json_escape(&event.role)),
        format!("\"actor\":\"{}\"", json_escape(&event.actor)),
        format!("\"intent\":\"{}\"", json_escape(&event.intent)),
        format!(
            "\"expected_output\":\"{}\"",
            json_escape(&event.expected_output)
        ),
        format!("\"at\":\"{}\"", json_escape(&event.at)),
        format!("\"model\":\"{}\"", json_escape(&event.model)),
    ];
    if let Some(tokens) = event.tokens {
        fields.push(format!("\"tokens\":{}", tokens));
    }
    if let Some(duration_ms) = event.duration_ms {
        fields.push(format!("\"duration_ms\":{}", duration_ms));
    }
    format!("{{{}}}\n", fields.join(","))
}

/// Whether the file at `path` already contains a line carrying this `event_id`.
/// A missing file ⇒ no duplicate. The id is matched as the `"event_id":"<id>"`
/// token so a substring of another field can't false-match.
///
/// **C-d.1 round 8, H-3.** This returned `bool` and mapped every read error onto
/// `false` — *"no duplicate"* — so an unreadable stream file made the idempotence
/// guard answer "go ahead" and the caller appended a second copy of an event it
/// already holds. `Err(_) => false` is the swallow with the answer inverted: the
/// permissive value, not the empty one. An ABSENT file genuinely holds no
/// duplicate and stays an answer; everything else refuses.
fn event_id_present(path: &std::path::Path, event_id: &str) -> Result<bool, Step0StreamError> {
    let needle = format!("\"event_id\":\"{}\"", json_escape(event_id));
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents.lines().any(|line| line.contains(&needle))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Step0StreamError::IoError {
            message: format!(
                "step0_stream_uninspectable: {} exists and could not be read: {e}. Refusing to \
                 append. Reading it as 'no duplicate' appends a second copy of an event the \
                 stream already holds, and every consumer counts it twice.",
                path.display()
            ),
        }),
    }
}

impl Step0StreamWritePort for FileSystemStep0StreamAdapter {
    fn append_step0_event(&self, event: &Step0Event) -> Result<(), Step0StreamError> {
        let dir = self.partition_dir(&event.track_id);
        std::fs::create_dir_all(&dir).map_err(|e| Step0StreamError::IoError {
            message: e.to_string(),
        })?;
        let path = dir.join(EVENTS_FILENAME);

        // Idempotent replay: a line carrying this stable event_id already exists
        // ⇒ no-op (no duplicate event for the same logical transition).
        if event_id_present(&path, &event.event_id())? {
            return Ok(());
        }

        let line = render_line(event);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Step0StreamError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| Step0StreamError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}
