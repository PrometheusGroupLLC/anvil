//! Typed filesystem-reader coverage for T-EEC-2 evidence extensions.
//!
//! These steps deliberately use the production adapter on both sides of the
//! evidence round-trip. Legacy and malformed rows are seeded as historical
//! bytes so the reader's compatibility boundary is exercised directly.

use anvil_core::domain::playbook::evidence_obligation::{
    EvidenceAssessment, EvidenceAssessmentStatus,
};
use anvil_core::domain::playbook::types::EvidenceClass;
use anvil_core::domain::shared_types::ClaimedEvidence;
use anvil_core_hearth::fs_step_measurement_adapter::{
    FileSystemStepMeasurementAdapter, SINK_FILENAME,
};
use anvil_core::ports::step_measurement_port::{
    StepEvidenceRecord, StepMeasurementError, StepMeasurementReadPort, StepMeasurementRecord,
    StepMeasurementWritePort, STEP_MEASUREMENT_KIND,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

use anvil_test_support::{retained_temp_dir, RetainedTempDir};

const FIXTURE_KEY: &str = "step_measurement_evidence_reader_fixture";
const VERSION: &str = "c0-reader-version-1";

#[derive(Clone)]
struct EvidenceReaderFixture {
    hearth: std::path::PathBuf,
    _hearth_handle: RetainedTempDir,
    record_to_append: Option<StepMeasurementRecord>,
    expected_reference: Option<String>,
    expected_legacy_label: Option<String>,
    expected_c0_project_label: Option<String>,
    read_result: Option<Result<Vec<StepMeasurementRecord>, StepMeasurementError>>,
}

fn c0_reference() -> String {
    (0_u8..=0x1f).map(char::from).collect()
}

fn is_exact_c0(value: &str) -> bool {
    value.chars().map(u32::from).eq(0_u32..=u32::from(0x1f_u8))
}

fn fixture(ctx: &Context) -> Result<&EvidenceReaderFixture, String> {
    ctx.get::<EvidenceReaderFixture>(FIXTURE_KEY)
        .ok_or_else(|| "No step-measurement evidence-reader fixture".to_string())
}

fn return_fixture(fixture: EvidenceReaderFixture) -> Result<Context, String> {
    let mut out = Context::new();
    out.set(FIXTURE_KEY, fixture);
    Ok(out)
}

fn read_result(
    fixture: &EvidenceReaderFixture,
) -> Result<&Result<Vec<StepMeasurementRecord>, StepMeasurementError>, String> {
    fixture
        .read_result
        .as_ref()
        .ok_or_else(|| "The durable step measurements have not been read".to_string())
}

fn read_records(fixture: &EvidenceReaderFixture) -> Result<&[StepMeasurementRecord], String> {
    match read_result(fixture)? {
        Ok(records) => Ok(records),
        Err(error) => Err(format!("Typed step-measurement read failed: {}", error)),
    }
}

fn one_record(fixture: &EvidenceReaderFixture) -> Result<&StepMeasurementRecord, String> {
    let records = read_records(fixture)?;
    if records.len() == 1 {
        Ok(&records[0])
    } else {
        Err(format!(
            "Expected exactly one typed step-measurement record, got {}",
            records.len()
        ))
    }
}

fn base_record(evidence: Option<StepEvidenceRecord>) -> StepMeasurementRecord {
    StepMeasurementRecord {
        kind: STEP_MEASUREMENT_KIND.to_string(),
        from_state: "spec".to_string(),
        to_state: "spec_review".to_string(),
        role: "doer".to_string(),
        intent_present: true,
        expected_output_present: true,
        at: "2026-07-19T22:30:00Z".to_string(),
        artifact_kind: "evidence_reader_probe".to_string(),
        actor_hash: None,
        conversation_hash: None,
        project_label: None,
        playbook_run_id: Some("evidence-reader-run".to_string()),
        evidence,
    }
}

fn seed_raw_row(fixture: &EvidenceReaderFixture, row: &[u8]) -> Result<(), String> {
    std::fs::write(fixture.hearth.join(SINK_FILENAME), row)
        .map_err(|error| format!("seed historical step-measurement row: {}", error))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a step measurement evidence-reader hearth",
            &[],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |_ctx, _params| {
                let (handle, hearth) = retained_temp_dir("anvil-step-evidence-reader-")?;
                return_fixture(EvidenceReaderFixture {
                    hearth,
                    _hearth_handle: handle,
                    record_to_append: None,
                    expected_reference: None,
                    expected_legacy_label: None,
                    expected_c0_project_label: None,
                    read_result: None,
                })
            },
        ),
        step_def(
            "an incomplete evidence record with one missing artifact class and one citation claim containing every JSON C0 control character",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                let reference = c0_reference();
                fixture.record_to_append = Some(base_record(Some(StepEvidenceRecord {
                    assessment: EvidenceAssessment {
                        status: EvidenceAssessmentStatus::Incomplete,
                        missing_classes: vec![EvidenceClass::ArtifactOfConsequence],
                    },
                    claimed_evidence: vec![ClaimedEvidence {
                        class: EvidenceClass::VerifiableCitation,
                        reference: reference.clone(),
                    }],
                    playbook_version: VERSION.to_string(),
                })));
                fixture.expected_reference = Some(reference);
                return_fixture(fixture)
            },
        ),
        step_def(
            "the evidence-bearing step measurement is appended and read",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                let record = fixture
                    .record_to_append
                    .as_ref()
                    .ok_or("No evidence-bearing record to append")?;
                let adapter = FileSystemStepMeasurementAdapter::new(&fixture.hearth);
                fixture.read_result = Some(
                    adapter
                        .append_step_measurement(record)
                        .and_then(|_| adapter.read_step_measurements()),
                );
                return_fixture(fixture)
            },
        ),
        step_def(
            "a legacy step measurement row without evidence and with a form-feed in its project label",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                let label = "legacy\u{000c}project".to_string();
                let row = format!(
                    "{{\"kind\":\"step_measurement\",\"from_state\":\"legacy\",\"to_state\":\"legacy_review\",\"role\":\"doer\",\"intent_present\":true,\"expected_output_present\":true,\"at\":\"2026-07-19T22:31:00Z\",\"artifact_kind\":\"legacy_probe\",\"actor_hash\":null,\"project_label\":\"{}\"}}\n",
                    label
                );
                seed_raw_row(&fixture, row.as_bytes())?;
                fixture.expected_legacy_label = Some(label);
                return_fixture(fixture)
            },
        ),
        step_def(
            "an evidence-neutral step measurement whose project label contains every JSON C0 control character",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                let project_label = c0_reference();
                let mut record = base_record(None);
                record.project_label = Some(project_label.clone());
                fixture.record_to_append = Some(record);
                fixture.expected_c0_project_label = Some(project_label);
                return_fixture(fixture)
            },
        ),
        step_def(
            "the evidence-neutral step measurement is appended and read",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                let record = fixture
                    .record_to_append
                    .as_ref()
                    .ok_or("No evidence-neutral record to append")?;
                let adapter = FileSystemStepMeasurementAdapter::new(&fixture.hearth);
                fixture.read_result = Some(
                    adapter
                        .append_step_measurement(record)
                        .and_then(|_| adapter.read_step_measurements()),
                );
                return_fixture(fixture)
            },
        ),
        step_def(
            "a step measurement row carrying only an evidence status",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?.clone();
                seed_raw_row(
                    &fixture,
                    b"{\"kind\":\"step_measurement\",\"from_state\":\"spec\",\"to_state\":\"spec_review\",\"role\":\"doer\",\"intent_present\":true,\"expected_output_present\":true,\"at\":\"2026-07-19T22:32:00Z\",\"artifact_kind\":\"partial_probe\",\"actor_hash\":null,\"evidence_status\":\"incomplete\"}\n",
                )?;
                return_fixture(fixture)
            },
        ),
        step_def(
            "a step measurement row whose sole evidence key is first in the object",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?.clone();
                seed_raw_row(
                    &fixture,
                    b"{\"evidence_status\":\"incomplete\",\"kind\":\"step_measurement\",\"from_state\":\"spec\",\"to_state\":\"spec_review\",\"role\":\"doer\",\"intent_present\":true,\"expected_output_present\":true,\"at\":\"2026-07-19T22:33:00Z\",\"artifact_kind\":\"first_partial_probe\",\"actor_hash\":null}\n",
                )?;
                return_fixture(fixture)
            },
        ),
        step_def(
            "a whitespace-formatted step measurement row carrying only an evidence status",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?.clone();
                seed_raw_row(
                    &fixture,
                    b"{\"kind\":\"step_measurement\",\"from_state\":\"spec\",\"to_state\":\"spec_review\",\"role\":\"doer\",\"intent_present\":true,\"expected_output_present\":true,\"at\":\"2026-07-19T22:34:00Z\",\"artifact_kind\":\"whitespace_partial_probe\",\"actor_hash\":null, \"evidence_status\" : \"incomplete\"}\n",
                )?;
                return_fixture(fixture)
            },
        ),
        step_def(
            "the durable step measurements are read",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let mut fixture = fixture(&ctx)?.clone();
                fixture.read_result = Some(
                    FileSystemStepMeasurementAdapter::new(&fixture.hearth)
                        .read_step_measurements(),
                );
                return_fixture(fixture)
            },
        ),
        check_def(
            "the typed step measurement read succeeds",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                read_records(fixture(&ctx)?)?;
                Ok(())
            },
        ),
        check_def(
            "the evidence extension round-trips exactly",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let actual = one_record(fixture)?
                    .evidence
                    .as_ref()
                    .ok_or("Typed record has no evidence extension")?;
                let expected = fixture
                    .record_to_append
                    .as_ref()
                    .and_then(|record| record.evidence.as_ref())
                    .ok_or("Fixture has no expected evidence extension")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Evidence extension changed across durable round-trip: expected {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the opaque reference preserves every JSON C0 control character",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let expected = fixture
                    .expected_reference
                    .as_ref()
                    .ok_or("Fixture has no expected C0 reference")?;
                let actual = one_record(fixture)?
                    .evidence
                    .as_ref()
                    .and_then(|evidence| evidence.claimed_evidence.first())
                    .map(|claim| claim.reference.as_str())
                    .ok_or("Typed record has no claimed opaque reference")?;
                if actual == expected && is_exact_c0(actual) {
                    Ok(())
                } else {
                    Err(format!(
                        "Opaque reference did not preserve C0 bytes 0x00..0x1f: got {:?}",
                        actual.as_bytes()
                    ))
                }
            },
        ),
        check_def(
            "the evidence-neutral record has no evidence extension",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                if one_record(fixture(&ctx)?)?.evidence.is_none() {
                    Ok(())
                } else {
                    Err("Evidence-neutral record unexpectedly gained evidence".to_string())
                }
            },
        ),
        check_def(
            "the project label preserves every JSON C0 control character",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let expected = fixture
                    .expected_c0_project_label
                    .as_deref()
                    .ok_or("Fixture has no expected C0 project label")?;
                let actual = one_record(fixture)?
                    .project_label
                    .as_deref()
                    .ok_or("Typed record has no project label")?;
                if actual == expected && is_exact_c0(actual) {
                    Ok(())
                } else {
                    Err(format!(
                        "Project label did not preserve C0 bytes 0x00..0x1f: got {:?}",
                        actual.as_bytes()
                    ))
                }
            },
        ),
        check_def(
            "the legacy record has no evidence extension",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                if one_record(fixture(&ctx)?)?.evidence.is_none() {
                    Ok(())
                } else {
                    Err("Legacy record unexpectedly gained an evidence extension".to_string())
                }
            },
        ),
        check_def(
            "the legacy project label is preserved exactly",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| {
                let fixture = fixture(&ctx)?;
                let expected = fixture
                    .expected_legacy_label
                    .as_deref()
                    .ok_or("Fixture has no expected legacy project label")?;
                let actual = one_record(fixture)?
                    .project_label
                    .as_deref()
                    .ok_or("Typed legacy record has no project label")?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Legacy project label changed: expected {:?}, got {:?}",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the typed step measurement read is rejected as a partial evidence extension",
            &[(FIXTURE_KEY, "EvidenceReaderFixture")],
            |ctx, _params| match read_result(fixture(&ctx)?)? {
                Err(StepMeasurementError::MalformedRecord { message })
                    if message.contains("partial evidence extension") =>
                {
                    Ok(())
                }
                Err(error) => Err(format!(
                    "Expected partial-extension malformed-record error, got {}",
                    error
                )),
                Ok(records) => Err(format!(
                    "Expected partial evidence extension rejection, read {} records",
                    records.len()
                )),
            },
        ),
    ]
}
