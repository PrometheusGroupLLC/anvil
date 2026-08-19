//! Step module for evidence_obligation_predicate.feature (T-EEC-1 Phase 1).
//!
//! Parses a required-class list and a claimed-class list from step arguments,
//! calls the pure `obligation_satisfied` predicate, and asserts the boolean.

use anvil_core::domain::playbook::evidence_obligation::obligation_satisfied;
use anvil_core::domain::playbook::types::EvidenceClass;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const REQUIRED_KEY: &str = "eo_required";
const CLAIMED_KEY: &str = "eo_claimed";

/// Parse a comma-separated evidence-class list. An empty/whitespace string is
/// an empty set.
fn parse_classes(list: &str) -> Result<Vec<EvidenceClass>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|token| match token {
            "artifact_of_consequence" => Ok(EvidenceClass::ArtifactOfConsequence),
            "verifiable_citation" => Ok(EvidenceClass::VerifiableCitation),
            "self_description" => Ok(EvidenceClass::SelfDescription),
            other => Err(format!("unknown evidence class '{}'", other)),
        })
        .collect()
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "an evidence obligation requires {string}",
            &[],
            &[(REQUIRED_KEY, "Vec<EvidenceClass>")],
            |_ctx, params| {
                let required = parse_classes(&params.get_string(0).ok_or("Expected required classes")?)?;
                let mut out = Context::new();
                out.set(REQUIRED_KEY, required);
                Ok(out)
            },
        ),
        step_def(
            "the actor claims {string}",
            &[(REQUIRED_KEY, "Vec<EvidenceClass>")],
            &[
                (REQUIRED_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            |ctx, params| {
                let claimed = parse_classes(&params.get_string(0).ok_or("Expected claimed classes")?)?;
                let required = ctx
                    .get::<Vec<EvidenceClass>>(REQUIRED_KEY)
                    .ok_or("No required classes in context")?
                    .clone();
                let mut out = Context::new();
                out.set(REQUIRED_KEY, required);
                out.set(CLAIMED_KEY, claimed);
                Ok(out)
            },
        ),
        check_def(
            "the obligation is satisfied",
            &[
                (REQUIRED_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            |ctx, _params| {
                let required = ctx.get::<Vec<EvidenceClass>>(REQUIRED_KEY).ok_or("No required")?;
                let claimed = ctx.get::<Vec<EvidenceClass>>(CLAIMED_KEY).ok_or("No claimed")?;
                if obligation_satisfied(required, claimed) {
                    Ok(())
                } else {
                    Err(format!(
                        "expected obligation SATISFIED but predicate returned false (required {:?}, claimed {:?})",
                        required, claimed
                    ))
                }
            },
        ),
        check_def(
            "the obligation is not satisfied",
            &[
                (REQUIRED_KEY, "Vec<EvidenceClass>"),
                (CLAIMED_KEY, "Vec<EvidenceClass>"),
            ],
            |ctx, _params| {
                let required = ctx.get::<Vec<EvidenceClass>>(REQUIRED_KEY).ok_or("No required")?;
                let claimed = ctx.get::<Vec<EvidenceClass>>(CLAIMED_KEY).ok_or("No claimed")?;
                if obligation_satisfied(required, claimed) {
                    Err(format!(
                        "expected obligation NOT satisfied but predicate returned true (required {:?}, claimed {:?})",
                        required, claimed
                    ))
                } else {
                    Ok(())
                }
            },
        ),
    ]
}
