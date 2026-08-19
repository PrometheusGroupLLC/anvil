//! Filesystem adapter for the durable, redacted step-measurement sink.
//!
//! Persists one JSON object per line to `<hearth>/step-measurement.jsonl`. Each
//! line carries ONLY the Part-3 allowlisted fields — `kind`, `from_state`,
//! `to_state`, `role`, the BOOLEANS `intent_present` / `expected_output_present`,
//! and `at`, plus optional hashed/labelled correlation keys. Obligated DRIVEN
//! steps add one all-or-nothing evidence extension: status, missing class
//! tokens, ordered opaque claims, and the registry machine's content version.
//! NEVER the intent/expected_output prose, message text, project/artifact paths,
//! identities, or token counts. The append is open-with-append (O_APPEND), which
//! is atomic per write for the small single-line records this sink produces.
//! Reads parse every line in append order; a missing file reads as an empty
//! stream.
//!
//! The JSON writer is hand-rendered over the known allowlist so the redaction
//! guarantee stays auditable. The read side uses strict `serde_json` parsing for
//! the nested evidence arrays and treats a partial/malformed four-field evidence
//! extension as a malformed record. Legacy rows omit all four keys and still
//! read with `evidence: None`.

use anvil_core::domain::playbook::evidence_obligation::{EvidenceAssessment, EvidenceAssessmentStatus};
use anvil_core::domain::playbook::types::EvidenceClass;
use anvil_core::domain::shared_types::ClaimedEvidence;
use crate::artifact_kind_field::read_artifact_kind;
use anvil_core::ports::step_measurement_port::{
    StepEvidenceRecord, StepMeasurementError, StepMeasurementReadPort, StepMeasurementRecord,
    StepMeasurementWritePort,
};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SINK_FILENAME: &str = "step-measurement.jsonl";

pub struct FileSystemStepMeasurementAdapter {
    sink_path: PathBuf,
}

impl FileSystemStepMeasurementAdapter {
    pub fn new(hearth_path: &Path) -> Self {
        Self {
            sink_path: hearth_path.join(SINK_FILENAME),
        }
    }
}

/// Escape a string for inclusion between JSON quotes.
///
/// Most record fields are machine-controlled, but evidence references are
/// deliberately opaque caller values.  JSON forbids every unescaped C0
/// control character, so handling only newline/tab would let an otherwise
/// valid protobuf string poison the append-only JSONL stream.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{001f}' => {
                use std::fmt::Write as _;
                write!(&mut out, "\\u{:04x}", u32::from(c))
                    .expect("writing to a String cannot fail");
            }
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
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        '/' => value.push('/'),
                        'b' => value.push('\u{0008}'),
                        'f' => value.push('\u{000c}'),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        'u' => {
                            let mut codepoint = 0_u32;
                            for _ in 0..4 {
                                let digit = chars.next()?.to_digit(16)?;
                                codepoint = (codepoint << 4) | digit;
                            }
                            value.push(char::from_u32(codepoint)?);
                        }
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

/// Extract one boolean field's value from a flat single-line JSON object
/// (`"key":true` / `"key":false`). Returns `None` if the key is absent.
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

fn evidence_class_token(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

fn evidence_class_from_token(value: &str) -> Option<EvidenceClass> {
    match value {
        "artifact_of_consequence" => Some(EvidenceClass::ArtifactOfConsequence),
        "verifiable_citation" => Some(EvidenceClass::VerifiableCitation),
        "self_description" => Some(EvidenceClass::SelfDescription),
        _ => None,
    }
}

fn malformed(message: impl Into<String>) -> StepMeasurementError {
    StepMeasurementError::MalformedRecord {
        message: message.into(),
    }
}

fn contains_unescaped_json_key(line: &str, key: &str) -> bool {
    let needle = format!("\"{}\"", key);
    line.match_indices(&needle).any(|(index, _)| {
        let preceding_backslashes = line[..index]
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count();
        preceding_backslashes % 2 == 0
            && line[index + needle.len()..]
                .trim_start()
                .starts_with(':')
    })
}

/// Parse the optional four-key evidence extension as one unit. A row with none
/// of the keys is a legacy row. A row with only some keys, an unknown token, or
/// anything other than the allowlisted claim fields is malformed.
fn parse_evidence(line: &str) -> Result<Option<StepEvidenceRecord>, StepMeasurementError> {
    let evidence_keys = [
        "evidence_status",
        "missing_evidence_classes",
        "claimed_evidence",
        "playbook_version",
    ];
    // Parse every valid JSON row structurally, so key order and insignificant
    // whitespace cannot hide a partial extension. Historical evidence-neutral
    // rows were accepted by the old hand-reader even when they contained raw
    // control bytes; preserve that boundary only when the invalid row has no
    // unescaped evidence-key syntax at all.
    let value = match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => value,
        Err(_)
            if !evidence_keys
                .iter()
                .any(|key| contains_unescaped_json_key(line, key)) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(malformed(format!("invalid JSON: {}", error))),
    };
    let object = value
        .as_object()
        .ok_or_else(|| malformed("step-measurement line is not a JSON object"))?;
    let present = evidence_keys
        .iter()
        .filter(|key| object.contains_key(**key))
        .count();
    if present == 0 {
        return Ok(None);
    }
    if present != evidence_keys.len() {
        return Err(malformed(format!(
            "partial evidence extension (expected all four keys) in line: {}",
            line
        )));
    }

    let status_token = object
        .get("evidence_status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| malformed("evidence_status must be a string"))?;
    let status = EvidenceAssessmentStatus::from_token(status_token)
        .ok_or_else(|| malformed(format!("unknown evidence_status token '{}'", status_token)))?;

    let missing_values = object
        .get("missing_evidence_classes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| malformed("missing_evidence_classes must be an array"))?;
    let mut missing_classes = Vec::with_capacity(missing_values.len());
    for value in missing_values {
        let token = value
            .as_str()
            .ok_or_else(|| malformed("missing evidence class must be a string"))?;
        let class = evidence_class_from_token(token)
            .ok_or_else(|| malformed(format!("unknown evidence class token '{}'", token)))?;
        missing_classes.push(class);
    }

    let claim_values = object
        .get("claimed_evidence")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| malformed("claimed_evidence must be an array"))?;
    let mut claimed_evidence = Vec::with_capacity(claim_values.len());
    for value in claim_values {
        let claim = value
            .as_object()
            .ok_or_else(|| malformed("claimed_evidence item must be an object"))?;
        if claim.len() != 2 || !claim.contains_key("class") || !claim.contains_key("reference") {
            return Err(malformed(
                "claimed_evidence item must contain only class and reference",
            ));
        }
        let class_token = claim
            .get("class")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| malformed("claimed_evidence class must be a string"))?;
        let class = evidence_class_from_token(class_token).ok_or_else(|| {
            malformed(format!(
                "unknown claimed_evidence class token '{}'",
                class_token
            ))
        })?;
        let reference = claim
            .get("reference")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| malformed("claimed_evidence reference must be a string"))?
            .to_string();
        claimed_evidence.push(ClaimedEvidence { class, reference });
    }

    let playbook_version = object
        .get("playbook_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| malformed("playbook_version must be a string"))?
        .to_string();

    Ok(Some(StepEvidenceRecord {
        assessment: EvidenceAssessment {
            status,
            missing_classes,
        },
        claimed_evidence,
        playbook_version,
    }))
}

impl StepMeasurementWritePort for FileSystemStepMeasurementAdapter {
    fn append_step_measurement(
        &self,
        record: &StepMeasurementRecord,
    ) -> Result<(), StepMeasurementError> {
        // `actor_hash` is rendered ONLY when present — JSON `null` when absent —
        // so legacy readers and fresh readers agree the field is optional. The
        // hash is a hex digest (machine-controlled), escaped for safety anyway.
        let actor_hash_json = match &record.actor_hash {
            Some(h) => format!("\"{}\"", json_escape(h)),
            None => "null".to_string(),
        };
        let mut line = format!(
            "{{\"kind\":\"{}\",\"from_state\":\"{}\",\"to_state\":\"{}\",\"role\":\"{}\",\"intent_present\":{},\"expected_output_present\":{},\"at\":\"{}\",\"artifact_kind\":\"{}\",\"actor_hash\":{}",
            json_escape(&record.kind),
            json_escape(&record.from_state),
            json_escape(&record.to_state),
            json_escape(&record.role),
            record.intent_present,
            record.expected_output_present,
            json_escape(&record.at),
            json_escape(&record.artifact_kind),
            actor_hash_json,
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
        if let Some(evidence) = &record.evidence {
            // The evidence extension is emitted as ONE unit. Never render
            // claims or a version independently: `evidence: None` is the
            // byte-identical legacy path for FREE/unobligated steps.
            line.push_str(&format!(
                ",\"evidence_status\":\"{}\"",
                evidence.assessment.status.as_str()
            ));
            line.push_str(",\"missing_evidence_classes\":[");
            for (index, class) in evidence.assessment.missing_classes.iter().enumerate() {
                if index > 0 {
                    line.push(',');
                }
                line.push_str(&format!("\"{}\"", evidence_class_token(*class)));
            }
            line.push(']');
            line.push_str(",\"claimed_evidence\":[");
            for (index, claim) in evidence.claimed_evidence.iter().enumerate() {
                if index > 0 {
                    line.push(',');
                }
                line.push_str(&format!(
                    "{{\"class\":\"{}\",\"reference\":\"{}\"}}",
                    evidence_class_token(claim.class),
                    json_escape(&claim.reference),
                ));
            }
            line.push(']');
            line.push_str(&format!(
                ",\"playbook_version\":\"{}\"",
                json_escape(&evidence.playbook_version)
            ));
        }
        line.push_str("}\n");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.sink_path)
            .map_err(|e| StepMeasurementError::IoError {
                message: e.to_string(),
            })?;
        file.write_all(line.as_bytes())
            .map_err(|e| StepMeasurementError::IoError {
                message: e.to_string(),
            })?;
        Ok(())
    }
}

impl StepMeasurementReadPort for FileSystemStepMeasurementAdapter {
    fn read_step_measurements(&self) -> Result<Vec<StepMeasurementRecord>, StepMeasurementError> {
        let contents = match std::fs::read_to_string(&self.sink_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(StepMeasurementError::IoError {
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
                StepMeasurementError::MalformedRecord {
                    message: format!("missing kind in line: {}", line),
                }
            })?;
            let from_state = extract_field(line, "from_state").unwrap_or_default();
            let to_state = extract_field(line, "to_state").unwrap_or_default();
            let role = extract_field(line, "role").unwrap_or_default();
            let intent_present = extract_bool(line, "intent_present").unwrap_or(false);
            let expected_output_present =
                extract_bool(line, "expected_output_present").unwrap_or(false);
            let at = extract_field(line, "at").unwrap_or_default();
            // Additive fields: legacy records lack them. `workflow_kind` reads as
            // empty (the fold falls back to `kind`); `actor_hash` reads as None
            // (a `null` literal or an absent key both yield None).
            // The governed artifact kind, whichever spelling this row uses.
            // Both live on disk permanently (existing rows are never rewritten);
            // a row carrying both is refused, not silently resolved.
            let kind_read = read_artifact_kind(line, extract_field);
            if kind_read.is_ambiguous() {
                return Err(StepMeasurementError::MalformedRecord {
                    message: kind_read.ambiguity_message(line),
                });
            }
            let artifact_kind = kind_read.or_empty();
            let actor_hash = extract_field(line, "actor_hash");
            let conversation_hash = extract_field(line, "conversation_hash");
            let project_label = extract_field(line, "project_label");
            let playbook_run_id = extract_migrated_field(line, "playbook_run_id", "workflow_instance_id");
            let evidence = parse_evidence(line)?;
            records.push(StepMeasurementRecord {
                kind,
                from_state,
                to_state,
                role,
                intent_present,
                expected_output_present,
                at,
                artifact_kind,
                actor_hash,
                conversation_hash,
                project_label,
                playbook_run_id,
                evidence,
            });
        }
        Ok(records)
    }
}
