use anvil_core::ports::session_verifier::{SessionMode, VerifiedSession, VerifyError};
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{step_def, StepDef};

/// A stub verify outcome stored in the brine context, representing what the
/// `SessionVerifier` port would return.  Using a plain enum here avoids a
/// real broker dep in tests.
#[derive(Debug, Clone)]
pub enum StubOutcome {
    OkNone,
    OkSome(VerifiedSession),
    Err,
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ── Given: verify outcome ──────────────────────────────────────────────
        step_def(
            "the verify outcome is Ok(None)",
            &[],
            &[("verify_outcome", "StubOutcome")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("verify_outcome", StubOutcome::OkNone);
                Ok(out)
            },
        ),
        step_def(
            "the verify outcome is Ok(Some(session)) with sub {string} and sid {string}",
            &[],
            &[("verify_outcome", "StubOutcome")],
            |_ctx, params| {
                let sub = params.get_string(0).ok_or("Expected sub")?.to_string();
                let sid = params.get_string(1).ok_or("Expected sid")?.to_string();
                let session = VerifiedSession {
                    sub,
                    sid,
                    provider: "test".to_string(),
                    scopes: vec![],
                    audience: "foundry-mcp:anvil-kit".to_string(),
                    expires_at: i64::MAX,
                };
                let mut out = Context::new();
                out.set("verify_outcome", StubOutcome::OkSome(session));
                Ok(out)
            },
        ),
        step_def(
            "the verify outcome is Err(BadSignature)",
            &[],
            &[("verify_outcome", "StubOutcome")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set("verify_outcome", StubOutcome::Err);
                Ok(out)
            },
        ),
        // ── Given: raw token ──────────────────────────────────────────────────
        // These steps require and re-emit `verify_outcome` so it is still
        // present in the context when the `When` step runs.
        step_def(
            "the raw token is absent",
            &[("verify_outcome", "StubOutcome")],
            &[
                ("verify_outcome", "StubOutcome"),
                ("raw_token", "Option<String>"),
            ],
            |mut ctx, _params| {
                let outcome = ctx
                    .take::<StubOutcome>("verify_outcome")
                    .ok_or("No verify_outcome")?;
                let token: Option<String> = None;
                let mut out = Context::new();
                out.set("verify_outcome", outcome);
                out.set("raw_token", token);
                Ok(out)
            },
        ),
        step_def(
            "the raw token is whitespace-only",
            &[("verify_outcome", "StubOutcome")],
            &[
                ("verify_outcome", "StubOutcome"),
                ("raw_token", "Option<String>"),
            ],
            |mut ctx, _params| {
                let outcome = ctx
                    .take::<StubOutcome>("verify_outcome")
                    .ok_or("No verify_outcome")?;
                let mut out = Context::new();
                out.set("verify_outcome", outcome);
                out.set("raw_token", Some("   \t\n  ".to_string()));
                Ok(out)
            },
        ),
        step_def(
            "the raw token is present",
            &[("verify_outcome", "StubOutcome")],
            &[
                ("verify_outcome", "StubOutcome"),
                ("raw_token", "Option<String>"),
            ],
            |mut ctx, _params| {
                let outcome = ctx
                    .take::<StubOutcome>("verify_outcome")
                    .ok_or("No verify_outcome")?;
                let mut out = Context::new();
                out.set("verify_outcome", outcome);
                out.set("raw_token", Some("some.jwt.token".to_string()));
                Ok(out)
            },
        ),
        // ── When ──────────────────────────────────────────────────────────────
        step_def(
            "session mode is decided",
            &[
                ("verify_outcome", "StubOutcome"),
                ("raw_token", "Option<String>"),
            ],
            &[("session_mode", "SessionMode")],
            |mut ctx, _params| {
                let outcome = ctx
                    .take::<StubOutcome>("verify_outcome")
                    .ok_or("No verify_outcome")?;
                let raw_token = ctx
                    .take::<Option<String>>("raw_token")
                    .ok_or("No raw_token")?;

                // Apply the trim rule: empty-after-trim ⇒ absent.
                let token_present_and_trimmed = raw_token
                    .as_deref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .is_some();

                let verify_result: Result<Option<VerifiedSession>, VerifyError> = match outcome {
                    StubOutcome::OkNone => Ok(None),
                    StubOutcome::OkSome(session) => Ok(Some(session)),
                    StubOutcome::Err => Err(VerifyError::BadSignature),
                };

                let mode = SessionMode::decide(token_present_and_trimmed, verify_result);
                let mut out = Context::new();
                out.set("session_mode", mode);
                Ok(out)
            },
        ),
        // ── Then ──────────────────────────────────────────────────────────────
        step_def(
            "the session mode is Standalone",
            &[("session_mode", "SessionMode")],
            &[],
            |mut ctx, _params| {
                let mode = ctx
                    .take::<SessionMode>("session_mode")
                    .ok_or("No session_mode")?;
                match mode {
                    SessionMode::Standalone => Ok(Context::new()),
                    other => Err(format!("Expected Standalone, got {:?}", other).into()),
                }
            },
        ),
        step_def(
            "the session mode is Foundry with sub {string}",
            &[("session_mode", "SessionMode")],
            &[],
            |mut ctx, params| {
                let expected_sub = params.get_string(0).ok_or("Expected sub")?.to_string();
                let mode = ctx
                    .take::<SessionMode>("session_mode")
                    .ok_or("No session_mode")?;
                match mode {
                    SessionMode::Foundry(session) if session.sub == expected_sub => {
                        Ok(Context::new())
                    }
                    SessionMode::Foundry(session) => Err(format!(
                        "Expected Foundry with sub {:?}, got sub {:?}",
                        expected_sub, session.sub
                    )
                    .into()),
                    other => Err(format!("Expected Foundry, got {:?}", other).into()),
                }
            },
        ),
        step_def(
            "the session mode is Refuse",
            &[("session_mode", "SessionMode")],
            &[],
            |mut ctx, _params| {
                let mode = ctx
                    .take::<SessionMode>("session_mode")
                    .ok_or("No session_mode")?;
                match mode {
                    SessionMode::Refuse => Ok(Context::new()),
                    other => Err(format!("Expected Refuse, got {:?}", other).into()),
                }
            },
        ),
    ]
}
