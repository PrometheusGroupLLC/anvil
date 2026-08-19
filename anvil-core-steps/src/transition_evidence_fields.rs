//! Steps for `transition_evidence_fields.feature` — the two new
//! `TransitionMeasurementRecord` fields and their read/write round trip.
//!
//! Exercises the real filesystem adapter, not a mock: the property under test is
//! that the fields survive serialisation and that older rows still decode, and
//! neither is observable against an in-memory stand-in.

use anvil_core::domain::begin::{classify_claim, is_bootstrap_placeholder, warns};
use anvil_core::domain::playbook::types::EvidenceClass;
use anvil_core_hearth::fs_transition_measurement_adapter::FileSystemTransitionMeasurementAdapter;
use anvil_core::ports::transition_measurement_port::{
    TransitionMeasurementReadPort,
    TransitionMeasurementRecord,
    TransitionMeasurementWritePort,
    TRANSITION_MEASUREMENT_KIND,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{
    check_def,
    step_def,
    StepDef,
};

const CLAIM_KEY: &str = "tef_claim";
const ARTIFACT_KEY: &str = "tef_artifact";
const DIR_KEY: &str = "tef_dir";
const STATUS_KEY: &str = "tef_status";
const WARN_KEY: &str = "tef_warns";

fn parse_classes(spec: &str) -> Vec<EvidenceClass> {
    spec.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| match t {
            "artifact_of_consequence" => EvidenceClass::ArtifactOfConsequence,
            "verifiable_citation" => EvidenceClass::VerifiableCitation,
            _ => EvidenceClass::SelfDescription,
        })
        .collect()
}

fn base_record() -> TransitionMeasurementRecord {
    TransitionMeasurementRecord {
        kind: TRANSITION_MEASUREMENT_KIND.to_string(),
        artifact_kind: "track".to_string(),
        from_state: "spec".to_string(),
        to_state: "spec_review".to_string(),
        role: "spec".to_string(),
        satisfaction: None,
        outcome: "ok".to_string(),
        success: true,
        at: "2026-08-10T00:00:00Z".to_string(),
        conversation_hash: None,
        project_label: None,
        playbook_run_id: Some("20260810T0000_probe".to_string()),
        claimed_evidence_status: None,
        artifact_assessment: None,
    }
}

fn write_record(rec: &TransitionMeasurementRecord) -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "anvil-tef-{}-{}",
        std::process::id(),
        rec.at.replace(':', "")
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    FileSystemTransitionMeasurementAdapter::new(&dir)
        .append_transition_measurement(rec)
        .map_err(|e| format!("{e}"))?;
    Ok(dir)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a transition measurement record with claim status {string} and artifact assessment {string}",
            &[],
            &[(DIR_KEY, "String")],
            |_ctx, params| {
                let claim = params.get_string(0).ok_or("Expected claim")?.to_string();
                let artifact = params.get_string(1).ok_or("Expected artifact")?.to_string();
                let mut rec = base_record();
                rec.claimed_evidence_status = Some(claim);
                rec.artifact_assessment = Some(artifact);
                let dir = write_record(&rec)?;
                let mut out = Context::new();
                out.set(DIR_KEY, dir.display().to_string());
                Ok(out)
            },
        ),
        step_def(
            "a transition measurement record with no evidence fields",
            &[],
            &[(DIR_KEY, "String")],
            |_ctx, _params| {
                let dir = write_record(&base_record())?;
                let mut out = Context::new();
                out.set(DIR_KEY, dir.display().to_string());
                Ok(out)
            },
        ),
        step_def(
            "the transition measurement sink is read back",
            &[(DIR_KEY, "String")],
            &[(DIR_KEY, "String"), (CLAIM_KEY, "String"), (ARTIFACT_KEY, "String")],
            |ctx, _params| {
                let dir = ctx.get::<String>(DIR_KEY).ok_or("No sink dir")?.clone();
                let records =
                    FileSystemTransitionMeasurementAdapter::new(std::path::Path::new(&dir))
                        .read_transition_measurements()
                        .map_err(|e| format!("{e}"))?;
                let last = records.last().ok_or("sink is empty")?;
                let mut out = Context::new();
                out.set(DIR_KEY, dir);
                // A missing field and an empty one are DIFFERENT facts, so the
                // absent case is rendered distinctly rather than as "".
                out.set(
                    CLAIM_KEY,
                    last.claimed_evidence_status
                        .clone()
                        .unwrap_or_else(|| "<none>".to_string()),
                );
                out.set(
                    ARTIFACT_KEY,
                    last.artifact_assessment
                        .clone()
                        .unwrap_or_else(|| "<none>".to_string()),
                );
                Ok(out)
            },
        ),
        check_def(
            "the transition record claim status is {string}",
            &[(CLAIM_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected claim")?.to_string();
                let got = ctx.get::<String>(CLAIM_KEY).ok_or("No read")?;
                if *got == want { Ok(()) } else { Err(format!("claim status was {got:?}, expected {want:?}")) }
            },
        ),
        check_def(
            "the transition record artifact assessment is {string}",
            &[(ARTIFACT_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected artifact")?.to_string();
                let got = ctx.get::<String>(ARTIFACT_KEY).ok_or("No read")?;
                if *got == want { Ok(()) } else { Err(format!("artifact assessment was {got:?}, expected {want:?}")) }
            },
        ),
        check_def(
            "the transition record has no claim status",
            &[(CLAIM_KEY, "String")],
            |ctx, _params| {
                let got = ctx.get::<String>(CLAIM_KEY).ok_or("No read")?;
                if got == "<none>" { Ok(()) } else { Err(format!("expected absent, got {got:?}")) }
            },
        ),
        check_def(
            "the transition record has no artifact assessment",
            &[(ARTIFACT_KEY, "String")],
            |ctx, _params| {
                let got = ctx.get::<String>(ARTIFACT_KEY).ok_or("No read")?;
                if got == "<none>" { Ok(()) } else { Err(format!("expected absent, got {got:?}")) }
            },
        ),
        step_def(
            "the file content {string} is tested as a bootstrap placeholder for {string} {string}",
            &[],
            &[(WARN_KEY, "String")],
            |_ctx, params| {
                let content = params.get_string(0).unwrap_or_default().replace("\\n", "\n");
                let kind = params.get_string(1).unwrap_or_default();
                let state = params.get_string(2).unwrap_or_default();
                let mut out = Context::new();
                out.set(
                    WARN_KEY,
                    if is_bootstrap_placeholder(&kind, &state, content.as_bytes()) {
                        "yes"
                    } else {
                        "no"
                    }
                    .to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "it is a placeholder is {string}",
            &[(WARN_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected yes/no")?.to_string();
                let got = ctx.get::<String>(WARN_KEY).ok_or("No verdict")?;
                if *got == want { Ok(()) } else { Err(format!("verdict was {got:?}, expected {want:?}")) }
            },
        ),
        step_def(
            "the claim is classified with begin {string} artifact_of_record {string} classes {string}",
            &[],
            &[(STATUS_KEY, "String")],
            |_ctx, params| {
                let is_begin = params.get_string(0).unwrap_or_default() == "yes";
                let has_aor = params.get_string(1).unwrap_or_default() == "yes";
                let classes = parse_classes(&params.get_string(2).unwrap_or_default());
                let mut out = Context::new();
                out.set(
                    STATUS_KEY,
                    classify_claim(is_begin, has_aor, &classes).to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the claim status is {string}",
            &[(STATUS_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected status")?.to_string();
                let got = ctx.get::<String>(STATUS_KEY).ok_or("No classification")?;
                if *got == want { Ok(()) } else { Err(format!("claim status was {got:?}, expected {want:?}")) }
            },
        ),
        step_def(
            "the warning gate is evaluated for claim {string} artifact {string}",
            &[],
            &[(WARN_KEY, "String")],
            |_ctx, params| {
                let claim = params.get_string(0).unwrap_or_default();
                let artifact = params.get_string(1).unwrap_or_default();
                let mut out = Context::new();
                out.set(
                    WARN_KEY,
                    if warns(&claim, &artifact) { "yes" } else { "no" }.to_string(),
                );
                Ok(out)
            },
        ),
        check_def(
            "the warning fires is {string}",
            &[(WARN_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected yes/no")?.to_string();
                let got = ctx.get::<String>(WARN_KEY).ok_or("No gate result")?;
                if *got == want { Ok(()) } else { Err(format!("warning was {got:?}, expected {want:?}")) }
            },
        ),
    ]
}
