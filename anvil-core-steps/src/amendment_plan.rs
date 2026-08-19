//! Step module for `amendment_plan_kind.feature` (Phase 3).
//!
//! Provides plan-kind-specific Given/When steps that mirror the spec-kind
//! counterparts in `amendment_schema` and `amendment_apply`, but use
//! `kind: "plan"` as the document kind. All core calls (validate_op, apply,
//! detect_conflicts, reverse) are unchanged — the framework is kind-agnostic.
//!
//! Also provides the shared "reorder with anchor" op-log-building step that
//! exercises the live Reorder path in apply.rs.

use crate::amendment_schema::{anchor_from_str, parse_elements};
use anvil_core::domain::amendment::apply::apply;
use anvil_core::domain::amendment::document::{ArtifactDocument, RenderedDocument};
use anvil_core::domain::amendment::op::{AmendmentOp, OpKind, OpLog};
use anvil_core::domain::amendment::schema::schema_for_kind;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{step_def, StepDef};

// Re-use the shared context keys from amendment_apply.
use crate::amendment_apply::{DOC_KEY, LOG_KEY};

const RENDER_KEY: &str = "am_render_result";
const REVERSED_LOG_KEY: &str = "am_reversed_log";

/// Carry the base doc forward so op-log steps don't lose it.
fn carry_doc(ctx: &Context, mut out: Context) -> Context {
    if let Some(doc) = ctx.get::<ArtifactDocument>(DOC_KEY) {
        out.set(DOC_KEY, doc.clone());
    }
    out
}

/// Push an op onto the log in context (or a fresh one), preserving the base doc.
fn with_pushed_op(ctx: &Context, op_id: &str, accepted_at: &str, op: AmendmentOp) -> Context {
    let mut log = ctx.get::<OpLog>(LOG_KEY).cloned().unwrap_or_default();
    log.push(op_id, accepted_at, op);
    carry_doc(ctx, Context::new().with(LOG_KEY, log))
}

/// Run apply using the named log key, storing the result and carrying forward
/// the doc + log.
fn run_apply_with(ctx: &Context, log_key: &str) -> Result<Context, String> {
    let doc = ctx
        .get::<ArtifactDocument>(DOC_KEY)
        .ok_or("no doc")?
        .clone();
    let log = ctx.get::<OpLog>(log_key).ok_or("no log")?.clone();
    let schema = schema_for_kind(&doc.kind).ok_or("no schema")?;
    let result: Result<RenderedDocument, String> =
        apply(schema, &doc, &log).map_err(|e| e.code().to_string());
    Ok(Context::new()
        .with(DOC_KEY, doc)
        .with(LOG_KEY, log)
        .with(RENDER_KEY, result))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Given: plan base document =====
        step_def(
            "a plan base document with elements:",
            &[],
            &[(DOC_KEY, "ArtifactDocument")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let elements = parse_elements(table)?;
                let doc = ArtifactDocument {
                    kind: "plan".to_string(),
                    elements,
                };
                Ok(Context::new().with(DOC_KEY, doc))
            },
        ),
        // ===== When: apply on plan base =====
        step_def(
            "apply is called on the plan base and log",
            &[(DOC_KEY, "ArtifactDocument"), (LOG_KEY, "OpLog")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
                (LOG_KEY, "OpLog"),
            ],
            |ctx, _p| run_apply_with(&ctx, LOG_KEY),
        ),
        step_def(
            "apply is called on the plan base and the reversed log",
            &[(DOC_KEY, "ArtifactDocument"), (REVERSED_LOG_KEY, "OpLog")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
                (LOG_KEY, "OpLog"),
            ],
            |ctx, _p| run_apply_with(&ctx, REVERSED_LOG_KEY),
        ),
        // ===== Shared: push a reorder op (target + anchor, no body) =====
        //
        // This step is the key path for Phase 3: it exercises the Reorder arm
        // in apply.rs (remove-then-reinsert via insert_at_anchor). There is no
        // existing shared step for reorder-with-anchor (the retire step pattern
        // carries no anchor; the add step carries a different set of params).
        step_def(
            "the log has a {string} op {string} on {string} anchored {string} accepted at {string}",
            &[(LOG_KEY, "OpLog")],
            &[(LOG_KEY, "OpLog"), (DOC_KEY, "ArtifactDocument")],
            |ctx, params| {
                let kind_str = params.get_string(0).ok_or("op kind")?;
                let op_kind = match kind_str {
                    "reorder" => OpKind::Reorder,
                    "add" => OpKind::Add,
                    other => return Err(format!("Unexpected op kind '{}' in reorder step", other)),
                };
                let op_id = params.get_string(1).ok_or("op id")?;
                let target = params.get_string(2).ok_or("target")?.to_string();
                let anchor = anchor_from_str(params.get_string(3).ok_or("anchor")?)?;
                let at = params.get_string(4).ok_or("accepted_at")?;
                let op = AmendmentOp {
                    target_id: target,
                    kind: op_kind,
                    body: None,
                    new_kind: None,
                    anchor: Some(anchor),
                };
                Ok(with_pushed_op(&ctx, op_id, at, op))
            },
        ),
    ]
}
