//! Apply — the pure fold composing base + ordered op log → rendered projection
//! (AC-3, AC-2 atomicity).
//!
//! `apply(schema, base, log) -> Result<RenderedDocument, AmendmentError>`. Pure:
//! takes only a structured base + a structured op log + the schema — no file
//! path, no prose string (this IS the AC-6 no-prose guarantee by construction).
//!
//! Ordering (AC-3): ops are folded in `OpLog::ordered()` order — by
//! `accepted_at`, FIFO tie-break on `seq`.
//!
//! Atomicity (AC-2): validate-the-whole-log-then-apply. Every op is validated
//! against the element set as of its position in `ordered()` (base ∪ prior-add
//! ops accumulated left-to-right). If any op is invalid the whole batch is
//! rejected with a typed error and the document is left unchanged — the mutating
//! fold never begins.
//!
//! Retire (KD-4): a `Retire` marks the element omitted from the rendered body;
//! the op stays in the log (the log is truth; render is the projection).

use crate::domain::amendment::document::{ArtifactDocument, ContentElement, RenderedDocument};
use crate::domain::amendment::error::AmendmentError;
use crate::domain::amendment::op::{AddAnchor, AmendmentOp, OpKind, OpLog};
use crate::domain::amendment::schema::ContentElementSchema;
use crate::domain::amendment::validate::validate_op;

/// Apply an ordered op log over a base document, returning the rendered
/// projection.
pub fn apply(
    schema: &ContentElementSchema,
    base: &ArtifactDocument,
    log: &OpLog,
) -> Result<RenderedDocument, AmendmentError> {
    let ordered = log.ordered();

    // ---- Pass 1: validate-all (atomicity, AC-2) ----------------------------
    // Validate each op against the element set as of its position. The element
    // set grows as Add ops are encountered (dynamic element set, AC-3).
    let mut known: Vec<ContentElement> = base.elements.clone();
    for entry in &ordered {
        validate_op(schema, &known, &entry.op)?;
        if entry.op.kind == OpKind::Add {
            // Record the minted element so later ops can target it.
            known.push(ContentElement {
                id: entry.op.target_id.clone(),
                kind: entry.op.new_kind.clone().unwrap_or_default(),
                body: entry.op.body.clone().unwrap_or_default(),
            });
        }
    }

    // ---- Pass 2: apply (the fold) ------------------------------------------
    // All ops validated; the mutating fold may now begin. `retired` tracks IDs
    // omitted from the rendered body (the Retire op itself stays in the log).
    let mut elements: Vec<ContentElement> = base.elements.clone();
    let mut retired: Vec<String> = Vec::new();
    for entry in &ordered {
        apply_one(&mut elements, &mut retired, &entry.op);
    }

    // Project: drop retired elements from the rendered body.
    let rendered: Vec<ContentElement> = elements
        .into_iter()
        .filter(|e| !retired.contains(&e.id))
        .collect();

    Ok(RenderedDocument {
        kind: base.kind.clone(),
        elements: rendered,
    })
}

/// Apply a single op to the working element list + retired set. Validation has
/// already succeeded for the whole batch, so this is infallible.
fn apply_one(elements: &mut Vec<ContentElement>, retired: &mut Vec<String>, op: &AmendmentOp) {
    match op.kind {
        OpKind::Revise => {
            if let Some(el) = elements.iter_mut().find(|e| e.id == op.target_id) {
                if let Some(body) = &op.body {
                    el.body = body.clone();
                }
            }
        }
        OpKind::Retire => {
            if !retired.contains(&op.target_id) {
                retired.push(op.target_id.clone());
            }
        }
        OpKind::Add => {
            let new_el = ContentElement {
                id: op.target_id.clone(),
                kind: op.new_kind.clone().unwrap_or_default(),
                body: op.body.clone().unwrap_or_default(),
            };
            insert_at_anchor(elements, new_el, op.anchor.as_ref());
        }
        OpKind::Reorder => {
            if let Some(pos) = elements.iter().position(|e| e.id == op.target_id) {
                let el = elements.remove(pos);
                insert_at_anchor(elements, el, op.anchor.as_ref());
            }
        }
    }
}

/// Insert `el` into `elements` at the position named by `anchor`. A missing or
/// unresolvable anchor falls back to appending at the end.
fn insert_at_anchor(
    elements: &mut Vec<ContentElement>,
    el: ContentElement,
    anchor: Option<&AddAnchor>,
) {
    match anchor {
        Some(AddAnchor::AtStart) => elements.insert(0, el),
        Some(AddAnchor::After(id)) => match elements.iter().position(|e| &e.id == id) {
            Some(pos) => elements.insert(pos + 1, el),
            None => elements.push(el),
        },
        Some(AddAnchor::Before(id)) => match elements.iter().position(|e| &e.id == id) {
            Some(pos) => elements.insert(pos, el),
            None => elements.push(el),
        },
        Some(AddAnchor::AtEnd) | None => elements.push(el),
    }
}
