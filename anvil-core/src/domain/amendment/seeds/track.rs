//! The `track` content-element schema seed (KD-3, Phase-5 fan-out).
//!
//! Element kinds + ID convention (grounded in real track structure):
//! - `overview`      (`overview`, singleton) — one-line purpose summary
//! - `goal`          (`goal-<slug>`) — a named delivery goal for this track
//! - `constraint`    (`constraint-<N>`) — a named constraint or boundary
//! - `milestone_ref` (`mref-<slug>`) — a reference to a contributing milestone
//!
//! Legal ops:
//! - `overview` is a singleton: accepts `Revise` only.
//! - `goal`, `constraint`, and `milestone_ref` accept `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "track".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "overview".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "goal".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "constraint".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "milestone_ref".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
