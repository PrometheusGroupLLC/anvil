//! The per-kind content-element schema (KD-3).
//!
//! A `ContentElementSchema` declares, for one artifact kind: the legal element
//! kinds (named, each with its ID convention) and the legal `OpKind`s. The
//! registry is **string-keyed and decoupled from the `ArtifactType` enum** —
//! the 8 valid amendment kind strings are their own namespace (KD-3). Notably
//! `playbook` is NOT an amendment kind (the playbook machine is a lifecycle
//! schema, not a body-element artifact), while `spec` and `plan` ARE (they are
//! document-level types within a track).
//!
//! `schema_for_kind(kind: &str)` resolves a kind string to its registered
//! schema; the `seeds` module owns the per-kind registration.

use crate::domain::amendment::op::OpKind;
use crate::domain::amendment::seeds;
use serde::{Deserialize, Serialize};

/// Declaration of one element kind within an artifact-kind schema.
///
/// `name` is the element-kind name carried on `ContentElement::kind` and on an
/// `Add` op's `new_kind`. `legal_ops` is the subset of `OpKind`s permitted for
/// elements of this kind (e.g. a singleton `overview` accepts `Revise` only).
/// `singleton` marks an element kind that may appear at most once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementKindDef {
    /// The element-kind name (matches `ContentElement::kind`).
    pub name: String,
    /// Legal ops for elements of this kind.
    pub legal_ops: Vec<OpKind>,
    /// True if at most one element of this kind may exist (e.g. `overview`).
    #[serde(default)]
    pub singleton: bool,
}

/// The declared content-element schema for one artifact kind (KD-3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentElementSchema {
    /// The artifact-kind string this schema describes (one of the 8 pinned).
    pub kind: String,
    /// The legal element kinds for this artifact kind.
    pub element_kinds: Vec<ElementKindDef>,
}

impl ContentElementSchema {
    /// Look up the declaration for an element kind by name.
    pub fn element_kind(&self, name: &str) -> Option<&ElementKindDef> {
        self.element_kinds.iter().find(|ek| ek.name == name)
    }
}

/// The 8 valid amendment kind strings (canonical list; KD-3).
///
/// This is the amendment-kind namespace — separate from `ArtifactType`.
pub const AMENDMENT_KINDS: &[&str] = &[
    "proposal",
    "track",
    "spec",
    "plan",
    "milestone",
    "initiative",
    "decision",
    "learning",
];

/// Resolve an amendment kind string to its registered content-element schema.
///
/// Returns `None` for any string outside the 8 pinned kinds. The `seeds` module
/// owns the per-kind registration; only `spec` is fully populated in this slice
/// — the other 7 are registry stubs until their fan-out phases.
pub fn schema_for_kind(kind: &str) -> Option<&'static ContentElementSchema> {
    seeds::schema_for_kind(kind)
}
