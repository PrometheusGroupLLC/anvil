//! Step module for `artifact_kind_dual_read.feature`.
//!
//! Seeds real JSONL rows byte-for-byte into a real TempDir hearth and reads
//! them back through the REAL `FileSystem*Adapter` read paths — no mocks, no
//! in-memory doubles. All five durable sinks that carry the governed
//! artifact-kind field are exercised in one step so the check cannot pass by
//! closing four of five.

use anvil_core_hearth::artifact_kind_field::{
    read_artifact_kind, ArtifactKindRead, CANONICAL_ARTIFACT_KIND_FIELD,
    LEGACY_ARTIFACT_KIND_FIELD,
};
use anvil_core_hearth::fs_activity_log_adapter::FileSystemActivityLogAdapter;
use anvil_core_hearth::fs_review_verdict_adapter::FileSystemReviewVerdictAdapter;
use anvil_core_hearth::fs_step_measurement_adapter::FileSystemStepMeasurementAdapter;
use anvil_core_hearth::fs_transition_measurement_adapter::FileSystemTransitionMeasurementAdapter;
use anvil_core_hearth::fs_playbook_measurement_adapter::FileSystemPlaybookMeasurementAdapter;
use anvil_core::ports::activity_log_port::ActivityLogReadPort;
use anvil_core::ports::review_verdict_port::ReviewVerdictReadPort;
use anvil_core::ports::step_measurement_port::StepMeasurementReadPort;
use anvil_core::ports::transition_measurement_port::TransitionMeasurementReadPort;
use anvil_core::ports::playbook_measurement_port::PlaybookMeasurementReadPort;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::{Path, PathBuf};

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const HEARTH_KEY: &str = "akd_hearth";
const HANDLE_KEY: &str = "akd_handle";
const REPORT_KEY: &str = "akd_report";

/// The governed kind every seeded row carries. A governed artifact kind — not
/// a definition kind — which is the whole reason the field is renamed.
const GOVERNED_KIND: &str = "lore_query";

/// One durable sink: its file name, and the row body every shape shares.
///
/// Each body omits the artifact-kind field; the seeder splices in whichever
/// spelling(s) the shape under test calls for. `kind` is the record-type
/// discriminator each reader requires and is deliberately NOT the field being
/// renamed.
const SINKS: [(&str, &str); 5] = [
    (
        "activity-log.jsonl",
        r#""command":"begin","outcome":"ok","from_state":"a","to_state":"b","at":"2026-07-30T00:00:00Z","source":"mcp""#,
    ),
    (
        "review-verdict.jsonl",
        r#""kind":"track","gate_state":"reviewing","satisfaction":"satisfied","outcome":"ok","at":"2026-07-30T00:00:00Z""#,
    ),
    (
        "workflow-measurement.jsonl",
        r#""kind":"track","terminal_state":"done","outcome":"ok","at":"2026-07-30T00:00:00Z""#,
    ),
    (
        "transition-measurement.jsonl",
        r#""kind":"track","from_state":"a","to_state":"b","role":"doer","outcome":"ok","at":"2026-07-30T00:00:00Z""#,
    ),
    (
        "step-measurement.jsonl",
        r#""kind":"track","from_state":"a","to_state":"b","role":"doer","intent_present":true,"expected_output_present":true,"at":"2026-07-30T00:00:00Z""#,
    ),
];

/// The artifact-kind fragment for a shape.
fn kind_fragment(shape: &str) -> String {
    match shape {
        "legacy" => format!(r#","{LEGACY_ARTIFACT_KIND_FIELD}":"{GOVERNED_KIND}""#),
        "canonical" => format!(r#","{CANONICAL_ARTIFACT_KIND_FIELD}":"{GOVERNED_KIND}""#),
        "both" => format!(
            r#","{CANONICAL_ARTIFACT_KIND_FIELD}":"{GOVERNED_KIND}","{LEGACY_ARTIFACT_KIND_FIELD}":"{GOVERNED_KIND}""#
        ),
        _ => String::new(),
    }
}

/// A minimal `"key":"value"` scanner, so the fold assertion exercises the fold
/// and not some adapter's unescaping.
fn naive_extract(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn seed(hearth: &Path, shapes: &[&str]) -> Result<(), String> {
    for (file, body) in SINKS {
        let mut contents = String::new();
        for shape in shapes {
            contents.push('{');
            contents.push_str(body);
            contents.push_str(&kind_fragment(shape));
            contents.push_str("}\n");
        }
        std::fs::write(hearth.join(file), contents).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read one sink and reduce it to a comparable verdict.
///
/// `ok:<kind>|<kind>` on success (one entry per row, in file order), or
/// `err:<message>` when the adapter refused the file.
fn read_all(hearth: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();

    out.push((
        "activity-log.jsonl".to_string(),
        match FileSystemActivityLogAdapter::new(hearth).read_activity_log() {
            Ok(rs) => format!(
                "ok:{}",
                rs.iter()
                    .map(|r| r.artifact_kind.clone())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Err(e) => format!("err:{e}"),
        },
    ));
    out.push((
        "review-verdict.jsonl".to_string(),
        match FileSystemReviewVerdictAdapter::new(hearth).read_review_verdicts() {
            Ok(rs) => format!(
                "ok:{}",
                rs.iter()
                    .map(|r| r.artifact_kind.clone())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Err(e) => format!("err:{e}"),
        },
    ));
    out.push((
        "workflow-measurement.jsonl".to_string(),
        match FileSystemPlaybookMeasurementAdapter::new(hearth).read_playbook_measurements() {
            Ok(rs) => format!(
                "ok:{}",
                rs.iter()
                    .map(|r| r.artifact_kind.clone())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Err(e) => format!("err:{e}"),
        },
    ));
    out.push((
        "transition-measurement.jsonl".to_string(),
        match FileSystemTransitionMeasurementAdapter::new(hearth).read_transition_measurements() {
            Ok(rs) => format!(
                "ok:{}",
                rs.iter()
                    .map(|r| r.artifact_kind.clone())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Err(e) => format!("err:{e}"),
        },
    ));
    out.push((
        "step-measurement.jsonl".to_string(),
        match FileSystemStepMeasurementAdapter::new(hearth).read_step_measurements() {
            Ok(rs) => format!(
                "ok:{}",
                rs.iter()
                    .map(|r| r.artifact_kind.clone())
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Err(e) => format!("err:{e}"),
        },
    ));

    out
}

fn seeding_step(name: &'static str, shapes: &'static [&'static str]) -> StepDef {
    step_def(
        name,
        &[],
        &[(HEARTH_KEY, "PathBuf"), (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>")],
        move |_ctx, _params| {
            let (handle, tmp) = retained_temp_dir("anvil-artifact-kind-")?;
            seed(&tmp, shapes)?;
            let mut out = Context::new();
            out.set(HEARTH_KEY, tmp);
            out.set::<RetainedTempDir>(HANDLE_KEY, handle);
            Ok(out)
        },
    )
}

pub fn steps() -> Vec<StepDef> {
    vec![
        seeding_step(
            "a durable sink hearth seeded with one legacy row and one canonical row per sink",
            &["legacy", "canonical"],
        ),
        seeding_step(
            "a durable sink hearth seeded with one dual-spelling row per sink",
            &["both"],
        ),
        seeding_step(
            "a durable sink hearth seeded with one row carrying neither spelling per sink",
            &["neither"],
        ),
        step_def(
            "every durable sink is read through its real adapter",
            &[(HEARTH_KEY, "PathBuf")],
            &[
                (HEARTH_KEY, "PathBuf"),
                (HANDLE_KEY, "Arc<Mutex<Option<TempDir>>>"),
                (REPORT_KEY, "String"),
            ],
            |ctx, _params| {
                let hearth = ctx.get::<PathBuf>(HEARTH_KEY).ok_or("No hearth")?.clone();
                let report = read_all(&hearth)
                    .into_iter()
                    .map(|(file, verdict)| format!("{file}={verdict}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut out = Context::new();
                out.set(HEARTH_KEY, hearth);
                if let Some(h) = ctx.get::<RetainedTempDir>(HANDLE_KEY) {
                    out.set::<RetainedTempDir>(HANDLE_KEY, h.clone());
                }
                out.set(REPORT_KEY, report);
                Ok(out)
            },
        ),
        check_def(
            "every sink reports the same governed kind for both rows",
            &[(REPORT_KEY, "String")],
            |ctx, _params| {
                let report: &String = ctx.require(REPORT_KEY)?;
                let want = format!("ok:{GOVERNED_KIND}|{GOVERNED_KIND}");
                assert_every_sink(report, &want)
            },
        ),
        check_def(
            "every sink refuses the row as malformed and names both field spellings",
            &[(REPORT_KEY, "String")],
            |ctx, _params| {
                let report: &String = ctx.require(REPORT_KEY)?;
                for line in report.lines() {
                    let (file, verdict) = line.split_once('=').ok_or("bad report line")?;
                    if !verdict.starts_with("err:") {
                        return Err(format!(
                            "{file}: a dual-spelling row must be refused, got '{verdict}'"
                        ));
                    }
                    for field in [CANONICAL_ARTIFACT_KIND_FIELD, LEGACY_ARTIFACT_KIND_FIELD] {
                        if !verdict.contains(field) {
                            return Err(format!(
                                "{file}: the refusal must name `{field}`, got '{verdict}'"
                            ));
                        }
                    }
                }
                Ok(())
            },
        ),
        step_def(
            "the artifact-kind fold is applied to the canonical, legacy, absent and dual-spelling shapes",
            &[],
            &[(REPORT_KEY, "String")],
            |_ctx, _params| {
                let row = |shape: &str| format!(r#"{{"kind":"track"{}}}"#, kind_fragment(shape));
                let mut lines = Vec::new();
                for shape in ["canonical", "legacy", "neither", "both"] {
                    let read = read_artifact_kind(&row(shape), naive_extract);
                    let variant = match &read {
                        ArtifactKindRead::Canonical(_) => "canonical",
                        ArtifactKindRead::Legacy(_) => "legacy",
                        ArtifactKindRead::Missing => "missing",
                        ArtifactKindRead::BothSpellings { .. } => "ambiguous",
                    };
                    lines.push(format!(
                        "{shape}={variant}/value={}/ambiguous={}",
                        read.value().unwrap_or("<none>"),
                        read.is_ambiguous()
                    ));
                }
                Ok(Context::new().with(REPORT_KEY, lines.join("\n")))
            },
        ),
        check_def(
            "the fold reports canonical, legacy, missing and ambiguous, and the ambiguous shape carries no value",
            &[(REPORT_KEY, "String")],
            |ctx, _params| {
                let report: &String = ctx.require(REPORT_KEY)?;
                let want = format!(
                    "canonical=canonical/value={GOVERNED_KIND}/ambiguous=false\n\
                     legacy=legacy/value={GOVERNED_KIND}/ambiguous=false\n\
                     neither=missing/value=<none>/ambiguous=false\n\
                     both=ambiguous/value=<none>/ambiguous=true"
                );
                if report != &want {
                    return Err(format!("expected:\n{want}\ngot:\n{report}"));
                }
                Ok(())
            },
        ),
        check_def(
            "every sink reports the empty governed kind and no error",
            &[(REPORT_KEY, "String")],
            |ctx, _params| {
                let report: &String = ctx.require(REPORT_KEY)?;
                assert_every_sink(report, "ok:")
            },
        ),
    ]
}

fn assert_every_sink(report: &str, want: &str) -> Result<(), String> {
    let mut seen = 0;
    for line in report.lines() {
        let (file, verdict) = line.split_once('=').ok_or("bad report line")?;
        if verdict != want {
            return Err(format!("{file}: expected '{want}', got '{verdict}'"));
        }
        seen += 1;
    }
    if seen != SINKS.len() {
        return Err(format!("expected {} sinks, saw {seen}", SINKS.len()));
    }
    Ok(())
}
