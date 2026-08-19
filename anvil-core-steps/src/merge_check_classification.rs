//! Step definitions for `merge_check_classification.feature`.
//!
//! A pure domain seam: no git, no filesystem, no engine. Resolutions are handed
//! in by token so the VERDICT rule can be exercised independently of what the
//! git adapter happens to be able to answer today — the two are separate
//! surfaces and a bug in either must be able to show up alone.

use anvil_core::domain::merge_check::{
    arms_merge_check, classify_reference, merge_check_verdict, ClaimResolution, CodeClaim,
    MergeCheckRefusal,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const CLASSIFIED_KEY: &str = "merge_check_classified";
const CLAIMS_KEY: &str = "merge_check_claims";
const VERDICT_KEY: &str = "merge_check_verdict";
const ARMED_KEY: &str = "merge_check_armed";

/// The fixed repository label every seeded resolution carries. A real label is
/// an absolute path; the scenarios only need it to be recognisable.
const REPO_LABEL: &str = "/repos/lore";
/// The branch a seeded `not-merged` commit lives on, asserted by name so the
/// "where is it then?" half of the message cannot silently disappear.
const STRANDED_BRANCH: &str = "example-org/stranded";

fn kind_token(claim: &CodeClaim) -> &'static str {
    match claim {
        CodeClaim::Commit { .. } => "commit",
        CodeClaim::Path { .. } => "path",
        CodeClaim::NotCode => "not-code",
    }
}

fn repo_token(claim: &CodeClaim) -> String {
    match claim {
        CodeClaim::Commit { repo, .. } | CodeClaim::Path { repo, .. } => {
            repo.clone().unwrap_or_default()
        }
        CodeClaim::NotCode => String::new(),
    }
}

fn target_token(claim: &CodeClaim) -> String {
    match claim {
        CodeClaim::Commit { rev, .. } => rev.clone(),
        CodeClaim::Path { path, .. } => path.clone(),
        CodeClaim::NotCode => String::new(),
    }
}

/// Build a resolution from the scenario's token. Every refusing variant is
/// reachable here, so the verdict rule is exercised over its whole domain
/// rather than over the one or two variants a git fixture happens to produce.
fn resolution_from_token(token: &str, reference: &str) -> Result<ClaimResolution, String> {
    Ok(match token {
        "not-applicable" => ClaimResolution::NotApplicable,
        "merged" => ClaimResolution::CommitMerged {
            repo_label: REPO_LABEL.to_string(),
            resolved_sha: sha_of(reference),
        },
        "not-merged" => ClaimResolution::CommitNotMerged {
            repo_label: REPO_LABEL.to_string(),
            resolved_sha: sha_of(reference),
            origin_main_sha: "0000000000000000000000000000000000000000".to_string(),
            branches: vec![STRANDED_BRANCH.to_string()],
        },
        "commit-unknown" => ClaimResolution::CommitUnknown {
            repo_label: REPO_LABEL.to_string(),
            rev: sha_of(reference),
        },
        "no-origin-main" => ClaimResolution::MissingOriginMain {
            repo_label: REPO_LABEL.to_string(),
        },
        "path-present" => ClaimResolution::PathPresent {
            repo_label: REPO_LABEL.to_string(),
            resolved: reference.to_string(),
        },
        "path-missing" => ClaimResolution::PathMissing {
            repo_label: REPO_LABEL.to_string(),
            searched: vec![reference.to_string()],
        },
        "repo-unresolved" => ClaimResolution::RepoUnresolved {
            repo_name: "nosuchrepo".to_string(),
            searched: vec!["/repos".to_string()],
        },
        "verifier-unavailable" => ClaimResolution::VerifierUnavailable {
            detail: "git would not run".to_string(),
        },
        other => return Err(format!("unknown resolution token '{}'", other)),
    })
}

/// The SHA a `commit:<repo>@<sha>` reference names, for message assertions.
fn sha_of(reference: &str) -> String {
    match classify_reference(reference) {
        CodeClaim::Commit { rev, .. } => rev,
        _ => reference.to_string(),
    }
}

fn claims(ctx: &Context) -> Vec<(String, ClaimResolution)> {
    ctx.get::<Vec<(String, ClaimResolution)>>(CLAIMS_KEY)
        .cloned()
        .unwrap_or_default()
}

fn verdict(ctx: &Context) -> Result<&Result<(), MergeCheckRefusal>, String> {
    ctx.get::<Result<(), MergeCheckRefusal>>(VERDICT_KEY)
        .ok_or_else(|| "No merge check verdict in context".to_string())
}

fn refusal(ctx: &Context) -> Result<&MergeCheckRefusal, String> {
    match verdict(ctx)? {
        Ok(()) => Err("expected a refusal, the merge check passed".to_string()),
        Err(refusal) => Ok(refusal),
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the claimed-evidence reference {string} is classified",
            &[],
            &[(CLASSIFIED_KEY, "CodeClaim")],
            |_ctx, params| {
                let reference = params.get_string(0).unwrap_or_default();
                let mut out = Context::new();
                out.set(CLASSIFIED_KEY, classify_reference(&reference));
                Ok(out)
            },
        ),
        check_def(
            "the reference is classified as {string}",
            &[(CLASSIFIED_KEY, "CodeClaim")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected a kind token")?;
                let claim = ctx
                    .get::<CodeClaim>(CLASSIFIED_KEY)
                    .ok_or("No classified claim")?;
                if kind_token(claim) == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected kind '{}', got '{}' ({:?})",
                        expected,
                        kind_token(claim),
                        claim
                    ))
                }
            },
        ),
        check_def(
            "the classified repository is {string}",
            &[(CLASSIFIED_KEY, "CodeClaim")],
            |ctx, params| {
                let expected = params.get_string(0).unwrap_or_default();
                let claim = ctx
                    .get::<CodeClaim>(CLASSIFIED_KEY)
                    .ok_or("No classified claim")?;
                let actual = repo_token(claim);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected repository '{}', got '{}'",
                        expected, actual
                    ))
                }
            },
        ),
        check_def(
            "the classified target is {string}",
            &[(CLASSIFIED_KEY, "CodeClaim")],
            |ctx, params| {
                let expected = params.get_string(0).unwrap_or_default();
                let claim = ctx
                    .get::<CodeClaim>(CLASSIFIED_KEY)
                    .ok_or("No classified claim")?;
                let actual = target_token(claim);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected target '{}', got '{}'", expected, actual))
                }
            },
        ),
        step_def(
            "no claimed evidence is presented",
            &[],
            &[(CLAIMS_KEY, "Vec<(String, ClaimResolution)>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set::<Vec<(String, ClaimResolution)>>(CLAIMS_KEY, Vec::new());
                Ok(out)
            },
        ),
        step_def(
            "a claim {string} resolved as {string}",
            &[],
            &[(CLAIMS_KEY, "Vec<(String, ClaimResolution)>")],
            |ctx, params| {
                let reference = params.get_string(0).ok_or("Expected a reference")?;
                let token = params.get_string(1).ok_or("Expected a resolution token")?;
                let mut all = claims(&ctx);
                all.push((
                    reference.to_string(),
                    resolution_from_token(&token, &reference)?,
                ));
                let mut out = Context::new();
                out.set(CLAIMS_KEY, all);
                Ok(out)
            },
        ),
        step_def(
            "the merge check verdict is taken",
            &[(CLAIMS_KEY, "Vec<(String, ClaimResolution)>")],
            &[
                (CLAIMS_KEY, "Vec<(String, ClaimResolution)>"),
                (VERDICT_KEY, "Result<(), MergeCheckRefusal>"),
            ],
            |ctx, _params| {
                let all = claims(&ctx);
                let result = merge_check_verdict(&all);
                let mut out = Context::new();
                out.set(CLAIMS_KEY, all);
                out.set(VERDICT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the merge check passes",
            &[(VERDICT_KEY, "Result<(), MergeCheckRefusal>")],
            |ctx, _params| match verdict(&ctx)? {
                Ok(()) => Ok(()),
                Err(refusal) => Err(format!("expected a pass, got: {}", refusal)),
            },
        ),
        check_def(
            "the merge check refuses",
            &[(VERDICT_KEY, "Result<(), MergeCheckRefusal>")],
            |ctx, _params| {
                refusal(&ctx)?;
                Ok(())
            },
        ),
        check_def(
            "the refusal counts {int} code claims examined and {int} refused",
            &[(VERDICT_KEY, "Result<(), MergeCheckRefusal>")],
            |ctx, params| {
                let expected_examined =
                    params.get_int(0).ok_or("Expected an examined count")? as usize;
                let expected_refused =
                    params.get_int(1).ok_or("Expected a refused count")? as usize;
                let refusal = refusal(&ctx)?;
                if refusal.code_claims_examined != expected_examined {
                    return Err(format!(
                        "expected {} code claims examined, got {}",
                        expected_examined, refusal.code_claims_examined
                    ));
                }
                if refusal.refused.len() != expected_refused {
                    return Err(format!(
                        "expected {} refused, got {}",
                        expected_refused,
                        refusal.refused.len()
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the refusal message names {string}",
            &[(VERDICT_KEY, "Result<(), MergeCheckRefusal>")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected a needle")?;
                let refusal = refusal(&ctx)?;
                let message = refusal.to_string();
                if message.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!(
                        "refusal message did not name '{}'. Message: {}",
                        needle, message
                    ))
                }
            },
        ),
        step_def(
            "the destination state {string} is offered to the merge check",
            &[],
            &[(ARMED_KEY, "bool")],
            |_ctx, params| {
                let state = params.get_string(0).ok_or("Expected a state")?;
                let mut out = Context::new();
                out.set(ARMED_KEY, arms_merge_check(&state));
                Ok(out)
            },
        ),
        check_def(
            "the merge check is {string}",
            &[(ARMED_KEY, "bool")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected armed|disarmed")?;
                let armed = *ctx.get::<bool>(ARMED_KEY).ok_or("No armed flag")?;
                let actual = if armed { "armed" } else { "disarmed" };
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!("expected '{}', got '{}'", expected, actual))
                }
            },
        ),
    ]
}
