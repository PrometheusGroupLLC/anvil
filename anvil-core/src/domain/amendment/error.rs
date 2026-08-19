//! Named error codes for amendment schema validation + application (AC-1).
//!
//! Each variant carries a stable snake_case code via `code()`, mirroring
//! `PlaybookLoadError`. `Display` embeds the code as a substring so step
//! assertions can match on it.

use std::fmt;

/// An error returned when validating or applying an amendment op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmendmentError {
    /// The op's `OpKind` is not legal for the targeted element kind (or for any
    /// element kind in the schema, for an `Add`).
    ///
    /// Code: `amendment_op_not_in_schema`
    OpNotInSchema {
        kind: String,
        target_id: String,
        op_kind: String,
    },

    /// The op targets an element ID not declared in the current element set
    /// (base ∪ prior-add-ops).
    ///
    /// Code: `amendment_unknown_element`
    UnknownElement { kind: String, target_id: String },

    /// An `Add` op mints an element ID that already exists in the current set.
    ///
    /// Code: `amendment_duplicate_element_id`
    DuplicateElementId { kind: String, target_id: String },

    /// An `Add` op is missing a required field (`new_kind` and/or `body`), or
    /// the named `new_kind` is not declared in the schema.
    ///
    /// Code: `amendment_invalid_add`
    InvalidAdd {
        kind: String,
        target_id: String,
        detail: String,
    },
}

impl AmendmentError {
    /// The stable snake_case error code for this variant.
    pub fn code(&self) -> &'static str {
        match self {
            AmendmentError::OpNotInSchema { .. } => "amendment_op_not_in_schema",
            AmendmentError::UnknownElement { .. } => "amendment_unknown_element",
            AmendmentError::DuplicateElementId { .. } => "amendment_duplicate_element_id",
            AmendmentError::InvalidAdd { .. } => "amendment_invalid_add",
        }
    }
}

impl fmt::Display for AmendmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AmendmentError::OpNotInSchema {
                kind,
                target_id,
                op_kind,
            } => write!(
                f,
                "amendment_op_not_in_schema: kind '{}' op '{}' on target '{}' is not in the schema",
                kind, op_kind, target_id
            ),
            AmendmentError::UnknownElement { kind, target_id } => write!(
                f,
                "amendment_unknown_element: kind '{}' op targets undeclared element '{}'",
                kind, target_id
            ),
            AmendmentError::DuplicateElementId { kind, target_id } => write!(
                f,
                "amendment_duplicate_element_id: kind '{}' add mints existing element id '{}'",
                kind, target_id
            ),
            AmendmentError::InvalidAdd {
                kind,
                target_id,
                detail,
            } => write!(
                f,
                "amendment_invalid_add: kind '{}' add of '{}' is invalid: {}",
                kind, target_id, detail
            ),
        }
    }
}

impl std::error::Error for AmendmentError {}
