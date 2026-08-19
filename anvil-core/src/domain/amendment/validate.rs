//! Schema validation of a single amendment op (AC-1, AC-2).
//!
//! Pure function: `validate_op(schema, elements, op) -> Result<(), AmendmentError>`.
//! No file/prose input. `elements` is the current element set (base ∪ prior-add
//! ops) at this op's position in the ordered log; each op names its target by an
//! author-assigned stable ID (KD-4, AC-2).
//!
//! Rejections (each a typed error):
//! - `Add` minting an ID already present                → `amendment_duplicate_element_id`
//! - `Add` missing `new_kind`/`body` or naming an
//!   undeclared element kind                            → `amendment_invalid_add`
//! - `Add` whose `new_kind` doesn't allow `Add`         → `amendment_op_not_in_schema`
//! - `Revise`/`Retire`/`Reorder` on an undeclared ID    → `amendment_unknown_element`
//! - op kind not legal for the target element's kind    → `amendment_op_not_in_schema`

use crate::domain::amendment::document::ContentElement;
use crate::domain::amendment::error::AmendmentError;
use crate::domain::amendment::op::{AmendmentOp, OpKind};
use crate::domain::amendment::schema::ContentElementSchema;

/// Validate one op against the schema and the current element set.
pub fn validate_op(
    schema: &ContentElementSchema,
    elements: &[ContentElement],
    op: &AmendmentOp,
) -> Result<(), AmendmentError> {
    let op_kind_str = op_kind_str(&op.kind);
    match op.kind {
        OpKind::Add => validate_add(schema, elements, op, op_kind_str),
        OpKind::Revise | OpKind::Retire | OpKind::Reorder => {
            // The target must exist in the current element set.
            let element = elements
                .iter()
                .find(|e| e.id == op.target_id)
                .ok_or_else(|| AmendmentError::UnknownElement {
                    kind: schema.kind.clone(),
                    target_id: op.target_id.clone(),
                })?;
            // The op kind must be legal for the target element's kind.
            let ek = schema.element_kind(&element.kind);
            let legal = ek
                .map(|ek| ek.legal_ops.contains(&op.kind))
                .unwrap_or(false);
            if !legal {
                return Err(AmendmentError::OpNotInSchema {
                    kind: schema.kind.clone(),
                    target_id: op.target_id.clone(),
                    op_kind: op_kind_str.to_string(),
                });
            }
            Ok(())
        }
    }
}

fn validate_add(
    schema: &ContentElementSchema,
    elements: &[ContentElement],
    op: &AmendmentOp,
    op_kind_str: &str,
) -> Result<(), AmendmentError> {
    // The minted ID must not already exist.
    if elements.iter().any(|e| e.id == op.target_id) {
        return Err(AmendmentError::DuplicateElementId {
            kind: schema.kind.clone(),
            target_id: op.target_id.clone(),
        });
    }
    // Add requires new_kind + body.
    let new_kind = op
        .new_kind
        .as_deref()
        .ok_or_else(|| AmendmentError::InvalidAdd {
            kind: schema.kind.clone(),
            target_id: op.target_id.clone(),
            detail: "missing new_kind".to_string(),
        })?;
    if op.body.is_none() {
        return Err(AmendmentError::InvalidAdd {
            kind: schema.kind.clone(),
            target_id: op.target_id.clone(),
            detail: "missing body".to_string(),
        });
    }
    // new_kind must be a declared element kind.
    let ek = schema
        .element_kind(new_kind)
        .ok_or_else(|| AmendmentError::InvalidAdd {
            kind: schema.kind.clone(),
            target_id: op.target_id.clone(),
            detail: format!("undeclared element kind '{}'", new_kind),
        })?;
    // That element kind must permit Add.
    if !ek.legal_ops.contains(&OpKind::Add) {
        return Err(AmendmentError::OpNotInSchema {
            kind: schema.kind.clone(),
            target_id: op.target_id.clone(),
            op_kind: op_kind_str.to_string(),
        });
    }
    Ok(())
}

fn op_kind_str(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Add => "add",
        OpKind::Revise => "revise",
        OpKind::Retire => "retire",
        OpKind::Reorder => "reorder",
    }
}
