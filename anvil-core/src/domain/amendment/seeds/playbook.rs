//! The `playbook` content-element schema seed.
//!
//! Playbook authoring amendments record append-only decisions and changes to
//! generated lifecycle artifacts.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "playbook".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "authoring_decision".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "artifact_change".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "validation_note".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
