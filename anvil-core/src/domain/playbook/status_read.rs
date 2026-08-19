//! Per-dir `status.yaml` read — the kit-lifecycle facts a playbook dir carries
//! in its sibling `status.yaml`, distinct from the `machine.yaml` the loader reads.
//!
//! The loader leaves `PlaybookMachine.owner_kit = ""` for every machine (no live
//! `machine.yaml` declares it); the real `owner_kit:` and lifecycle `state:` live
//! in `playbooks/<dir>/status.yaml`. The atlas fold reads this per dir to slot
//! each playbook into its kit → status grouping.
//!
//! Serde shape: PLAIN `#[derive(Deserialize)]` with `#[serde(default)]` on every
//! field and NO `#[serde(deny_unknown_fields)]`. Real `status.yaml` files carry
//! `version`/`kind`/`actors`/`transitions`/`visibility`/`org`/... alongside
//! `owner_kit`/`state`; the deny convention used elsewhere in the schema would
//! parse-fail all present files and collapse every entry to `owner_kit: ""`.

use serde::Deserialize;
use std::path::Path;

/// The kit-lifecycle facts read from a playbook dir's `status.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct PlaybookStatus {
    /// The kit that owns this playbook (`""` when the file omits it).
    #[serde(default)]
    pub owner_kit: String,
    /// The lifecycle state (`None` when the file omits it, e.g.
    /// `proposal_lifecycle`'s status.yaml has `owner_kit` but no `state`).
    #[serde(default)]
    pub state: Option<String>,
}

/// Read `<dir>/status.yaml` into a [`PlaybookStatus`].
///
/// Returns `None` when the dir has no `status.yaml` (or it cannot be read/parsed)
/// — the 3 live dirs with no status.yaml (`kit_generation`,
/// `council_experiment_design`, `council_experiment_tracking`) group under an
/// `(unassigned)` owner at the atlas layer. A present-but-partial file parses via
/// the all-`#[serde(default)]` shape (missing `owner_kit` → `""`, missing `state`
/// → `None`).
pub fn read_playbook_status(dir: &Path) -> Option<PlaybookStatus> {
    let path = dir.join("status.yaml");
    let text = std::fs::read_to_string(path).ok()?;
    serde_yaml::from_str::<PlaybookStatus>(&text).ok()
}
