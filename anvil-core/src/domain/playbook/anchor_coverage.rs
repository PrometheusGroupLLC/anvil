//! Pure anchor coverage validation for generated playbook review guidance.
//!
//! This is intentionally a library check, not a complete-time engine gate.

use super::candidate::{LedgerClassification, NoneYetJustification};
use super::exemplar_resolver::{exemplar_coverage, ResolvedExemplar};
use super::types::SuccessRubric;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorCoverageValidation {
    Pass,
    Fail(AnchorCoverageFailure),
}

impl AnchorCoverageValidation {
    pub fn is_pass(&self) -> bool {
        matches!(self, AnchorCoverageValidation::Pass)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorCoverageFailure {
    pub code: &'static str,
    pub reason: String,
}

pub fn validate_anchor_coverage(
    rubric: &SuccessRubric,
    exemplars: &[ResolvedExemplar],
    ledger_classification: Option<&LedgerClassification>,
    none_yet_justification: Option<&NoneYetJustification>,
) -> AnchorCoverageValidation {
    let coverage = exemplar_coverage(rubric, exemplars);
    if coverage.is_complete() {
        return AnchorCoverageValidation::Pass;
    }

    if is_none_yet(ledger_classification) {
        return validate_none_yet_justification(none_yet_justification)
            .unwrap_or_else(AnchorCoverageValidation::Fail);
    }

    AnchorCoverageValidation::Fail(AnchorCoverageFailure {
        code: "anchor_coverage_uncovered_dimensions",
        reason: format!(
            "rubric-scored dimensions lack resolved exemplar anchors: {}",
            coverage.uncovered.join(", ")
        ),
    })
}

fn is_none_yet(ledger_classification: Option<&LedgerClassification>) -> bool {
    ledger_classification
        .map(|classification| classification.classification == "none_yet")
        .unwrap_or(false)
}

fn validate_none_yet_justification(
    justification: Option<&NoneYetJustification>,
) -> Result<AnchorCoverageValidation, AnchorCoverageFailure> {
    let Some(justification) = justification else {
        return Err(incomplete_none_yet("none_yet_justification"));
    };

    let missing = [
        ("corpus_searched", justification.corpus_searched.as_str()),
        ("ledger_searched", justification.ledger_searched.as_str()),
        ("why_no_exemplar", justification.why_no_exemplar.as_str()),
        (
            "followup_condition",
            justification.followup_condition.as_str(),
        ),
    ]
    .into_iter()
    .filter_map(|(field, value)| value.trim().is_empty().then_some(field))
    .collect::<Vec<_>>();

    if missing.is_empty() {
        Ok(AnchorCoverageValidation::Pass)
    } else {
        Err(incomplete_none_yet(&missing.join(", ")))
    }
}

fn incomplete_none_yet(field: &str) -> AnchorCoverageFailure {
    AnchorCoverageFailure {
        code: "anchor_coverage_incomplete_none_yet_justification",
        reason: format!(
            "ledger_classification none_yet requires complete none_yet_justification; missing {}",
            field
        ),
    }
}
