//! Content-derived playbook version.
//!
//! temper treats "playbook version = experiment unit": the generator's
//! evolve loop and the router's v1-vs-v2 experiments compare versions of a
//! playbook, so every measurement/verdict record must carry WHICH version of a
//! playbook produced it (decision `playbook_success_rubric_model`, Amendment 1
//! addendum — "REAL GAP: playbook_version").
//!
//! The version is a stable CONTENT HASH of the loaded [`PlaybookMachine`], not
//! an author-declared field. Content-hashing means the version changes exactly
//! when the machine definition changes and can never be forgotten — a variant
//! event is visible BY CONSTRUCTION rather than by author diligence.
//!
//! Determinism: the machine is serialized via `serde_json` (struct fields emit
//! in declaration order; the machine's only maps are `BTreeMap`s, which emit in
//! sorted key order) so the SAME machine content always serializes to the SAME
//! bytes and therefore the SAME digest. Reuses the same `sha2::Sha256` hashing
//! primitive as `telemetry_salt`, truncated to a short hex digest.

use crate::domain::playbook::types::PlaybookMachine;
use sha2::{Digest, Sha256};

/// Number of hex characters retained from the SHA-256 digest. 16 hex chars = 64
/// bits — collisions negligible for the playbook cardinalities temper compares,
/// while keeping the stamped token compact. Mirrors `telemetry_salt`.
pub const PLAYBOOK_VERSION_HEX_LEN: usize = 16;

/// Compute the content-derived version of a playbook machine, or `None` when the
/// machine cannot be deterministically serialized (fail-open: the emit sites
/// leave `playbook_version = None` rather than failing the transition).
///
/// `playbook_version = hex(sha256(serialize(machine)))[..16]`.
///
/// Deterministic: identical machine content yields an identical version;
/// different machine content yields a different version.
pub fn machine_content_version(machine: &PlaybookMachine) -> Option<String> {
    let serialized = serde_json::to_string(machine).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    Some(
        hex.get(..PLAYBOOK_VERSION_HEX_LEN)
            .unwrap_or(&hex)
            .to_string(),
    )
}
