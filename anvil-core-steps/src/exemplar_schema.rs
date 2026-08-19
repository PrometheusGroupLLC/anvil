//! Step module for exemplar_schema_storage_resolution.feature.

use anvil_core::domain::playbook::anchor_coverage::{
    validate_anchor_coverage, AnchorCoverageValidation,
};
use anvil_core::domain::playbook::candidate::{LedgerClassification, NoneYetJustification};
use anvil_core::domain::playbook::exemplar::{
    load_from_markdown, Exemplar, ExemplarFrontmatter, ExemplarLoadError, ExemplarProvenance,
    OutcomeLink,
};
use anvil_core::domain::playbook::exemplar_resolver::{
    exemplar_coverage, resolve_anchors, AnchorResolution, AnchorResolutionError,
    AnchorResolutionWarning, ExemplarCoverage, ResolvedExemplar,
};
use anvil_core::domain::playbook::types::{
    AnchorRef, EvidenceClass, RubricDimension, SuccessRubric,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const EX_MARKDOWN_KEY: &str = "exemplar_markdown";
const EX_LOAD_KEY: &str = "exemplar_load_result";
const EX_HEARTH_KEY: &str = "exemplar_hearth_path";
const EX_HEARTH_HANDLE_KEY: &str = "exemplar_hearth_handle";
const EX_RUBRIC_KEY: &str = "exemplar_success_rubric";
const EX_RESOLUTION_KEY: &str = "exemplar_resolution_result";
const EX_COVERAGE_INPUT_KEY: &str = "exemplar_coverage_inputs";
const EX_COVERAGE_KEY: &str = "exemplar_coverage_result";
const EX_LEDGER_KEY: &str = "exemplar_ledger_classification";
const EX_NONE_YET_KEY: &str = "exemplar_none_yet_justification";
const EX_VALIDATION_KEY: &str = "exemplar_anchor_coverage_validation";

fn exemplar_markdown(id: &str, band: &str, evidence_class: &str, outcome_link: bool) -> String {
    let outcome = if outcome_link {
        r#"outcome_link:
  authority: brine
  opaque_ref: outcome-123
  verified_at: "2026-07-04T00:00:00Z"
"#
    } else {
        ""
    };
    format!(
        r#"---
id: {id}
band: {band}
dimensions: [correctness, security]
evidence_class: {evidence_class}
{outcome}provenance:
  source: internal
  corpus: brine-fixtures
playbook_version: v1
refreshed_at: "2026-07-04T00:00:00Z"
---
This distilled pattern preserves the useful structure without raw material.
"#
    )
}

fn exemplar_markdown_with_version(id: &str, band: &str, playbook_version: &str) -> String {
    exemplar_markdown(id, band, "artifact_of_consequence", true).replace(
        "playbook_version: v1",
        &format!("playbook_version: {}", playbook_version),
    )
}

fn create_hearth_kind(kind: &str) -> Result<(anvil_test_support::RetainedTempDir, PathBuf, PathBuf), String> {
    let (handle, hearth) = anvil_test_support::retained_temp_dir("anvil-exemplars-")?;
    let kind_dir = hearth.join("playbooks").join(kind);
    std::fs::create_dir_all(kind_dir.join("exemplars"))
        .map_err(|e| format!("create exemplar dir: {}", e))?;
    Ok((handle, hearth, kind_dir))
}

fn write_exemplar_file(
    hearth: &Path,
    kind: &str,
    filename: &str,
    id: &str,
    playbook_version: &str,
) -> Result<(), String> {
    let path = hearth
        .join("playbooks")
        .join(kind)
        .join("exemplars")
        .join(filename);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create exemplar dir: {}", e))?;
    }
    std::fs::write(
        &path,
        exemplar_markdown_with_version(id, "good", playbook_version),
    )
    .map_err(|e| format!("write exemplar {}: {}", path.display(), e))
}

fn parse_dimensions_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn rubric_with_dimensions(dimensions: Vec<String>) -> SuccessRubric {
    SuccessRubric {
        dimensions: dimensions
            .into_iter()
            .map(|dimension| RubricDimension {
                dimension,
                weight: 1,
                evidence_class: EvidenceClass::SelfDescription,
            })
            .collect(),
        grader: None,
        lagging_signals: vec![],
        anchors: vec![],
    }
}

fn exemplar_for_dimensions(dimensions: Vec<String>) -> ResolvedExemplar {
    ResolvedExemplar {
        anchor: AnchorRef {
            instance: "fixture".to_string(),
            band: "good".to_string(),
        },
        path: PathBuf::from("fixture.md"),
        exemplar: Exemplar {
            frontmatter: ExemplarFrontmatter {
                id: "fixture".to_string(),
                band: "good".to_string(),
                dimensions,
                evidence_class: EvidenceClass::SelfDescription,
                outcome_link: None,
                provenance: ExemplarProvenance {
                    source: "internal".to_string(),
                    corpus: "brine-fixtures".to_string(),
                },
                playbook_version: "v1".to_string(),
                refreshed_at: "2026-07-04T00:00:00Z".to_string(),
            },
            body: "distilled pattern".to_string(),
        },
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an exemplar Markdown file with band {string}",
            &[],
            &[(EX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let band = params.get_string(0).ok_or("Expected band")?;
                let mut out = Context::new();
                out.set(
                    EX_MARKDOWN_KEY,
                    exemplar_markdown(&band.replace('_', "-"), &band, "artifact_of_consequence", true),
                );
                Ok(out)
            },
        ),
        step_def(
            "an exemplar Markdown file with artifact_of_consequence evidence and no outcome_link",
            &[],
            &[(EX_MARKDOWN_KEY, "String")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(
                    EX_MARKDOWN_KEY,
                    exemplar_markdown("consequence-without-link", "good", "artifact_of_consequence", false),
                );
                Ok(out)
            },
        ),
        step_def(
            "an exemplar Markdown file containing raw marker {string}",
            &[],
            &[(EX_MARKDOWN_KEY, "String")],
            |_ctx, params| {
                let marker = params.get_string(0).ok_or("Expected raw marker")?;
                let markdown = if marker == "body_fence" {
                    exemplar_markdown("raw-body", "good", "self_description", false)
                        .replace("This distilled pattern", "```raw-artifact\nsecret\n```\nThis distilled pattern")
                } else {
                    exemplar_markdown("raw-frontmatter", "good", "self_description", false)
                        .replace("refreshed_at:", &format!("{}: forbidden\nrefreshed_at:", marker))
                };
                let mut out = Context::new();
                out.set(EX_MARKDOWN_KEY, markdown);
                Ok(out)
            },
        ),
        step_def(
            "the exemplar is loaded",
            &[(EX_MARKDOWN_KEY, "String")],
            &[(EX_LOAD_KEY, "Result<Exemplar, ExemplarLoadError>")],
            |ctx, _params| {
                let markdown = ctx.get::<String>(EX_MARKDOWN_KEY).ok_or("No exemplar markdown")?;
                let mut out = Context::new();
                out.set(EX_LOAD_KEY, load_from_markdown(markdown));
                Ok(out)
            },
        ),
        check_def(
            "the exemplar load succeeds",
            &[(EX_LOAD_KEY, "Result<Exemplar, ExemplarLoadError>")],
            |ctx, _params| match ctx
                .get::<Result<Exemplar, ExemplarLoadError>>(EX_LOAD_KEY)
                .ok_or("No exemplar load result")?
            {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Expected exemplar load success, got {}", e)),
            },
        ),
        check_def(
            "the loaded exemplar band is {string}",
            &[(EX_LOAD_KEY, "Result<Exemplar, ExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected band")?;
                let exemplar = ctx
                    .get::<Result<Exemplar, ExemplarLoadError>>(EX_LOAD_KEY)
                    .ok_or("No exemplar load result")?
                    .as_ref()
                    .map_err(|e| format!("Expected exemplar load success, got {}", e))?;
                if exemplar.frontmatter.band == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected band '{}' got '{}'",
                        expected, exemplar.frontmatter.band
                    ))
                }
            },
        ),
        check_def(
            "the loaded exemplar body contains {string}",
            &[(EX_LOAD_KEY, "Result<Exemplar, ExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected body text")?;
                let exemplar = ctx
                    .get::<Result<Exemplar, ExemplarLoadError>>(EX_LOAD_KEY)
                    .ok_or("No exemplar load result")?
                    .as_ref()
                    .map_err(|e| format!("Expected exemplar load success, got {}", e))?;
                if exemplar.body.contains(&expected) {
                    Ok(())
                } else {
                    Err(format!("Body did not contain '{}': {}", expected, exemplar.body))
                }
            },
        ),
        check_def(
            "the exemplar load fails with code {string}",
            &[(EX_LOAD_KEY, "Result<Exemplar, ExemplarLoadError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected error code")?;
                match ctx
                    .get::<Result<Exemplar, ExemplarLoadError>>(EX_LOAD_KEY)
                    .ok_or("No exemplar load result")?
                {
                    Ok(exemplar) => Err(format!("Expected exemplar load failure, got {:?}", exemplar)),
                    Err(e) if e.code() == expected => Ok(()),
                    Err(e) => Err(format!("Expected code '{}' got '{}': {}", expected, e.code(), e)),
                }
            },
        ),
        step_def(
            "a temporary hearth playbook kind {string} with exemplar {string}",
            &[],
            &[
                (EX_HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (EX_HEARTH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let exemplar_id = params.get_string(1).ok_or("Expected exemplar id")?;
                let (handle, hearth, _kind_dir) = create_hearth_kind(&kind)?;
                write_exemplar_file(&hearth, &kind, &format!("{}.md", exemplar_id), &exemplar_id, "v1")?;
                let mut out = Context::new();
                out.set(EX_HEARTH_HANDLE_KEY, handle);
                out.set(EX_HEARTH_KEY, hearth);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth playbook kind {string} with exemplar {string} playbook_version {string}",
            &[],
            &[
                (EX_HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (EX_HEARTH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let exemplar_id = params.get_string(1).ok_or("Expected exemplar id")?;
                let version = params.get_string(2).ok_or("Expected version")?;
                let (handle, hearth, _kind_dir) = create_hearth_kind(&kind)?;
                write_exemplar_file(
                    &hearth,
                    &kind,
                    &format!("{}.md", exemplar_id),
                    &exemplar_id,
                    &version,
                )?;
                let mut out = Context::new();
                out.set(EX_HEARTH_HANDLE_KEY, handle);
                out.set(EX_HEARTH_KEY, hearth);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth playbook kind {string} with no exemplars",
            &[],
            &[
                (EX_HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (EX_HEARTH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let (handle, hearth, _kind_dir) = create_hearth_kind(&kind)?;
                let mut out = Context::new();
                out.set(EX_HEARTH_HANDLE_KEY, handle);
                out.set(EX_HEARTH_KEY, hearth);
                Ok(out)
            },
        ),
        step_def(
            "a temporary hearth playbook kind {string} with exemplar file {string} id {string}",
            &[],
            &[
                (EX_HEARTH_HANDLE_KEY, "RetainedTempDir"),
                (EX_HEARTH_KEY, "PathBuf"),
            ],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let filename = params.get_string(1).ok_or("Expected filename")?;
                let id = params.get_string(2).ok_or("Expected id")?;
                let (handle, hearth, _kind_dir) = create_hearth_kind(&kind)?;
                write_exemplar_file(&hearth, &kind, &filename, &id, "v1")?;
                let mut out = Context::new();
                out.set(EX_HEARTH_HANDLE_KEY, handle);
                out.set(EX_HEARTH_KEY, hearth);
                Ok(out)
            },
        ),
        step_def(
            "the hearth playbook kind {string} has exemplar file {string} id {string}",
            &[(EX_HEARTH_KEY, "PathBuf"), (EX_HEARTH_HANDLE_KEY, "RetainedTempDir")],
            &[(EX_HEARTH_KEY, "PathBuf"), (EX_HEARTH_HANDLE_KEY, "RetainedTempDir")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let filename = params.get_string(1).ok_or("Expected filename")?;
                let id = params.get_string(2).ok_or("Expected id")?;
                let hearth = ctx.get::<PathBuf>(EX_HEARTH_KEY).ok_or("No exemplar hearth")?.clone();
                write_exemplar_file(&hearth, &kind, &filename, &id, "v1")?;
                let mut out = Context::new();
                out.set(EX_HEARTH_KEY, hearth);
                if let Some(handle) = ctx.get::<anvil_test_support::RetainedTempDir>(EX_HEARTH_HANDLE_KEY) {
                    out.set(EX_HEARTH_HANDLE_KEY, Arc::clone(handle));
                }
                Ok(out)
            },
        ),
        step_def(
            "a success rubric anchor referencing {string} with band {string} and playbook_version {string}",
            &[],
            &[
                (EX_RUBRIC_KEY, "SuccessRubric"),
                (EX_HEARTH_KEY, "PathBuf"),
                (EX_HEARTH_HANDLE_KEY, "RetainedTempDir"),
            ],
            |ctx, params| {
                let instance = params.get_string(0).ok_or("Expected instance")?;
                let band = params.get_string(1).ok_or("Expected band")?;
                let mut rubric = rubric_with_dimensions(vec!["correctness".to_string()]);
                rubric.anchors.push(AnchorRef {
                    instance: instance.to_string(),
                    band: band.to_string(),
                });
                let mut out = Context::new();
                out.set(EX_RUBRIC_KEY, rubric);
                if let Some(hearth) = ctx.get::<PathBuf>(EX_HEARTH_KEY) {
                    out.set(EX_HEARTH_KEY, hearth.clone());
                }
                if let Some(handle) = ctx.get::<anvil_test_support::RetainedTempDir>(EX_HEARTH_HANDLE_KEY) {
                    out.set(EX_HEARTH_HANDLE_KEY, Arc::clone(handle));
                }
                Ok(out)
            },
        ),
        step_def(
            "the rubric anchors are resolved for kind {string} and playbook_version {string}",
            &[(EX_HEARTH_KEY, "PathBuf"), (EX_RUBRIC_KEY, "SuccessRubric")],
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?;
                let version = params.get_string(1).ok_or("Expected version")?;
                let hearth = ctx.get::<PathBuf>(EX_HEARTH_KEY).ok_or("No exemplar hearth")?;
                let rubric = ctx.get::<SuccessRubric>(EX_RUBRIC_KEY).ok_or("No rubric")?;
                let result = resolve_anchors(
                    &hearth.join("playbooks").join(kind),
                    rubric,
                    &version,
                );
                let mut out = Context::new();
                out.set(EX_RESOLUTION_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "anchor resolution succeeds",
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, _params| match ctx
                .get::<Result<AnchorResolution, AnchorResolutionError>>(EX_RESOLUTION_KEY)
                .ok_or("No anchor resolution result")?
            {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Expected anchor resolution success, got {}", e)),
            },
        ),
        check_def(
            "the resolved exemplar body contains {string}",
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected body text")?;
                let resolution = ctx
                    .get::<Result<AnchorResolution, AnchorResolutionError>>(EX_RESOLUTION_KEY)
                    .ok_or("No anchor resolution result")?
                    .as_ref()
                    .map_err(|e| format!("Expected anchor resolution success, got {}", e))?;
                let Some(resolved) = resolution.exemplars.first() else {
                    return Err("No resolved exemplars".to_string());
                };
                if resolved.exemplar.body.contains(&expected) {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected body containing '{}', got '{}'",
                        expected, resolved.exemplar.body
                    ))
                }
            },
        ),
        check_def(
            "anchor resolution has no warnings",
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, _params| {
                let resolution = ctx
                    .get::<Result<AnchorResolution, AnchorResolutionError>>(EX_RESOLUTION_KEY)
                    .ok_or("No anchor resolution result")?
                    .as_ref()
                    .map_err(|e| format!("Expected anchor resolution success, got {}", e))?;
                if resolution.warnings.is_empty() {
                    Ok(())
                } else {
                    Err(format!("Expected no warnings, got {:?}", resolution.warnings))
                }
            },
        ),
        check_def(
            "anchor resolution fails with code {string}",
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected error code")?;
                match ctx
                    .get::<Result<AnchorResolution, AnchorResolutionError>>(EX_RESOLUTION_KEY)
                    .ok_or("No anchor resolution result")?
                {
                    Ok(resolution) => Err(format!("Expected failure, got {:?}", resolution)),
                    Err(e) if e.code() == expected => Ok(()),
                    Err(e) => Err(format!("Expected code '{}' got '{}': {}", expected, e.code(), e)),
                }
            },
        ),
        check_def(
            "anchor resolution has a stale warning for exemplar {string} expected {string} actual {string}",
            &[(EX_RESOLUTION_KEY, "Result<AnchorResolution, AnchorResolutionError>")],
            |ctx, params| {
                let instance = params.get_string(0).ok_or("Expected instance")?;
                let expected = params.get_string(1).ok_or("Expected version")?;
                let actual = params.get_string(2).ok_or("Expected actual version")?;
                let resolution = ctx
                    .get::<Result<AnchorResolution, AnchorResolutionError>>(EX_RESOLUTION_KEY)
                    .ok_or("No anchor resolution result")?
                    .as_ref()
                    .map_err(|e| format!("Expected anchor resolution success, got {}", e))?;
                let wanted = AnchorResolutionWarning::StalePlaybookVersion {
                    instance: instance.to_string(),
                    expected: expected.to_string(),
                    actual: actual.to_string(),
                };
                if resolution.warnings.contains(&wanted) {
                    Ok(())
                } else {
                    Err(format!("Expected warning {:?}, got {:?}", wanted, resolution.warnings))
                }
            },
        ),
        step_def(
            "a success rubric scoring dimensions {string}",
            &[],
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            |_ctx, params| {
                let dimensions = parse_dimensions_csv(&params.get_string(0).ok_or("Expected dimensions")?);
                let mut out = Context::new();
                out.set(
                    EX_COVERAGE_INPUT_KEY,
                    (rubric_with_dimensions(dimensions), Vec::<ResolvedExemplar>::new()),
                );
                Ok(out)
            },
        ),
        step_def(
            "resolved exemplars covering dimensions {string}",
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            |ctx, params| {
                let dimensions = parse_dimensions_csv(&params.get_string(0).ok_or("Expected dimensions")?);
                let (rubric, mut exemplars) = ctx
                    .get::<(SuccessRubric, Vec<ResolvedExemplar>)>(EX_COVERAGE_INPUT_KEY)
                    .ok_or("No coverage input")?
                    .clone();
                exemplars.push(exemplar_for_dimensions(dimensions));
                let mut out = Context::new();
                out.set(EX_COVERAGE_INPUT_KEY, (rubric, exemplars));
                Ok(out)
            },
        ),
        step_def(
            "exemplar coverage is calculated",
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            &[(EX_COVERAGE_KEY, "ExemplarCoverage")],
            |ctx, _params| {
                let (rubric, exemplars) = ctx
                    .get::<(SuccessRubric, Vec<ResolvedExemplar>)>(EX_COVERAGE_INPUT_KEY)
                    .ok_or("No coverage input")?;
                let mut out = Context::new();
                out.set(EX_COVERAGE_KEY, exemplar_coverage(rubric, exemplars));
                Ok(out)
            },
        ),
        check_def(
            "exemplar coverage is complete",
            &[(EX_COVERAGE_KEY, "ExemplarCoverage")],
            |ctx, _params| {
                let coverage = ctx.get::<ExemplarCoverage>(EX_COVERAGE_KEY).ok_or("No coverage")?;
                if coverage.is_complete() {
                    Ok(())
                } else {
                    Err(format!("Expected complete coverage, got {:?}", coverage))
                }
            },
        ),
        check_def(
            "exemplar coverage is incomplete with uncovered dimensions {string}",
            &[(EX_COVERAGE_KEY, "ExemplarCoverage")],
            |ctx, params| {
                let expected = parse_dimensions_csv(&params.get_string(0).ok_or("Expected dimensions")?);
                let coverage = ctx.get::<ExemplarCoverage>(EX_COVERAGE_KEY).ok_or("No coverage")?;
                if coverage.uncovered == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected uncovered {:?}, got {:?}",
                        expected, coverage.uncovered
                    ))
                }
            },
        ),
        step_def(
            "ledger classification {string} with complete none_yet justification",
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            &[
                (EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)"),
                (EX_LEDGER_KEY, "LedgerClassification"),
                (EX_NONE_YET_KEY, "NoneYetJustification"),
            ],
            |mut ctx, params| {
                let classification = params.get_string(0).ok_or("Expected classification")?;
                let coverage = ctx
                    .take::<(SuccessRubric, Vec<ResolvedExemplar>)>(EX_COVERAGE_INPUT_KEY)
                    .ok_or("No coverage input")?;
                let mut out = Context::new();
                out.set(EX_COVERAGE_INPUT_KEY, coverage);
                out.set(EX_LEDGER_KEY, ledger_classification(classification));
                out.set(EX_NONE_YET_KEY, complete_none_yet_justification());
                Ok(out)
            },
        ),
        step_def(
            "ledger classification {string} with missing none_yet justification field {string}",
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            &[
                (EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)"),
                (EX_LEDGER_KEY, "LedgerClassification"),
                (EX_NONE_YET_KEY, "NoneYetJustification"),
            ],
            |mut ctx, params| {
                let classification = params.get_string(0).ok_or("Expected classification")?;
                let missing = params.get_string(1).ok_or("Expected field")?;
                let coverage = ctx
                    .take::<(SuccessRubric, Vec<ResolvedExemplar>)>(EX_COVERAGE_INPUT_KEY)
                    .ok_or("No coverage input")?;
                let mut justification = complete_none_yet_justification();
                match missing {
                    "corpus_searched" => justification.corpus_searched.clear(),
                    "ledger_searched" => justification.ledger_searched.clear(),
                    "why_no_exemplar" => justification.why_no_exemplar.clear(),
                    "followup_condition" => justification.followup_condition.clear(),
                    other => return Err(format!("unknown none_yet field '{}'", other)),
                }
                let mut out = Context::new();
                out.set(EX_COVERAGE_INPUT_KEY, coverage);
                out.set(EX_LEDGER_KEY, ledger_classification(classification));
                out.set(EX_NONE_YET_KEY, justification);
                Ok(out)
            },
        ),
        step_def(
            "anchor coverage is validated",
            &[(EX_COVERAGE_INPUT_KEY, "(SuccessRubric, Vec<ResolvedExemplar>)")],
            &[(EX_VALIDATION_KEY, "AnchorCoverageValidation")],
            |ctx, _params| {
                let (rubric, exemplars) = ctx
                    .get::<(SuccessRubric, Vec<ResolvedExemplar>)>(EX_COVERAGE_INPUT_KEY)
                    .ok_or("No coverage input")?;
                let result = validate_anchor_coverage(
                    rubric,
                    exemplars,
                    ctx.get::<LedgerClassification>(EX_LEDGER_KEY),
                    ctx.get::<NoneYetJustification>(EX_NONE_YET_KEY),
                );
                let mut out = Context::new();
                out.set(EX_VALIDATION_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "anchor coverage validation passes",
            &[(EX_VALIDATION_KEY, "AnchorCoverageValidation")],
            |ctx, _params| {
                let validation = ctx
                    .get::<AnchorCoverageValidation>(EX_VALIDATION_KEY)
                    .ok_or("No anchor coverage validation")?;
                if validation.is_pass() {
                    Ok(())
                } else {
                    Err(format!("Expected validation pass, got {:?}", validation))
                }
            },
        ),
        check_def(
            "anchor coverage validation fails with code {string}",
            &[(EX_VALIDATION_KEY, "AnchorCoverageValidation")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected code")?;
                let validation = ctx
                    .get::<AnchorCoverageValidation>(EX_VALIDATION_KEY)
                    .ok_or("No anchor coverage validation")?;
                match validation {
                    AnchorCoverageValidation::Fail(failure) if failure.code == expected => Ok(()),
                    AnchorCoverageValidation::Fail(failure) => Err(format!(
                        "Expected failure code '{}' got '{}': {}",
                        expected, failure.code, failure.reason
                    )),
                    AnchorCoverageValidation::Pass => {
                        Err("Expected validation failure, got pass".to_string())
                    }
                }
            },
        ),
        check_def(
            "anchor coverage validation failure mentions {string}",
            &[(EX_VALIDATION_KEY, "AnchorCoverageValidation")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected text")?;
                let validation = ctx
                    .get::<AnchorCoverageValidation>(EX_VALIDATION_KEY)
                    .ok_or("No anchor coverage validation")?;
                match validation {
                    AnchorCoverageValidation::Fail(failure)
                        if failure.reason.contains(expected) =>
                    {
                        Ok(())
                    }
                    AnchorCoverageValidation::Fail(failure) => Err(format!(
                        "Expected failure reason to mention '{}', got '{}'",
                        expected, failure.reason
                    )),
                    AnchorCoverageValidation::Pass => {
                        Err("Expected validation failure, got pass".to_string())
                    }
                }
            },
        ),
    ]
}

fn ledger_classification(classification: &str) -> LedgerClassification {
    LedgerClassification {
        corpus: "fixture-corpus".to_string(),
        ledger: "fixture-ledger".to_string(),
        classification: classification.to_string(),
    }
}

fn complete_none_yet_justification() -> NoneYetJustification {
    NoneYetJustification {
        corpus_searched: "fixture corpus".to_string(),
        ledger_searched: "fixture ledger".to_string(),
        why_no_exemplar: "no comparable outcome exists yet".to_string(),
        production_routing_allowed: false,
        followup_condition: "add anchors after first production outcome".to_string(),
    }
}
