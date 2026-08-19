//! Step module for `amendment_schema_validation_spec.feature` (AC-1, AC-2).
//!
//! Builds an `ArtifactDocument` element set from a data table, resolves a kind's
//! schema via `schema_for_kind`, constructs an `AmendmentOp`, calls the pure
//! `validate_op`, and asserts Ok / typed error code.

use anvil_core::domain::amendment::document::{ArtifactDocument, ContentElement};
use anvil_core::domain::amendment::op::{AddAnchor, AmendmentOp, OpKind};
use anvil_core::domain::amendment::schema::schema_for_kind;
use anvil_core::domain::amendment::validate::validate_op;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};

const DOC_KEY: &str = "amv_doc";
const RESULT_KEY: &str = "amv_result";

/// Parse the standard `id | kind | body` element table into a flat list.
pub(crate) fn parse_elements(
    table: &brine_core::parser::DataTable,
) -> Result<Vec<ContentElement>, String> {
    let id_idx = col(table, "id")?;
    let kind_idx = col(table, "kind")?;
    let body_idx = col(table, "body")?;
    let mut elements = Vec::new();
    for row in &table.rows {
        elements.push(ContentElement {
            id: cell(row, id_idx, "id")?,
            kind: cell(row, kind_idx, "kind")?,
            body: cell(row, body_idx, "body")?,
        });
    }
    Ok(elements)
}

fn col(table: &brine_core::parser::DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing column '{}'", name))
}

fn cell(row: &[String], idx: usize, name: &str) -> Result<String, String> {
    row.get(idx)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Row too short for '{}'", name))
}

fn op_kind_from_str(s: &str) -> Result<OpKind, String> {
    match s {
        "add" => Ok(OpKind::Add),
        "revise" => Ok(OpKind::Revise),
        "retire" => Ok(OpKind::Retire),
        "reorder" => Ok(OpKind::Reorder),
        other => Err(format!("Unknown op kind '{}'", other)),
    }
}

pub(crate) fn anchor_from_str(s: &str) -> Result<AddAnchor, String> {
    match s {
        "at_start" => Ok(AddAnchor::AtStart),
        "at_end" => Ok(AddAnchor::AtEnd),
        other => {
            if let Some(rest) = other.strip_prefix("after:") {
                Ok(AddAnchor::After(rest.to_string()))
            } else if let Some(rest) = other.strip_prefix("before:") {
                Ok(AddAnchor::Before(rest.to_string()))
            } else {
                Err(format!("Unknown anchor '{}'", other))
            }
        }
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a spec base document with elements:",
            &[],
            &[(DOC_KEY, "ArtifactDocument")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected a data table")?;
                let elements = parse_elements(table)?;
                let doc = ArtifactDocument {
                    kind: "spec".to_string(),
                    elements,
                };
                Ok(Context::new().with(DOC_KEY, doc))
            },
        ),
        step_def(
            "validate_op is called for a {string} op targeting {string}",
            &[(DOC_KEY, "ArtifactDocument")],
            &[(RESULT_KEY, "Result<(), String>")],
            |ctx, params| {
                let op_kind = op_kind_from_str(params.get_string(0).ok_or("Expected op kind")?)?;
                let target_id = params.get_string(1).ok_or("Expected target id")?.to_string();
                let doc = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("No base doc")?;
                let schema = schema_for_kind(&doc.kind)
                    .ok_or_else(|| format!("No schema for kind '{}'", doc.kind))?;
                let op = AmendmentOp {
                    target_id,
                    kind: op_kind,
                    body: Some("revised body".to_string()),
                    new_kind: None,
                    anchor: None,
                };
                let result: Result<(), String> =
                    validate_op(schema, &doc.elements, &op).map_err(|e| e.code().to_string());
                Ok(Context::new().with(RESULT_KEY, result))
            },
        ),
        step_def(
            "validate_op is called for an {string} op minting {string} of kind {string} anchored {string}",
            &[(DOC_KEY, "ArtifactDocument")],
            &[(RESULT_KEY, "Result<(), String>")],
            |ctx, params| {
                let op_kind = op_kind_from_str(params.get_string(0).ok_or("Expected op kind")?)?;
                let target_id = params.get_string(1).ok_or("Expected minted id")?.to_string();
                let new_kind = params.get_string(2).ok_or("Expected new kind")?.to_string();
                let anchor = anchor_from_str(params.get_string(3).ok_or("Expected anchor")?)?;
                let doc = ctx.get::<ArtifactDocument>(DOC_KEY).ok_or("No base doc")?;
                let schema = schema_for_kind(&doc.kind)
                    .ok_or_else(|| format!("No schema for kind '{}'", doc.kind))?;
                let op = AmendmentOp {
                    target_id,
                    kind: op_kind,
                    body: Some("new body".to_string()),
                    new_kind: Some(new_kind),
                    anchor: Some(anchor),
                };
                let result: Result<(), String> =
                    validate_op(schema, &doc.elements, &op).map_err(|e| e.code().to_string());
                Ok(Context::new().with(RESULT_KEY, result))
            },
        ),
        check_def(
            "validation succeeds",
            &[(RESULT_KEY, "Result<(), String>")],
            |ctx, _params| {
                let result = ctx
                    .get::<Result<(), String>>(RESULT_KEY)
                    .ok_or("No result")?;
                match result {
                    Ok(()) => Ok(()),
                    Err(code) => Err(format!("Expected Ok but got error '{}'", code)),
                }
            },
        ),
        check_def(
            "validation fails with code {string}",
            &[(RESULT_KEY, "Result<(), String>")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected code")?;
                let result = ctx
                    .get::<Result<(), String>>(RESULT_KEY)
                    .ok_or("No result")?;
                match result {
                    Ok(()) => Err(format!("Expected error '{}' but validation succeeded", expected)),
                    Err(code) if code == expected => Ok(()),
                    Err(code) => Err(format!("Expected error '{}' but got '{}'", expected, code)),
                }
            },
        ),
    ]
}
