//! Compiled-in per-kind `ContentElementSchema` seeds (KD-3).
//!
//! The registry is string-keyed over the 8 pinned amendment kinds (separate
//! from `ArtifactType`). In this foundation slice (B5a Phase 0–2) only `spec` is
//! fully populated; the other 7 kinds are registry stubs (an empty-element
//! schema) until their fan-out phases. The accessor + registry exist for all 8
//! so callers can resolve any pinned kind string.
//!
//! Seeds are `&'static ContentElementSchema` values lazily initialized by
//! `OnceLock` (no new deps — same convention as `playbook::seeds`).

mod decision;
mod initiative;
mod learning;
mod milestone;
mod plan;
mod playbook;
mod proposal;
mod spec;
mod track;

use crate::domain::amendment::schema::ContentElementSchema;
use std::sync::OnceLock;

static SPEC_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static PLAN_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static PROPOSAL_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static TRACK_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static MILESTONE_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static INITIATIVE_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static DECISION_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static LEARNING_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();
static PLAYBOOK_SCHEMA: OnceLock<ContentElementSchema> = OnceLock::new();

/// Resolve an amendment kind string to its registered schema.
///
/// Returns `None` for any string outside the 8 pinned kinds.
pub fn schema_for_kind(kind: &str) -> Option<&'static ContentElementSchema> {
    match kind {
        "spec" => Some(SPEC_SCHEMA.get_or_init(spec::build)),
        "plan" => Some(PLAN_SCHEMA.get_or_init(plan::build)),
        "proposal" => Some(PROPOSAL_SCHEMA.get_or_init(proposal::build)),
        "track" => Some(TRACK_SCHEMA.get_or_init(track::build)),
        "milestone" => Some(MILESTONE_SCHEMA.get_or_init(milestone::build)),
        "initiative" => Some(INITIATIVE_SCHEMA.get_or_init(initiative::build)),
        "decision" => Some(DECISION_SCHEMA.get_or_init(decision::build)),
        "learning" => Some(LEARNING_SCHEMA.get_or_init(learning::build)),
        // Canonical only. `request.kind` here is a CLIENT-SUPPLIED amendment
        // kind, not a kind read off disk — `canonical_playbook_language_mcp`
        // requires the real stdio server to REFUSE `kind: workflow`, and an
        // arm here is the whole reason it would be accepted. A legacy artifact
        // is amended by naming the canonical kind; its persisted `status.yaml`
        // kind is normalised on read (catalog_artifact_kind.feature).
        "playbook" => Some(PLAYBOOK_SCHEMA.get_or_init(playbook::build)),
        _ => None,
    }
}
