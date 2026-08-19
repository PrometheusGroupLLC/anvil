//! Conflict detection (AC-4).
//!
//! `detect_conflicts(log) -> Vec<Conflict>` — a pure fold over the ordered log.
//! Two or more accepted ops targeting the same `target_id` are flagged (KD-4),
//! including a `Retire` + `Revise` pair on the same target. One `Conflict` per
//! contended `target_id`, carrying every contending op_id in `ordered()` order.
//! No prose parsing.

use crate::domain::amendment::op::OpLog;

/// A detected conflict: every op identity targeting one shared element ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The shared element ID the conflicting ops target.
    pub target_id: String,
    /// The op_ids of the conflicting ops, in ordered() order.
    pub op_ids: Vec<String>,
}

/// Detect conflicts in an op log: any `target_id` touched by 2+ ops.
pub fn detect_conflicts(log: &OpLog) -> Vec<Conflict> {
    let ordered = log.ordered();

    // Group op_ids by target_id, preserving first-seen order of targets and
    // ordered() order of op_ids within each target.
    let mut targets: Vec<String> = Vec::new();
    let mut grouped: Vec<Vec<String>> = Vec::new();
    for entry in &ordered {
        match targets.iter().position(|t| t == &entry.op.target_id) {
            Some(idx) => grouped[idx].push(entry.op_id.clone()),
            None => {
                targets.push(entry.op.target_id.clone());
                grouped.push(vec![entry.op_id.clone()]);
            }
        }
    }

    targets
        .into_iter()
        .zip(grouped)
        .filter(|(_, op_ids)| op_ids.len() >= 2)
        .map(|(target_id, op_ids)| Conflict { target_id, op_ids })
        .collect()
}
