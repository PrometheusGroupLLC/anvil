//! The `decision` content-element schema seed (KD-3, Phase-8 fan-out).
//!
//! Element kinds + ID convention (grounded in real decision definition.md structure):
//! - `question`           (`question`, singleton) — the decision question
//! - `answer`             (`answer`, singleton)   — the resolution / answer
//! - `rationale`          (`rationale`, singleton) — the rationale for the answer
//! - `validity_condition` (`vc-<N>`) — a condition under which the decision holds
//!
//! Legal ops:
//! - `question`, `answer`, and `rationale` are singletons: accept `Revise` only.
//! - `validity_condition` accepts `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "decision".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "question".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "answer".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "rationale".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "validity_condition".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
