//! Step 2×2 fold — the blind-gate detector (Q3 attribution).
//!
//! Decision `playbook_success_rubric_model` Amendment 1 item 8 names the
//! diagnostic pair for a step-kind: **one-shot gate pass-rate × defect-escape
//! rate** (high/high = a BLIND gate — it waves work through AND lets defects
//! escape to be caught downstream). This fold computes both axes deterministically
//! over the EXISTING sinks, per `(playbook_kind, step_kind)`, each cell carrying
//! its own N (item 10, N-honesty):
//!
//!   - **One-shot gate pass-rate** — from the universal activity log. A
//!     `step_kind` is the phase before a `*_review` gate (e.g. `spec_review` →
//!     `spec`). For each instance that reached a step's review gate (a transition
//!     FROM `{step_kind}_review`), the gate passed ONE-SHOT iff the instance never
//!     bounced back into `{step_kind}_revision`. Rate = one-shot passes / gate
//!     observations.
//!   - **Defect-escape rate** — from the review-verdict sink. A finding carries an
//!     `origin_phase` attributed by the FINDER. When a finding attributed to step
//!     `P` is recorded at a gate whose phase differs from `P`, that defect ESCAPED
//!     its origin gate (phase-containment/escape, manufacturing QA). Rate = escapes
//!     / findings attributed to the step.
//!
//! Pure functions over already-read record vectors — no filesystem, no ports, no
//! registry (both axes are decided purely from state-name suffixes + the finder's
//! attribution). Empty inputs fold to an empty result.

use crate::ports::activity_log_port::ActivityLogRecord;
use crate::ports::review_verdict_port::ReviewVerdictRecord;
use std::collections::{BTreeMap, BTreeSet};

/// Suffix marking a review-gate state (`spec_review`, `impl_review`, ...).
const REVIEW_SUFFIX: &str = "_review";
/// Suffix marking a revision state (`spec_revision`, `impl_revision`, ...).
use crate::domain::shared_types::revision_step_kind;

/// One `(playbook_kind, step_kind)` cell of the step 2×2, carrying both axes and
/// each axis's own N. A cell appears when EITHER axis observed data for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepCell {
    /// The playbook kind (workflow_kind), e.g. `"track"`.
    pub kind: String,
    /// The step phase, e.g. `"spec"` / `"plan"` / `"impl"` (a `*_review` gate's
    /// prefix, and the `origin_phase` a finding attributes to).
    pub step_kind: String,
    /// N for the pass-rate axis: instances that reached this step's review gate.
    pub gate_observations: u64,
    /// Of `gate_observations`, those that passed one-shot (never bounced into
    /// `{step_kind}_revision`).
    pub one_shot_passes: u64,
    /// N for the escape axis: findings attributed (by `origin_phase`) to this step.
    pub escape_observations: u64,
    /// Of `escape_observations`, those caught at a gate whose phase differs from
    /// the finding's origin phase (a defect that escaped its origin gate).
    pub defect_escapes: u64,
}

impl StepCell {
    /// One-shot gate pass-rate (`one_shot_passes / gate_observations`; 0.0 when N=0).
    pub fn one_shot_pass_rate(&self) -> f64 {
        if self.gate_observations > 0 {
            self.one_shot_passes as f64 / self.gate_observations as f64
        } else {
            0.0
        }
    }

    /// Defect-escape rate (`defect_escapes / escape_observations`; 0.0 when N=0).
    pub fn defect_escape_rate(&self) -> f64 {
        if self.escape_observations > 0 {
            self.defect_escapes as f64 / self.escape_observations as f64
        } else {
            0.0
        }
    }
}

/// The full step 2×2 fold result: cells ordered by `(kind, step_kind)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepTwoByTwoResult {
    pub cells: Vec<StepCell>,
}

impl StepTwoByTwoResult {
    /// The cell for a `(kind, step_kind)` pair, if present.
    pub fn cell(&self, kind: &str, step_kind: &str) -> Option<&StepCell> {
        self.cells
            .iter()
            .find(|c| c.kind == kind && c.step_kind == step_kind)
    }
}

/// A single per-instance transition: a record with a non-empty `to_state`.
struct Transition {
    from_state: String,
    to_state: String,
}

/// Fold the activity log + review-verdict streams into the step 2×2.
///
/// The pass-rate axis reads the activity log (records lacking a
/// `playbook_run_id` are skipped from the per-instance measure); the escape
/// axis reads the review-verdict findings. The two axes are accumulated into the
/// same `(kind, step_kind)` cell map; a cell appears when either axis saw data.
pub fn fold_step_two_by_two(
    activity: &[ActivityLogRecord],
    verdicts: &[ReviewVerdictRecord],
) -> StepTwoByTwoResult {
    // Per-cell accumulators keyed by (kind, step_kind).
    let mut gate_observations: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut one_shot_passes: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut escape_observations: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut defect_escapes: BTreeMap<(String, String), u64> = BTreeMap::new();

    // ===== Pass-rate axis (activity log) =====
    // Group transitions by instance, carrying each instance's resolved kind.
    let mut transitions_by_instance: BTreeMap<String, Vec<Transition>> = BTreeMap::new();
    let mut kind_by_instance: BTreeMap<String, String> = BTreeMap::new();
    for record in activity {
        let Some(instance) = record.playbook_run_id.as_ref() else {
            continue;
        };
        if instance.is_empty() {
            continue;
        }
        if !record.artifact_kind.is_empty() {
            kind_by_instance
                .entry(instance.clone())
                .or_insert_with(|| record.artifact_kind.clone());
        }
        if record.to_state.is_empty() {
            continue;
        }
        transitions_by_instance
            .entry(instance.clone())
            .or_default()
            .push(Transition {
                from_state: record.from_state.clone(),
                to_state: record.to_state.clone(),
            });
    }

    for (instance, transitions) in &transitions_by_instance {
        let kind = kind_by_instance.get(instance).cloned().unwrap_or_default();
        // Step-kinds whose review gate this instance exited (a decision made),
        // and step-kinds it bounced back into revision for.
        let mut gate_step_kinds: BTreeSet<String> = BTreeSet::new();
        let mut bounced_step_kinds: BTreeSet<String> = BTreeSet::new();
        for t in transitions {
            if let Some(step) = t.from_state.strip_suffix(REVIEW_SUFFIX) {
                gate_step_kinds.insert(step.to_string());
            }
            if let Some(step) = revision_step_kind(&t.to_state) {
                bounced_step_kinds.insert(step.to_string());
            }
        }
        for step in gate_step_kinds {
            let key = (kind.clone(), step.clone());
            *gate_observations.entry(key.clone()).or_insert(0) += 1;
            // One-shot iff the instance never bounced into this step's revision.
            if !bounced_step_kinds.contains(&step) {
                *one_shot_passes.entry(key).or_insert(0) += 1;
            }
        }
    }

    // ===== Escape axis (review verdicts) =====
    for verdict in verdicts {
        // The phase of the gate that recorded the verdict (strip `_review`;
        // gates that are not `*_review` fall back to the raw gate label).
        let gate_phase = verdict
            .gate_state
            .strip_suffix(REVIEW_SUFFIX)
            .unwrap_or(verdict.gate_state.as_str());
        for finding in &verdict.findings {
            let Some(origin) = finding.origin_phase.as_ref() else {
                continue;
            };
            let key = (verdict.artifact_kind.clone(), origin.clone());
            *escape_observations.entry(key.clone()).or_insert(0) += 1;
            if origin.as_str() != gate_phase {
                *defect_escapes.entry(key).or_insert(0) += 1;
            }
        }
    }

    // ===== Merge the two axes into ordered cells =====
    let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
    keys.extend(gate_observations.keys().cloned());
    keys.extend(escape_observations.keys().cloned());

    let cells = keys
        .into_iter()
        .map(|key| StepCell {
            kind: key.0.clone(),
            step_kind: key.1.clone(),
            gate_observations: gate_observations.get(&key).copied().unwrap_or(0),
            one_shot_passes: one_shot_passes.get(&key).copied().unwrap_or(0),
            escape_observations: escape_observations.get(&key).copied().unwrap_or(0),
            defect_escapes: defect_escapes.get(&key).copied().unwrap_or(0),
        })
        .collect();

    StepTwoByTwoResult { cells }
}
