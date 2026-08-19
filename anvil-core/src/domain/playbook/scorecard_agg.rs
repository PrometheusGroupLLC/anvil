//! Scorecard aggregation — folds temper's on-disk per-instance quality
//! scorecards into a per-kind measurement summary for the Atlas.
//!
//! Temper persists one `WorkflowScorecard` per instance at
//! `<temper_home>/.temper/workflow-measurements/<instance_id>/scorecard.json`
//! (see `temper-core::domain::step_measurement::WorkflowScorecard`). Each
//! scorecard's `track_id` carries the playbook KIND (e.g. `"track"`), and each
//! of its `steps` carries a `[0,1]` `quality_score` — an LLM-judge coherence
//! grade for that (from_state, to_state, role) transition.
//!
//! This module folds an already-read, already-parsed vector of scorecards
//! (paired with their instance id, since a scorecard's own `workflow_id` field
//! is not load-bearing for identity here — the caller supplies the on-disk
//! directory name) into:
//!   - a per-(to_state, role) MEAN quality across every matching instance,
//!     with a sample count (deduped: a step reachable via more than one edge
//!     folds to ONE row, not one per from_state — see `StepQualitySummary`);
//!   - the kind's overall mean_quality (the mean of each matching instance's
//!     own `mean_quality`);
//!   - a flat "recent measurements" feed, capped at a caller-supplied limit.
//!
//! Pure — no filesystem, no clock. The caller (anvil-engine) globs the temper
//! home, parses each `scorecard.json`, orders instances MOST-RECENT-FIRST
//! (by file mtime — scorecards carry no internal timestamp), and hands the
//! ordered vector here. "Recent" is therefore defined entirely by INPUT ORDER:
//! this fold flattens each instance's `steps` in the order given, instance by
//! instance, and truncates at `recent_limit`. An empty input, or a kind with no
//! matching instances, folds to an empty (not an error) summary.

/// One step (transition) scored within a single instance's scorecard.
#[derive(Debug, Clone, PartialEq)]
pub struct ScorecardStep {
    pub from_state: String,
    pub to_state: String,
    pub role: String,
    pub actor: String,
    /// `[0,1]` coherence quality (temper's judge grade for this step).
    pub quality_score: f64,
    pub model: String,
}

/// One instance's full scorecard, as persisted by temper.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scorecard {
    /// The playbook KIND this instance ran (temper's `track_id`).
    pub track_id: String,
    pub step_count: u64,
    /// Arithmetic mean of `steps[].quality_score` for this instance.
    pub mean_quality: f64,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
    pub steps: Vec<ScorecardStep>,
}

/// One on-disk scorecard paired with its instance id (the
/// `workflow-measurements/<instance_id>/` directory name).
#[derive(Debug, Clone, PartialEq)]
pub struct ScorecardInstance {
    pub instance_id: String,
    pub scorecard: Scorecard,
}

/// The folded MEAN quality for one declared (to_state, role) step, across
/// every matching instance of the kind.
///
/// `(to_state, role)` — NOT `(from_state, to_state, role)` — is the identity
/// here: a machine's `measurement_by_role` is declared PER STATE (the state a
/// step transitions INTO), not per edge, and the artifact-quality grader's
/// schema (below) carries no `from_state` at all. Folding by the wider
/// `(from_state, to_state, role)` key used to produce DUPLICATE rows for a
/// step reachable via more than one edge (e.g. a review gate re-entered from
/// both its revision loop and a skip edge) — the Atlas rendered the same step
/// 2-3 times. `from_state` is retained below as PROVENANCE only (the first
/// edge observed for this key), never as part of identity.
#[derive(Debug, Clone, PartialEq)]
pub struct StepQualitySummary {
    /// Provenance only (first-observed edge into this step) — NOT part of
    /// this row's identity. See the struct doc.
    pub from_state: String,
    pub to_state: String,
    pub role: String,
    /// `[0,1]` mean of `quality_score` across `sample_count` observations —
    /// temper's coherence-judge grade for this transition. Kept for provenance,
    /// but NOT the meaningful score once artifact-quality is available (below).
    pub mean_quality: f64,
    pub sample_count: u32,
    /// `[0,10]` HARSH anchored mean artifact-quality score for this
    /// (to_state, role) — temper's separate `artifact_quality.json` grader,
    /// scoring the ACTUAL ARTIFACT the step produced (not generic coherence).
    /// `None` when no artifact-quality measurement exists yet for this step —
    /// NOT a fabricated zero. This is the score the Atlas should PREFER.
    pub artifact_quality: Option<f64>,
    /// Count of artifact-quality observations folded into `artifact_quality`.
    /// `0` when `artifact_quality` is `None`.
    pub artifact_sample_count: u32,
}

/// One flattened entry in the recent-measurements feed.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentMeasurement {
    pub instance_id: String,
    pub to_state: String,
    pub role: String,
    pub actor: String,
    pub quality_score: f64,
    pub model: String,
}

/// The full per-kind scorecard measurement summary.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScorecardMeasurementSummary {
    pub kind: String,
    /// Count of instances whose `track_id` matched `kind`.
    pub instance_count: u32,
    /// Mean of each matching instance's own `mean_quality`. `0.0` when
    /// `instance_count` is 0 (no instances to average — not a real zero score).
    pub overall_mean_quality: f64,
    /// Per-step mean quality, ordered ascending by (to_state, role) — one row
    /// per step, deduped across from_state (see `StepQualitySummary`).
    pub steps: Vec<StepQualitySummary>,
    /// The `recent_limit`-capped recent feed, in the order the caller supplied
    /// instances (most-recent-first by convention).
    pub recent: Vec<RecentMeasurement>,
}

impl ScorecardMeasurementSummary {
    /// The summary row for a (from_state, to_state, role) step, if present.
    pub fn step(&self, from_state: &str, to_state: &str, role: &str) -> Option<&StepQualitySummary> {
        self.steps
            .iter()
            .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
    }
}

/// Fold a (caller-ordered) vector of scorecard instances into the per-kind
/// measurement summary.
///
/// `instances` is filtered to those whose `scorecard.track_id == kind`
/// (non-matching instances are excluded from every part of the result).
/// `recent_limit` caps the flattened recent feed; `0` yields an empty feed.
pub fn fold_scorecard_measurements(
    instances: &[ScorecardInstance],
    kind: &str,
    recent_limit: usize,
) -> ScorecardMeasurementSummary {
    use std::collections::BTreeMap;

    let matching: Vec<&ScorecardInstance> = instances
        .iter()
        .filter(|i| i.scorecard.track_id == kind)
        .collect();

    let instance_count = matching.len() as u32;

    let overall_mean_quality = if matching.is_empty() {
        0.0
    } else {
        matching.iter().map(|i| i.scorecard.mean_quality).sum::<f64>() / matching.len() as f64
    };

    // (to_state, role) -> (sum of quality_score, count, first-observed from_state)
    // Keyed WITHOUT from_state (see the StepQualitySummary doc) so a step
    // reachable via more than one edge folds to ONE row, not one per edge.
    let mut sums: BTreeMap<(String, String), (f64, u32, String)> = BTreeMap::new();
    for instance in &matching {
        for step in &instance.scorecard.steps {
            let entry = sums
                .entry((step.to_state.clone(), step.role.clone()))
                .or_insert_with(|| (0.0, 0, step.from_state.clone()));
            entry.0 += step.quality_score;
            entry.1 += 1;
        }
    }

    let steps: Vec<StepQualitySummary> = sums
        .into_iter()
        .map(|((to_state, role), (sum, count, from_state))| StepQualitySummary {
            from_state,
            to_state,
            role,
            mean_quality: if count > 0 { sum / count as f64 } else { 0.0 },
            sample_count: count,
            // Artifact-quality is folded separately (`fold_artifact_quality_into_steps`)
            // — absent here by construction.
            artifact_quality: None,
            artifact_sample_count: 0,
        })
        .collect();

    let mut recent: Vec<RecentMeasurement> = Vec::new();
    'outer: for instance in &matching {
        for step in &instance.scorecard.steps {
            if recent.len() >= recent_limit {
                break 'outer;
            }
            recent.push(RecentMeasurement {
                instance_id: instance.instance_id.clone(),
                to_state: step.to_state.clone(),
                role: step.role.clone(),
                actor: step.actor.clone(),
                quality_score: step.quality_score,
                model: step.model.clone(),
            });
        }
    }

    ScorecardMeasurementSummary {
        kind: kind.to_string(),
        instance_count,
        overall_mean_quality,
        steps,
        recent,
    }
}

// ── Artifact quality (the REAL, harsh-anchored score) ───────────────────────
//
// A separate grader writes one `artifact_quality.json` per instance at
// `<temper_home>/.temper/workflow-measurements/<instance_id>/artifact_quality.json`
// — a sibling of `scorecard.json` in the SAME instance directory. Unlike the
// coherence `quality_score` above (a generic §0-template grade, uniformly
// low), this is a HARSH 0-10 anchored score of the ACTUAL ARTIFACT the step
// produced (e.g. `spec.md`). It carries no `from_state` — only the step it
// produced (`to_state`) and the `role` that produced it.

/// One step's artifact-quality score within a single instance's
/// `artifact_quality.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactQualityStep {
    pub to_state: String,
    pub role: String,
    /// The artifact filename graded (e.g. `"spec.md"`).
    pub artifact: String,
    /// `[0,10]` HARSH anchored score.
    pub artifact_quality_0_10: f64,
    /// The band label the grader assigned (e.g. `"mediocre"`), provenance only.
    pub band: String,
}

/// One instance's full artifact-quality record, as persisted by the grader.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArtifactQualityInstance {
    pub workflow_id: String,
    /// The playbook KIND this instance ran (matches `Scorecard::track_id`).
    pub track_id: String,
    pub steps: Vec<ArtifactQualityStep>,
}

/// Fold `artifact_quality_instances` (already read + parsed by the caller,
/// filtered to none in particular — filtering by `kind` happens here) into
/// `steps`, ATTACHING the `[0,10]` mean artifact-quality score to every
/// existing `StepQualitySummary` row whose `(to_state, role)` matches. A
/// `(to_state, role)` pair with artifact-quality data but NO matching
/// coherence row (e.g. a kind with no `scorecard.json` steps at all, or a
/// step the coherence grader never covered) gets a NEW row appended with an
/// empty `from_state` (the artifact-quality schema carries no `from_state`)
/// and `mean_quality: 0.0, sample_count: 0` (honest: no coherence measurement,
/// not a fabricated zero — callers must gate on `sample_count` same as always).
///
/// Pure — no filesystem, no clock. `instances` need not be pre-filtered by
/// kind; non-matching instances are excluded here, mirroring
/// `fold_scorecard_measurements`.
pub fn fold_artifact_quality_into_steps(
    mut steps: Vec<StepQualitySummary>,
    instances: &[ArtifactQualityInstance],
    kind: &str,
) -> Vec<StepQualitySummary> {
    use std::collections::BTreeMap;

    let mut sums: BTreeMap<(String, String), (f64, u32)> = BTreeMap::new();
    for instance in instances.iter().filter(|i| i.track_id == kind) {
        for step in &instance.steps {
            let entry = sums
                .entry((step.to_state.clone(), step.role.clone()))
                .or_insert((0.0, 0));
            entry.0 += step.artifact_quality_0_10;
            entry.1 += 1;
        }
    }

    for ((to_state, role), (sum, count)) in sums {
        if count == 0 {
            continue;
        }
        let mean = sum / count as f64;
        let matched = steps
            .iter_mut()
            .filter(|s| s.to_state == to_state && s.role == role)
            .count();
        if matched > 0 {
            for s in steps
                .iter_mut()
                .filter(|s| s.to_state == to_state && s.role == role)
            {
                s.artifact_quality = Some(mean);
                s.artifact_sample_count = count;
            }
        } else {
            steps.push(StepQualitySummary {
                from_state: String::new(),
                to_state,
                role,
                mean_quality: 0.0,
                sample_count: 0,
                artifact_quality: Some(mean),
                artifact_sample_count: count,
            });
        }
    }

    steps
}

/// The overall ARTIFACT-QUALITY summary for a kind — the HARSH 0-10 anchored
/// mean across every already-folded `step_quality` row that carries an
/// artifact-quality measurement, weighted by each row's
/// `artifact_sample_count`. THIS is the number the Atlas should show as its
/// headline/overall score — NOT the coherence-based `overall_mean_quality`
/// (uniformly low, grades a generic template rather than the real artifact).
///
/// Returns `(mean, sample_count)`. `(0.0, 0)` when no row carries an
/// artifact-quality measurement yet — an honest "not yet graded", never a
/// fabricated score. Pure — operates on already-folded steps (call AFTER
/// `fold_artifact_quality_into_steps`), no filesystem, no clock.
pub fn fold_overall_artifact_quality(steps: &[StepQualitySummary]) -> (f64, u32) {
    let mut sum = 0.0;
    let mut count: u32 = 0;
    for s in steps {
        if let Some(q) = s.artifact_quality {
            sum += q * s.artifact_sample_count as f64;
            count += s.artifact_sample_count;
        }
    }
    if count == 0 {
        (0.0, 0)
    } else {
        (sum / count as f64, count)
    }
}
