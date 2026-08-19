//! The `milestone` content-element schema seed (KD-3, Phase-6 fan-out).
//!
//! Element kinds + ID convention (grounded in real milestone.md structure):
//! - `outcome`           (`outcome`, singleton) — the milestone's primary outcome
//! - `success_criterion` (`SC<N>`) — a named success criterion
//! - `scope`             (`scope-<slug>`) — a named scope item (in or out of scope)
//!
//! Legal ops:
//! - `outcome` is a singleton: accepts `Revise` only.
//! - `success_criterion` and `scope` accept `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "milestone".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "outcome".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "success_criterion".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "scope".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
