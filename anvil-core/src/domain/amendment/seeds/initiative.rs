//! The `initiative` content-element schema seed (KD-3, Phase-7 fan-out).
//!
//! Element kinds + ID convention (grounded in real initiative definition.md structure):
//! - `pattern`     (`pattern`, singleton)    — the initiative's behavioural pattern
//! - `expectation` (`expectation-<N>`)       — a named expected outcome
//! - `evidence`    (`expectation-<N>.evidence-<M>`) — a piece of supporting evidence
//!   nested under an expectation via the dotted-ID naming convention on a FLAT
//!   document (KD-4). The core treats the ID as an opaque string — no dot-parsing.
//!
//! Legal ops:
//! - `pattern` is a singleton: accepts `Revise` only.
//! - `expectation` and `evidence` accept `Add`/`Revise`/`Retire`/`Reorder`.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::schema::{ContentElementSchema, ElementKindDef};

pub(super) fn build() -> ContentElementSchema {
    ContentElementSchema {
        kind: "initiative".to_string(),
        element_kinds: vec![
            ElementKindDef {
                name: "pattern".to_string(),
                legal_ops: vec![OpKind::Revise],
                singleton: true,
            },
            ElementKindDef {
                name: "expectation".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
            ElementKindDef {
                name: "evidence".to_string(),
                legal_ops: vec![OpKind::Add, OpKind::Revise, OpKind::Retire, OpKind::Reorder],
                singleton: false,
            },
        ],
    }
}
