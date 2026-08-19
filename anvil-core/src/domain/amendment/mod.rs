//! Structured-amendment-diffs core (B5a) — pure `anvil-core`.
//!
//! Forge amendments to frozen artifacts as **structured semantic diffs**: a
//! per-artifact-kind content-element schema + named ops over declared elements
//! that compose into a rendered projection (base document + accepted ops =
//! current state).
//!
//! Pillars (all pure, no I/O, no prose parsing):
//! - [`document`] — the structured base/rendered document model (KD-1). FLAT
//!   ordered list of [`ContentElement`]s; nesting is dotted-ID convention only.
//! - [`op`] — [`AmendmentOp`], [`OpKind`], [`AddAnchor`], [`OpLogEntry`],
//!   [`OpLog`] (KD-2/KD-4). `OpLog::push` assigns `seq`; `ordered()` is stable
//!   across serde round-trips.
//! - [`schema`] — [`ContentElementSchema`] + [`schema_for_kind`] (KD-3),
//!   string-keyed over the 8 pinned amendment kinds, decoupled from
//!   `ArtifactType`.
//! - [`validate`] — typed-error schema validation of a single op (AC-1, AC-2).
//! - [`apply`] — the pure fold base + ordered op log → rendered projection
//!   (AC-3); validate-all-then-apply atomicity (AC-2).
//! - [`render`] — the intended-state projection wrapper over `apply` (AC-6).
//! - [`conflict`] — `detect_conflicts` over `target_id` (AC-4).
//! - [`reverse`] — `reverse` by `op_id` (AC-5).
//!
//! No-prose guarantee (AC-6): [`apply::apply`] and [`render::render`] take only
//! `(schema, base: ArtifactDocument, log: OpLog)` — no file path, no prose
//! string parameter. There is no prose-parsing code path by construction.
//!
//! Scope: all 8 amendment kinds are fully populated (B5a Phase-11 complete).
//! No proto/engine/MCP changes (that is the successor track B5b).
//!
//! ## Stability freeze (B5a Phase-11 — post-Phase-9)
//!
//! The core `apply`/`render` API and the per-kind schemas are **frozen** after
//! Phase-9 fan-out completion. Within this track, no breaking changes to:
//! - [`apply::apply`] and [`render::render`] signatures:
//!   `(schema: &ContentElementSchema, base: &ArtifactDocument, log: &OpLog)`
//!   — no file path, no prose string, no `*.amendments.md` parameter.
//!   **This is the no-prose guarantee for AC-6**: there is no prose-parsing code
//!   path by construction; the type signature makes it impossible.
//! - [`schema_for_kind`] returning `Option<&'static ContentElementSchema>` for
//!   the 8 pinned kind strings (KD-3: `proposal`, `track`, `spec`, `plan`,
//!   `milestone`, `initiative`, `decision`, `learning`).
//! - The 8 per-kind `ContentElementSchema` element vocabularies as declared in
//!   `seeds/`.
//!
//! This freeze is the downstream-readiness / B5b-consumability guarantee: B5b can
//! call `apply(schema_for_kind(kind).unwrap(), &base, &log)` and receive a
//! `RenderedDocument` without any further changes to this module.
//!
//! The freeze guard is enforced by:
//! 1. `amendment_all_kinds_schema_present.feature` — proves all 8 kinds resolve
//!    a non-empty schema (no kind left as a stub).
//! 2. This freeze marker (review + discipline guarantee).
//! 3. `cargo test --workspace` green (additive-only; no existing tests regress).

pub mod apply;
pub mod conflict;
pub mod document;
pub mod error;
pub mod op;
pub mod render;
pub mod reverse;
pub mod schema;
pub mod seeds;
pub mod validate;

pub use apply::apply;
pub use conflict::{detect_conflicts, Conflict};
pub use document::{ArtifactDocument, ContentElement, RenderedDocument};
pub use error::AmendmentError;
pub use op::{AddAnchor, AmendmentOp, OpKind, OpLog, OpLogEntry};
pub use render::render;
pub use reverse::reverse;
pub use schema::{schema_for_kind, ContentElementSchema, ElementKindDef, AMENDMENT_KINDS};
pub use validate::validate_op;
