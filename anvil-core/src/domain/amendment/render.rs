//! Render — the intended-state projection (AC-6).
//!
//! `render` is a thin wrapper over `apply`: it takes only `(schema, base, log)`
//! — no file path, no prose string, no `*.amendments.md` parameter. There is no
//! prose-parsing code path by construction (the AC-6 no-prose guarantee). The
//! rendered output is base + accepted structured ops only — explicitly not a
//! complete history and not a parse of any pre-existing prose amendments.

use crate::domain::amendment::apply::apply;
use crate::domain::amendment::document::{ArtifactDocument, RenderedDocument};
use crate::domain::amendment::error::AmendmentError;
use crate::domain::amendment::op::OpLog;
use crate::domain::amendment::schema::ContentElementSchema;

/// Render the intended-state projection of base + accepted structured ops.
///
/// Structurally identical to `apply` in B5a — a thin projection wrapper that
/// documents the "base + accepted ops only, no prose" contract.
pub fn render(
    schema: &ContentElementSchema,
    base: &ArtifactDocument,
    log: &OpLog,
) -> Result<RenderedDocument, AmendmentError> {
    apply(schema, base, log)
}
