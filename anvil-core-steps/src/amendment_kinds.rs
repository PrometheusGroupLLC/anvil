//! Step module for the 6 fan-out kind features (Phases 4–9) and the Phase-11
//! all-kinds-present scenario.
//!
//! Provides:
//! - Per-kind "a <kind> base document with elements:" Given steps for proposal,
//!   track, milestone, initiative, decision, and learning.
//! - Per-kind "apply is called on the <kind> base and log" When steps.
//! - Per-kind "apply is called on the <kind> base and the reversed log" When steps.
//! - "schema_for_kind is called for kind {string}" When step + schema presence
//!   Then steps for the all-kinds-present scenario.
//!
//! All core calls (validate_op, apply, detect_conflicts, reverse) are unchanged —
//! the framework is kind-agnostic. The per-kind steps only differ in the kind
//! string written into ArtifactDocument::kind so schema_for_kind resolves correctly.

use crate::amendment_apply::{DOC_KEY, LOG_KEY};
use crate::amendment_schema::parse_elements;
use anvil_core::domain::amendment::apply::apply;
use anvil_core::domain::amendment::document::{ArtifactDocument, RenderedDocument};
use anvil_core::domain::amendment::op::OpLog;
use anvil_core::domain::amendment::schema::schema_for_kind;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const RENDER_KEY: &str = "am_render_result";
const REVERSED_LOG_KEY: &str = "am_reversed_log";
const SCHEMA_KEY: &str = "am_schema_result";

/// Carry `DOC_KEY` forward into `out` if it is present in `ctx`.
fn carry_doc(ctx: &Context, mut out: Context) -> Context {
    if let Some(doc) = ctx.get::<ArtifactDocument>(DOC_KEY) {
        out.set(DOC_KEY, doc.clone());
    }
    out
}

/// Run apply using the named log key, storing the result and carrying forward
/// the doc + log.
fn run_apply_with(ctx: &Context, log_key: &str) -> Result<Context, String> {
    let doc = ctx
        .get::<ArtifactDocument>(DOC_KEY)
        .ok_or("no doc")?
        .clone();
    let log = ctx.get::<OpLog>(log_key).ok_or("no log")?.clone();
    let schema =
        schema_for_kind(&doc.kind).ok_or_else(|| format!("no schema for kind '{}'", doc.kind))?;
    let result: Result<RenderedDocument, String> =
        apply(schema, &doc, &log).map_err(|e| e.code().to_string());
    Ok(Context::new()
        .with(DOC_KEY, doc)
        .with(LOG_KEY, log)
        .with(RENDER_KEY, result))
}

/// Build the base-document Given step for a specific kind.
fn base_doc_step(kind: &'static str, pattern: &'static str) -> StepDef {
    step_def(
        pattern,
        &[],
        &[(DOC_KEY, "ArtifactDocument")],
        move |_ctx, params| {
            let table = params.data_table().ok_or("Expected a data table")?;
            let elements = parse_elements(table)?;
            let doc = ArtifactDocument {
                kind: kind.to_string(),
                elements,
            };
            Ok(Context::new().with(DOC_KEY, doc))
        },
    )
}

/// Build the "apply is called on the <kind> base and log" When step.
fn apply_step(kind: &'static str, pattern: &'static str) -> StepDef {
    step_def(
        pattern,
        &[(DOC_KEY, "ArtifactDocument"), (LOG_KEY, "OpLog")],
        &[
            (RENDER_KEY, "Result<RenderedDocument, String>"),
            (DOC_KEY, "ArtifactDocument"),
            (LOG_KEY, "OpLog"),
        ],
        move |ctx, _p| {
            // verify the doc kind matches expectations (defensive)
            let doc = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("no doc")?;
            if doc.kind != kind {
                return Err(format!("expected kind '{}', got '{}'", kind, doc.kind));
            }
            run_apply_with(&ctx, LOG_KEY)
        },
    )
}

/// Build the "apply is called on the <kind> base and the reversed log" When step.
fn apply_reversed_step(kind: &'static str, pattern: &'static str) -> StepDef {
    step_def(
        pattern,
        &[(DOC_KEY, "ArtifactDocument"), (REVERSED_LOG_KEY, "OpLog")],
        &[
            (RENDER_KEY, "Result<RenderedDocument, String>"),
            (DOC_KEY, "ArtifactDocument"),
            (LOG_KEY, "OpLog"),
        ],
        move |ctx, _p| {
            let doc = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("no doc")?;
            if doc.kind != kind {
                return Err(format!("expected kind '{}', got '{}'", kind, doc.kind));
            }
            run_apply_with(&ctx, REVERSED_LOG_KEY)
        },
    )
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== proposal kind =====
        base_doc_step("proposal", "a proposal base document with elements:"),
        apply_step("proposal", "apply is called on the proposal base and log"),
        apply_reversed_step(
            "proposal",
            "apply is called on the proposal base and the reversed log",
        ),
        // ===== track kind =====
        base_doc_step("track", "a track base document with elements:"),
        apply_step("track", "apply is called on the track base and log"),
        apply_reversed_step(
            "track",
            "apply is called on the track base and the reversed log",
        ),
        // ===== milestone kind =====
        base_doc_step("milestone", "a milestone base document with elements:"),
        apply_step("milestone", "apply is called on the milestone base and log"),
        apply_reversed_step(
            "milestone",
            "apply is called on the milestone base and the reversed log",
        ),
        // ===== initiative kind =====
        base_doc_step("initiative", "an initiative base document with elements:"),
        apply_step(
            "initiative",
            "apply is called on the initiative base and log",
        ),
        apply_reversed_step(
            "initiative",
            "apply is called on the initiative base and the reversed log",
        ),
        // ===== decision kind =====
        base_doc_step("decision", "a decision base document with elements:"),
        apply_step("decision", "apply is called on the decision base and log"),
        apply_reversed_step(
            "decision",
            "apply is called on the decision base and the reversed log",
        ),
        // ===== learning kind =====
        base_doc_step("learning", "a learning base document with elements:"),
        apply_step("learning", "apply is called on the learning base and log"),
        apply_reversed_step(
            "learning",
            "apply is called on the learning base and the reversed log",
        ),
        // ===== Phase-11: all-kinds-present schema presence steps =====
        step_def(
            "schema_for_kind is called for kind {string}",
            &[],
            &[(SCHEMA_KEY, "Option<bool>")],
            |_ctx, params| {
                let kind = params.get_string(0).ok_or("kind")?;
                let result: Option<bool> =
                    schema_for_kind(kind).map(|s| !s.element_kinds.is_empty());
                Ok(Context::new().with(SCHEMA_KEY, result))
            },
        ),
        check_def(
            "the schema is present",
            &[(SCHEMA_KEY, "Option<bool>")],
            |ctx, _p| {
                let r = ctx
                    .get::<Option<bool>>(SCHEMA_KEY)
                    .ok_or("no schema result")?;
                if r.is_some() {
                    Ok(())
                } else {
                    Err("schema_for_kind returned None (schema absent)".to_string())
                }
            },
        ),
        check_def(
            "the schema has at least one element kind",
            &[(SCHEMA_KEY, "Option<bool>")],
            |ctx, _p| {
                let r = ctx
                    .get::<Option<bool>>(SCHEMA_KEY)
                    .ok_or("no schema result")?;
                match r {
                    Some(true) => Ok(()),
                    Some(false) => {
                        Err("schema is present but has zero element kinds (stub)".to_string())
                    }
                    None => Err("schema_for_kind returned None".to_string()),
                }
            },
        ),
        check_def(
            "the schema is absent",
            &[(SCHEMA_KEY, "Option<bool>")],
            |ctx, _p| {
                let r = ctx
                    .get::<Option<bool>>(SCHEMA_KEY)
                    .ok_or("no schema result")?;
                if r.is_none() {
                    Ok(())
                } else {
                    Err("expected schema to be absent but it was present".to_string())
                }
            },
        ),
    ]
}
