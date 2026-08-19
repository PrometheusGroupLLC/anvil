//! The structured base/rendered document model for amendments (KD-1).
//!
//! INVARIANT: `ArtifactDocument` is a **FLAT ordered list** of `ContentElement`s.
//! Nested structure (e.g. plan phases containing tasks) is encoded by a dotted
//! ID naming convention (`phase-N.task-M`) on top-level elements — NOT by
//! parent-child nesting in the element type. There is no `children` field and no
//! recursive element type. The schema layer (KD-3/KD-4) enforces dotted-ID
//! conventions per kind.
//!
//! `apply` does NOT parse markdown. The `base` handed to `apply` is already a
//! structured `ArtifactDocument`; the rendered output is a structured
//! `RenderedDocument` of the same element shape. Markdown is a separate render
//! target and is explicitly OUT of B5a (this is the AC-6 no-prose guarantee by
//! construction).

use serde::{Deserialize, Serialize};

/// A single declared content element of an artifact body.
///
/// `id` is the author-assigned stable label (KD-4): `R5`, `AC3`, `phase-1`,
/// `phase-1.task-2`. It is the verbatim key used by op targets, conflict
/// detection, and reversal.
///
/// `kind` is the element-kind name (e.g. `requirement`, `acceptance_criterion`),
/// which must be one of the kinds declared by the artifact-kind's schema.
///
/// `body` is the element's prose payload, carried verbatim. The core never
/// parses it — it is opaque text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentElement {
    /// Author-assigned stable ID (KD-4) — the targeting/conflict/reversal key.
    pub id: String,
    /// Element-kind name; must be declared in the artifact-kind's schema.
    pub kind: String,
    /// Opaque prose body of this element. Never parsed by the core.
    pub body: String,
}

/// The structured base document handed to `apply`/`render`.
///
/// A FLAT ordered list of `ContentElement`s (see module invariant). The order
/// is the authored document order; ops may reorder via anchors but the struct
/// itself imposes no nesting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDocument {
    /// The artifact-kind string (one of the 8 pinned kinds, KD-3).
    pub kind: String,
    /// Flat, ordered list of content elements.
    pub elements: Vec<ContentElement>,
}

/// The structured projection produced by `apply`/`render` (KD-1).
///
/// Same element shape as `ArtifactDocument`; the rendered output is base +
/// accepted structured ops only — not a re-emitted markdown string and not a
/// parse of any prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedDocument {
    /// The artifact-kind string (carried through from the base).
    pub kind: String,
    /// Flat, ordered list of rendered content elements (retired elements omitted).
    pub elements: Vec<ContentElement>,
}
