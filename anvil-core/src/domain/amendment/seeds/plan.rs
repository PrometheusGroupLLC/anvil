//! The `plan` content-element schema seed (KD-3, Phase 3 fan-out).
//!
//! Element kinds + ID convention (matches real plan.md structure):
//! - `phase`  (`phase-N`)               — a numbered implementation phase
//! - `task`   (`phase-N.task-M`)        — a task nested within a phase; the
//!   dotted-ID convention encodes the parent–child relationship on a FLAT
//!   document (document.rs FLAT invariant + KD-4). The core treats the ID
//!   as an opaque string — no dot-parsing anywhere in the framework.
//!
//! Legal ops: `Add`/`Revise`/`Retire`/`Reorder` for both element kinds.
//! Task ops name their target by the full dotted ID (e.g. `phase-1.task-2`).

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "plan".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "phase".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "task".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
