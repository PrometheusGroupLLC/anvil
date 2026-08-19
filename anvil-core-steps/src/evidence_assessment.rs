//! Step definitions for `evidence_assessment.feature` (T-EEC-2 P2a).
//!
//! The scenarios exercise the assessment as a pure domain seam. Evidence-class
//! tokens are parsed here, while strength semantics remain owned by
//! `obligation_satisfied` in anvil-core.

use anvil_core::domain::playbook::evidence_obligation::{
    assess_evidence_obligation, obligation_satisfied,
};
use anvil_core::domain::playbook::types::EvidenceClass;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const OBLIGATION_KEY: &str = "evidence_assessment_obligation";
const CLAIMED_KEY: &str = "evidence_assessment_claimed";
const RESULT_KEY: &str = "evidence_assessment_result";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssessmentView {
    status: String,
    missing_classes: Vec<EvidenceClass>,
}

fn parse_classes(list: &str) -> Result<Vec<EvidenceClass>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| match token {
            "artifact_of_consequence" => Ok(EvidenceClass::ArtifactOfConsequence),
            "verifiable_citation" => Ok(EvidenceClass::VerifiableCitation),
            "self_description" => Ok(EvidenceClass::SelfDescription),
            other => Err(format!("unknown evidence class '{}'", other)),
        })
        .collect()
}

fn class_token(class: EvidenceClass) -> &'static str {
    match class {
        EvidenceClass::ArtifactOfConsequence => "artifact_of_consequence",
        EvidenceClass::VerifiableCitation => "verifiable_citation",
        EvidenceClass::SelfDescription => "self_description",
    }
}

fn assess_evidence_under_test(
    obligation: &[EvidenceClass],
    claimed: &[EvidenceClass],
) -> Option<AssessmentView> {
    assess_evidence_obligation(obligation, claimed).map(|assessment| AssessmentView {
        status: assessment.status.as_str().to_string(),
        missing_classes: assessment.missing_classes,
    })
}

fn result(ctx: &Context) -> Result<&Option<AssessmentView>, String> {
    ctx.get::<Option<AssessmentView>>(RESULT_KEY)
        .ok_or_else(|| "No evidence assessment result in context".to_string())
}

fn carry_claims(ctx: &Context, claimed: Vec<EvidenceClass>) -> Result<Context, String> {
    let obligation = ctx
        .get::<Vec<EvidenceClass>>(OBLIGATION_KEY)
        .ok_or("No evidence obligation in context")?
        .clone();
    let mut out = Context::new();
    out.set(OBLIGATION_KEY, obligation);
    out.set(CLAIMED_KEY, claimed);
    Ok(out)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an evidence obligation of {string}",
            &[],
            &[(OBLIGATION_KEY, "Vec<EvidenceClass>")],
            |_ctx, params| {
                let obligation =
                    parse_classes(&params.get_string(0).ok_or("Expected evidence obligation")?)?;
                let mut out = Context::new();
                out.set(OBLIGATION_KEY, obligation);
                Ok(out)
            },
        ),
        step_def(
            "no evidence obligation",
            &[],
            &[(OBLIGATION_KEY, "Vec<EvidenceClass>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(OBLIGATION_KEY, Vec::<EvidenceClass>::new());
                Ok(out)
            },
        ),
        step_def(
            "claimed evidence classes of {string}",
            &[(OBLIGATION_KEY, "Vec<EvidenceClass>")],
            &[
                (OBLIGATION_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            |ctx, params| {
                let claimed =
                    parse_classes(&params.get_string(0).ok_or("Expected claimed classes")?)?;
                carry_claims(&ctx, claimed)
            },
        ),
        step_def(
            "no evidence classes are claimed",
            &[(OBLIGATION_KEY, "Vec<EvidenceClass>")],
            &[
                (OBLIGATION_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            |ctx, _params| carry_claims(&ctx, Vec::new()),
        ),
        step_def(
            "the evidence obligation is assessed",
            &[
                (OBLIGATION_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            &[
                (OBLIGATION_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
                (RESULT_KEY, "Option<AssessmentView>"),
            ],
            |ctx, _params| {
                let obligation = ctx
                    .get::<Vec<EvidenceClass>>(OBLIGATION_KEY)
                    .ok_or("No evidence obligation in context")?
                    .clone();
                let claimed = ctx
                    .get::<Vec<EvidenceClass>>(CLAIMED_KEY)
                    .ok_or("No claimed evidence in context")?
                    .clone();
                let assessment = assess_evidence_under_test(&obligation, &claimed);
                let mut out = Context::new();
                out.set(OBLIGATION_KEY, obligation);
                out.set(CLAIMED_KEY, claimed);
                out.set(RESULT_KEY, assessment);
                Ok(out)
            },
        ),
        check_def(
            "the evidence assessment status is {string}",
            &[(RESULT_KEY, "Option<AssessmentView>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected assessment status")?;
                let assessment = result(&ctx)?
                    .as_ref()
                    .ok_or("Expected an evidence assessment, but none was produced")?;
                if assessment.status == expected.as_ref() as &str {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected evidence assessment status '{}', got '{}'",
                        expected, assessment.status
                    ))
                }
            },
        ),
        check_def(
            "the evidence assessment names no missing classes",
            &[(RESULT_KEY, "Option<AssessmentView>")],
            |ctx, _params| {
                let assessment = result(&ctx)?
                    .as_ref()
                    .ok_or("Expected an evidence assessment, but none was produced")?;
                if assessment.missing_classes.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no missing evidence classes, got {:?}",
                        assessment.missing_classes
                    ))
                }
            },
        ),
        check_def(
            "the missing evidence classes are {string}",
            &[(RESULT_KEY, "Option<AssessmentView>")],
            |ctx, params| {
                let expected =
                    parse_classes(&params.get_string(0).ok_or("Expected missing classes")?)?;
                let assessment = result(&ctx)?
                    .as_ref()
                    .ok_or("Expected an evidence assessment, but none was produced")?;
                if assessment.missing_classes == expected {
                    Ok(())
                } else {
                    let actual = assessment
                        .missing_classes
                        .iter()
                        .copied()
                        .map(class_token)
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(format!(
                        "Expected missing evidence classes '{}', got '{}'",
                        params.get_string(0).unwrap_or_default(),
                        actual
                    ))
                }
            },
        ),
        check_def(
            "the evidence assessment agrees with the shared obligation predicate",
            &[
                (OBLIGATION_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
                (RESULT_KEY, "Option<AssessmentView>"),
            ],
            |ctx, _params| {
                let obligation = ctx
                    .get::<Vec<EvidenceClass>>(OBLIGATION_KEY)
                    .ok_or("No evidence obligation in context")?;
                let claimed = ctx
                    .get::<Vec<EvidenceClass>>(CLAIMED_KEY)
                    .ok_or("No claimed evidence in context")?;
                let assessment = result(&ctx)?
                    .as_ref()
                    .ok_or("Expected an evidence assessment, but none was produced")?;
                let predicate_satisfied = obligation_satisfied(obligation, claimed);
                let assessment_satisfied = assessment.status == "present-as-claimed";
                if assessment_satisfied == predicate_satisfied {
                    Ok(())
                } else {
                    Err(format!(
                        "Assessment status '{}' disagrees with shared predicate result {}",
                        assessment.status, predicate_satisfied
                    ))
                }
            },
        ),
        check_def(
            "no evidence assessment is produced",
            &[(RESULT_KEY, "Option<AssessmentView>")],
            |ctx, _params| {
                if result(&ctx)?.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected no evidence assessment, got {:?}",
                        result(&ctx)?
                    ))
                }
            },
        ),
    ]
}
