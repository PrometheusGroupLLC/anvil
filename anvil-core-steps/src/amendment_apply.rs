//! Step module for `amendment_apply_spec.feature`,
//! `amendment_atomicity_spec.feature`, and
//! `amendment_render_structured_only_spec.feature` (AC-3, AC-2, AC-6).
//!
//! Also hosts the SHARED op-log/base-document building steps reused by the
//! conflict/reverse/serde step modules (all share the brine `Context`).

use crate::amendment_schema::anchor_from_str;
use anvil_core::domain::amendment::apply::apply;
use anvil_core::domain::amendment::document::{ArtifactDocument, RenderedDocument};
use anvil_core::domain::amendment::op::{AmendmentOp, OpKind, OpLog};
use anvil_core::domain::amendment::render::render;
use anvil_core::domain::amendment::schema::schema_for_kind;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

pub(crate) const DOC_KEY: &str = "amv_doc";
pub(crate) const LOG_KEY: &str = "am_log";
const RENDER_KEY: &str = "am_render_result";

fn op_kind_from_str(s: &str) -> Result<OpKind, String> {
    match s {
        "add" => Ok(OpKind::Add),
        "revise" => Ok(OpKind::Revise),
        "retire" => Ok(OpKind::Retire),
        "reorder" => Ok(OpKind::Reorder),
        other => Err(format!("Unknown op kind '{}'", other)),
    }
}

/// Carry `amv_doc` forward into `out` if it is present in `ctx`.
///
/// brine's runner replaces the context with each Map step's output, retaining
/// only that step's declared `provides` keys. The op-log-building steps must
/// therefore re-emit the base document so a later `apply`/`render` step (which
/// requires `amv_doc`) still sees it.
fn carry_doc(ctx: &Context, mut out: Context) -> Context {
    if let Some(doc) = ctx.get::<ArtifactDocument>(DOC_KEY) {
        out.set(DOC_KEY, doc.clone());
    }
    out
}

/// Push an entry onto the log currently in context (or a fresh one), preserving
/// the base document.
fn with_pushed_op(ctx: &Context, op_id: &str, accepted_at: &str, op: AmendmentOp) -> Context {
    let mut log = ctx.get::<OpLog>(LOG_KEY).cloned().unwrap_or_default();
    log.push(op_id, accepted_at, op);
    carry_doc(&ctx, Context::new().with(LOG_KEY, log))
}

pub fn steps() -> Vec<StepDef> {
    vec![
        // ===== Shared: empty op log =====
        step_def(
            "an empty op log",
            &[],
            &[(LOG_KEY, "OpLog"), (DOC_KEY, "ArtifactDocument")],
            |ctx, _p| Ok(carry_doc(&ctx, Context::new().with(LOG_KEY, OpLog::new()))),
        ),
        // ===== Shared: push a revise/retire op (no anchor) =====
        step_def(
            "the log has a {string} op {string} on {string} with body {string} accepted at {string}",
            &[(LOG_KEY, "OpLog")],
            &[(LOG_KEY, "OpLog"), (DOC_KEY, "ArtifactDocument")],
            |ctx, params| {
                let kind = op_kind_from_str(params.get_string(0).ok_or("op kind")?)?;
                let op_id = params.get_string(1).ok_or("op id")?;
                let target = params.get_string(2).ok_or("target")?.to_string();
                let body = params.get_string(3).ok_or("body")?.to_string();
                let at = params.get_string(4).ok_or("accepted_at")?;
                let op = AmendmentOp {
                    target_id: target,
                    kind,
                    body: Some(body),
                    new_kind: None,
                    anchor: None,
                };
                Ok(with_pushed_op(&ctx, op_id, at, op))
            },
        ),
        // ===== Shared: push a retire op (no body) =====
        step_def(
            "the log has a {string} op {string} on {string} accepted at {string}",
            &[(LOG_KEY, "OpLog")],
            &[(LOG_KEY, "OpLog"), (DOC_KEY, "ArtifactDocument")],
            |ctx, params| {
                let kind = op_kind_from_str(params.get_string(0).ok_or("op kind")?)?;
                let op_id = params.get_string(1).ok_or("op id")?;
                let target = params.get_string(2).ok_or("target")?.to_string();
                let at = params.get_string(3).ok_or("accepted_at")?;
                let op = AmendmentOp {
                    target_id: target,
                    kind,
                    body: None,
                    new_kind: None,
                    anchor: None,
                };
                Ok(with_pushed_op(&ctx, op_id, at, op))
            },
        ),
        // ===== Shared: push an add op (new_kind + anchor) =====
        step_def(
            "the log has an {string} op {string} minting {string} of kind {string} with body {string} anchored {string} accepted at {string}",
            &[(LOG_KEY, "OpLog")],
            &[(LOG_KEY, "OpLog"), (DOC_KEY, "ArtifactDocument")],
            |ctx, params| {
                let kind = op_kind_from_str(params.get_string(0).ok_or("op kind")?)?;
                let op_id = params.get_string(1).ok_or("op id")?;
                let minted = params.get_string(2).ok_or("minted id")?.to_string();
                let new_kind = params.get_string(3).ok_or("new kind")?.to_string();
                let body = params.get_string(4).ok_or("body")?.to_string();
                let anchor = anchor_from_str(params.get_string(5).ok_or("anchor")?)?;
                let at = params.get_string(6).ok_or("accepted_at")?;
                let op = AmendmentOp {
                    target_id: minted,
                    kind,
                    body: Some(body),
                    new_kind: Some(new_kind),
                    anchor: Some(anchor),
                };
                Ok(with_pushed_op(&ctx, op_id, at, op))
            },
        ),
        // ===== When: apply =====
        step_def(
            "apply is called on the spec base and log",
            &[(DOC_KEY, "ArtifactDocument"), (LOG_KEY, "OpLog")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
                (LOG_KEY, "OpLog"),
            ],
            |ctx, _p| run_apply(&ctx, LOG_KEY),
        ),
        step_def(
            "apply is called on the spec base and the reversed log",
            &[(DOC_KEY, "ArtifactDocument"), ("am_reversed_log", "OpLog")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
                (LOG_KEY, "OpLog"),
            ],
            |ctx, _p| run_apply(&ctx, "am_reversed_log"),
        ),
        // ===== When: render =====
        step_def(
            "render is called on the spec base and log",
            &[(DOC_KEY, "ArtifactDocument"), (LOG_KEY, "OpLog")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
                (LOG_KEY, "OpLog"),
            ],
            |ctx, _p| run_render(&ctx, LOG_KEY),
        ),
        step_def(
            "render is called on the spec base and an empty log",
            &[(DOC_KEY, "ArtifactDocument")],
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
            ],
            |ctx, _p| {
                // Use a fresh empty log so atomicity's "unchanged doc" check is honest.
                let doc = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("no doc")?.clone();
                let schema = schema_for_kind(&doc.kind).ok_or("no schema")?;
                let result: Result<RenderedDocument, String> =
                    render(schema, &doc, &OpLog::new()).map_err(|e| e.code().to_string());
                Ok(Context::new()
                    .with(DOC_KEY, doc)
                    .with(RENDER_KEY, result))
            },
        ),
        // ===== Then: success/failure =====
        check_def(
            "apply succeeds",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, _p| assert_ok(&ctx),
        ),
        check_def(
            "render succeeds",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, _p| assert_ok(&ctx),
        ),
        check_def(
            "apply fails with code {string}",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("code")?;
                let r = ctx
                    .get::<Result<RenderedDocument, String>>(RENDER_KEY)
                    .ok_or("no result")?;
                match r {
                    Ok(_) => Err(format!("Expected error '{}' but apply succeeded", expected)),
                    Err(c) if c == expected => Ok(()),
                    Err(c) => Err(format!("Expected '{}' got '{}'", expected, c)),
                }
            },
        ),
        // ===== Then: element count =====
        check_def(
            "the rendered document has {int} elements",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, params| {
                let n: usize = params.get_int(0).ok_or("count")?.try_into().map_err(|_| "neg")?;
                let doc = rendered(&ctx)?;
                if doc.elements.len() == n {
                    Ok(())
                } else {
                    Err(format!("expected {} elements, got {}", n, doc.elements.len()))
                }
            },
        ),
        // ===== Then: element body =====
        check_def(
            "rendered element {string} has body {string}",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("id")?;
                let body = params.get_string(1).ok_or("body")?;
                let doc = rendered(&ctx)?;
                let el = doc
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .ok_or_else(|| format!("no element '{}'", id))?;
                if el.body == body {
                    Ok(())
                } else {
                    Err(format!("element '{}' body: expected '{}' got '{}'", id, body, el.body))
                }
            },
        ),
        // ===== Then: element at position =====
        check_def(
            "rendered element at position {int} has id {string}",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, params| {
                let pos: usize = params.get_int(0).ok_or("pos")?.try_into().map_err(|_| "neg")?;
                let id = params.get_string(1).ok_or("id")?;
                let doc = rendered(&ctx)?;
                let el = doc
                    .elements
                    .get(pos)
                    .ok_or_else(|| format!("no element at position {}", pos))?;
                if el.id == id {
                    Ok(())
                } else {
                    Err(format!("position {}: expected id '{}' got '{}'", pos, id, el.id))
                }
            },
        ),
        // ===== Then: absence =====
        check_def(
            "the rendered document does not contain element {string}",
            &[(RENDER_KEY, "Result<RenderedDocument, String>")],
            |ctx, params| {
                let id = params.get_string(0).ok_or("id")?;
                let doc = rendered(&ctx)?;
                if doc.elements.iter().any(|e| e.id == id) {
                    Err(format!("element '{}' should be omitted but is present", id))
                } else {
                    Ok(())
                }
            },
        ),
        // ===== Then: op still in log =====
        check_def(
            "the op log still contains op {string}",
            &[(LOG_KEY, "OpLog")],
            |ctx, params| {
                let op_id = params.get_string(0).ok_or("op id")?;
                let log = ctx.get::<OpLog>(LOG_KEY).ok_or("no log")?;
                if log.entries().iter().any(|e| e.op_id == op_id) {
                    Ok(())
                } else {
                    Err(format!("op '{}' not retained in log", op_id))
                }
            },
        ),
        // ===== Then: rendered equals base =====
        check_def(
            "the rendered document equals the base document",
            &[
                (RENDER_KEY, "Result<RenderedDocument, String>"),
                (DOC_KEY, "ArtifactDocument"),
            ],
            |ctx, _p| {
                let doc = rendered(&ctx)?;
                let base = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("no base")?;
                if doc.kind == base.kind && doc.elements == base.elements {
                    Ok(())
                } else {
                    Err(format!(
                        "rendered != base: rendered {:?}, base {:?}",
                        doc.elements, base.elements
                    ))
                }
            },
        ),
    ]
}

fn run_apply(ctx: &Context, log_key: &str) -> Result<Context, String> {
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

fn run_render(ctx: &Context, log_key: &str) -> Result<Context, String> {
    let doc = ctx
        .get::<ArtifactDocument>(DOC_KEY)
        .ok_or("no doc")?
        .clone();
    let log = ctx.get::<OpLog>(log_key).ok_or("no log")?.clone();
    let schema = schema_for_kind(&doc.kind).ok_or("no schema")?;
    let result: Result<RenderedDocument, String> =
        render(schema, &doc, &log).map_err(|e| e.code().to_string());
    Ok(Context::new()
        .with(DOC_KEY, doc)
        .with(LOG_KEY, log)
        .with(RENDER_KEY, result))
}

fn assert_ok(ctx: &Context) -> Result<(), String> {
    let r = ctx
        .get::<Result<RenderedDocument, String>>(RENDER_KEY)
        .ok_or("no result")?;
    match r {
        Ok(_) => Ok(()),
        Err(c) => Err(format!("expected success, got error '{}'", c)),
    }
}

fn rendered(ctx: &Context) -> Result<&RenderedDocument, String> {
    let r = ctx
        .get::<Result<RenderedDocument, String>>(RENDER_KEY)
        .ok_or("no result")?;
    r.as_ref()
        .map_err(|c| format!("apply/render errored: {}", c))
}
