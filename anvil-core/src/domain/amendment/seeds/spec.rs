//! The `spec` content-element schema seed (KD-3, appendix "spec").
//!
//! Element kinds + ID convention (matches the real spec convention):
//! - `requirement` (`R<N>`)
//! - `acceptance_criterion` (`AC<N>`)
//! - `scope_boundary` (`scope-<slug>`)
//! - `overview` (`overview`, singleton)
//!
//! Legal ops: `Add`/`Revise`/`Retire`/`Reorder` for the list-like kinds;
//! `overview` is a singleton and accepts `Revise` only.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "spec".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "requirement".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "acceptance_criterion".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "scope_boundary".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "overview".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
        ],
    }
}
