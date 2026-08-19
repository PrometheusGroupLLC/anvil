//! Steps for `continuation_lexicons.feature` — the pure predicates behind the
//! continuation decision procedure (continuation_recognition, spec r12).
//!
//! These call the production domain functions directly. That is the established
//! way anvil-core features test pure logic, and it is what keeps the rule "all
//! behavioural testing is `.feature` files through brine" honest for code that
//! has no I/O seam of its own: the feature is the test, the step module is only
//! the adapter.

use anvil_core::domain::route::{
    continuation_tokens,
    extract_prior_proposal,
    resolve_continuation,
    ContinuationInputs,
    ContinuationOutcome,
    is_affirmative,
    is_deferral,
    is_pure_consent,
    is_question,
    is_rejection,
};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{
    check_def,
    step_def,
    StepDef,
};

const MSG_KEY: &str = "continuation_message";
const PROPOSAL_KEY: &str = "extracted_proposal_text";
const PROC_SETUP_KEY: &str = "continuation_setup";
const PROC_OUTCOME_KEY: &str = "continuation_outcome";

fn msg(ctx: &Context) -> Result<String, String> {
    ctx.get::<String>(MSG_KEY)
        .cloned()
        .ok_or_else(|| "No continuation_message in context".to_string())
}

/// One assertion shape for all five predicates: name it, evaluate it, and say
/// which way it went. A separate hand-written step per predicate is how the
/// negative cases quietly stop being checked.
fn predicate_check(
    phrase: &'static str,
    expect: bool,
    f: fn(&str) -> bool,
    label: &'static str,
) -> StepDef {
    check_def(phrase, &[(MSG_KEY, "String")], move |ctx, _params| {
        let m = msg(&ctx)?;
        let got = f(&m);
        if got == expect {
            Ok(())
        } else {
            Err(format!(
                "{m:?}: expected {label}={expect}, got {got} (tokens: {:?})",
                continuation_tokens(&m)
            ))
        }
    })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "the continuation tokeniser runs on {string}",
            &[],
            &[(MSG_KEY, "String")],
            |_ctx, params| {
                let m = params.get_string(0).ok_or("Expected message")?.to_string();
                let mut out = Context::new();
                out.set(MSG_KEY, m);
                Ok(out)
            },
        ),
        check_def(
            "the continuation tokens are {string}",
            &[(MSG_KEY, "String")],
            |ctx, params| {
                let m = msg(&ctx)?;
                let want = params.get_string(0).ok_or("Expected tokens")?.to_string();
                let got = continuation_tokens(&m).join(",");
                if got == want {
                    Ok(())
                } else {
                    Err(format!("{m:?} tokenised to {got:?}, expected {want:?}"))
                }
            },
        ),
        // Feature tables cannot carry a literal newline, so `\n` in a cell means
        // a line break in the turn — the multi-line cases (a list prefix above a
        // proposal) are the whole point of several rows.
        step_def(
            "the prior proposal is extracted from {string}",
            &[],
            &[(PROPOSAL_KEY, "String")],
            |_ctx, params| {
                let turn = params
                    .get_string(0)
                    .ok_or("Expected turn")?
                    .replace("\\n", "\n");
                let got = extract_prior_proposal(&turn, false)
                    .map(|p| p.text)
                    .unwrap_or_default();
                let mut out = Context::new();
                out.set(PROPOSAL_KEY, got);
                Ok(out)
            },
        ),
        check_def(
            "the extracted proposal text is {string}",
            &[(PROPOSAL_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected text")?.to_string();
                let got = ctx.get::<String>(PROPOSAL_KEY).ok_or("No extraction")?;
                if *got == want {
                    Ok(())
                } else {
                    Err(format!("extracted {got:?}, expected {want:?}"))
                }
            },
        ),
        check_def(
            "no prior proposal is extracted",
            &[(PROPOSAL_KEY, "String")],
            |ctx, _params| {
                let got = ctx.get::<String>(PROPOSAL_KEY).ok_or("No extraction")?;
                if got.is_empty() {
                    Ok(())
                } else {
                    Err(format!("expected no proposal, extracted {got:?}"))
                }
            },
        ),
        step_def(
            "an open {string} run, new_intent {string}, context {string}, proposal {string}",
            &[],
            &[(PROC_SETUP_KEY, "String")],
            |_ctx, params| {
                // Four scalars joined rather than four context keys: the setup is
                // meaningless split apart, and a partially-set context is how a
                // scenario silently exercises a different branch than it names.
                let joined = format!(
                    "{}|{}|{}|{}",
                    params.get_string(0).unwrap_or_default(),
                    params.get_string(1).unwrap_or_default(),
                    params.get_string(2).unwrap_or_default(),
                    params.get_string(3).unwrap_or_default(),
                );
                let mut out = Context::new();
                out.set(PROC_SETUP_KEY, joined);
                Ok(out)
            },
        ),
        step_def(
            "the continuation procedure runs on {string}",
            &[(PROC_SETUP_KEY, "String")],
            &[(PROC_SETUP_KEY, "String"), (PROC_OUTCOME_KEY, "String")],
            |ctx, params| {
                let setup = ctx
                    .get::<String>(PROC_SETUP_KEY)
                    .cloned()
                    .ok_or("No setup")?;
                let parts: Vec<&str> = setup.split('|').collect();
                let open_kind = parts.first().copied().unwrap_or("");
                let proposal = parts.get(3).copied().unwrap_or("");
                let message = params.get_string(0).ok_or("Expected message")?.to_string();
                let outcome = resolve_continuation(&ContinuationInputs {
                    message: &message,
                    has_open_run: !open_kind.is_empty(),
                    open_kind,
                    has_new_intent: parts.get(1).copied().unwrap_or("") == "yes",
                    has_recent_context: parts.get(2).copied().unwrap_or("") == "yes",
                    proposal_kind: if proposal.is_empty() {
                        None
                    } else {
                        Some(proposal)
                    },
                });
                let rendered = match &outcome {
                    ContinuationOutcome::RouteNormally => "route_normally".to_string(),
                    ContinuationOutcome::Rejected => "rejected".to_string(),
                    ContinuationOutcome::ResumeContextual => "resume_contextual".to_string(),
                    ContinuationOutcome::Redirect { kind } => format!("redirect:{kind}"),
                    ContinuationOutcome::Resume => "resume".to_string(),
                    ContinuationOutcome::ResumeWidened => "resume_widened".to_string(),
                };
                let mut out = Context::new();
                out.set(PROC_SETUP_KEY, setup);
                out.set(PROC_OUTCOME_KEY, rendered);
                Ok(out)
            },
        ),
        check_def(
            "the continuation outcome is {string}",
            &[(PROC_OUTCOME_KEY, "String")],
            |ctx, params| {
                let want = params.get_string(0).ok_or("Expected outcome")?.to_string();
                let got = ctx.get::<String>(PROC_OUTCOME_KEY).ok_or("No outcome")?;
                if *got == want {
                    Ok(())
                } else {
                    Err(format!("outcome was {got:?}, expected {want:?}"))
                }
            },
        ),
        predicate_check("the message is a rejection", true, is_rejection, "rejection"),
        predicate_check(
            "the message is not a rejection",
            false,
            is_rejection,
            "rejection",
        ),
        predicate_check("the message is a deferral", true, is_deferral, "deferral"),
        predicate_check(
            "the message is not a deferral",
            false,
            is_deferral,
            "deferral",
        ),
        predicate_check(
            "the message is pure consent",
            true,
            is_pure_consent,
            "pure_consent",
        ),
        predicate_check(
            "the message is not pure consent",
            false,
            is_pure_consent,
            "pure_consent",
        ),
        predicate_check("the message is a question", true, is_question, "question"),
        predicate_check(
            "the message is not a question",
            false,
            is_question,
            "question",
        ),
        predicate_check(
            "the message is affirmative",
            true,
            is_affirmative,
            "affirmative",
        ),
        predicate_check(
            "the message is not affirmative",
            false,
            is_affirmative,
            "affirmative",
        ),
    ]
}
