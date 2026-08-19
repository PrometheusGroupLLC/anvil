//! The `proposal` content-element schema seed (KD-3, Phase-4 fan-out).
//!
//! Element kinds + ID convention (grounded in real proposal.md structure):
//! - `problem`   (`problem`, singleton) — the motivating problem statement
//! - `approach`  (`approach`, singleton) — the proposed solution approach
//! - `slice`     (`slice-<N>`) — an incremental delivery slice
//! - `risk`      (`risk-<slug>`) — an identified risk
//!
//! Legal ops:
//! - `problem` and `approach` are singletons: accept `Revise` only.
//! - `slice` and `risk` accept `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "proposal".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "problem".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "approach".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "slice".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "risk".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
