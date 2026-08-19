//! The `learning` content-element schema seed (KD-3, Phase-9 fan-out).
//!
//! Element kinds + ID convention (grounded in real learning definition.md structure):
//! - `observation` (`observation`, singleton) — the observed phenomenon / what happened
//! - `conclusion`  (`conclusion`, singleton)  — the synthesized insight / so what
//! - `evidence`    (`evidence-<N>`) — a named piece of supporting evidence
//!
//! Legal ops:
//! - `observation` and `conclusion` are singletons: accept `Revise` only.
//! - `evidence` accepts `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "learning".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "observation".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "conclusion".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "evidence".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
