//! Reads temper's on-disk per-instance quality scorecards and folds them into
//! the Atlas per-kind measurement summary.
//!
//! Temper persists one `PlaybookScorecard` per instance at
//! `<temper_home>/.temper/workflow-measurements/<instance_id>/scorecard.json`
//! (mirrors the §0 stream's `<temper_home>/.temper/step-measurements/` root —
//! see `resolve_temper_home` + `FileSystemStep0StreamAdapter`). This module is
//! the READ side: it globs that directory, parses each `scorecard.json`
//! (tolerating unknown/extra fields — temper's real payload also carries
//! `playbook_id`, `cost_usd`, `tokens`, `duration_ms`, none of which this
//! Atlas view needs), and orders instances MOST-RECENT-FIRST by the
//! `scorecard.json` file's mtime (scorecards carry no internal timestamp).
//!
//! Fail-open throughout: a missing `workflow-measurements` directory, an
//! unreadable entry, or a malformed `scorecard.json` is skipped (logged at
//! `warn`), never surfaced as an error — this is a best-effort read-side
//! enrichment, not a load-bearing query. Works identically whether 0, 1, or
//! 1000 scorecards exist on disk.

use std::path::Path;
use std::time::SystemTime;

use anvil_core::domain::playbook::scorecard_agg::{
    ArtifactQualityInstance, ArtifactQualityStep, Scorecard, ScorecardInstance, ScorecardStep,
};
use serde::Deserialize;

/// The `workflow-measurements` directory under a resolved temper home.
fn playbook_measurements_dir(temper_home: &Path) -> std::path::PathBuf {
    temper_home.join(".temper").join("workflow-measurements")
}

/// Mirrors temper's `StepScorecard` — only the fields this Atlas view needs.
/// `#[serde(default)]` on every field means a scorecard missing a field (or
/// carrying extra ones) still parses rather than erroring the whole read.
#[derive(Debug, Deserialize)]
struct RawStep {
    #[serde(default)]
    from_state: String,
    #[serde(default)]
    to_state: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    actor: String,
    #[serde(default)]
    quality_score: f64,
    #[serde(default)]
    model: String,
}

/// Mirrors temper's `PlaybookScorecard` — only the fields this Atlas view
/// needs. Extra JSON keys (`playbook_id`, per-step `cost_usd`/`tokens`, etc.)
/// are ignored by serde's default (non-`deny_unknown_fields`) behavior.
#[derive(Debug, Deserialize)]
struct RawScorecard {
    #[serde(default)]
    track_id: String,
    #[serde(default)]
    step_count: u64,
    #[serde(default)]
    mean_quality: f64,
    #[serde(default)]
    total_cost_usd: f64,
    #[serde(default)]
    total_tokens: u64,
    #[serde(default)]
    steps: Vec<RawStep>,
}

/// Read every `<temper_home>/.temper/workflow-measurements/<instance_id>/scorecard.json`,
/// ordered MOST-RECENT-FIRST by the file's mtime. Missing directory, or a
/// directory with no readable scorecards, yields an empty vec (no error).
pub fn read_scorecard_instances(temper_home: &Path) -> Vec<ScorecardInstance> {
    let root = playbook_measurements_dir(temper_home);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    // (mtime, ScorecardInstance) — sorted descending by mtime below.
    let mut dated: Vec<(SystemTime, ScorecardInstance)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(instance_id) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let scorecard_path = path.join("scorecard.json");
        let metadata = match std::fs::metadata(&scorecard_path) {
            Ok(m) => m,
            Err(_) => continue, // no scorecard.json in this instance dir — skip.
        };
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        let raw = match std::fs::read_to_string(&scorecard_path) {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(
                    outcome = "scorecard_read_failed",
                    path = %scorecard_path.display(),
                    error = %e,
                    "temper scorecard read failed (non-fatal, skipped)"
                );
                continue;
            }
        };
        let parsed: RawScorecard = match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    outcome = "scorecard_parse_failed",
                    path = %scorecard_path.display(),
                    error = %e,
                    "temper scorecard parse failed (non-fatal, skipped)"
                );
                continue;
            }
        };

        let scorecard = Scorecard {
            track_id: parsed.track_id,
            step_count: parsed.step_count,
            mean_quality: parsed.mean_quality,
            total_cost_usd: parsed.total_cost_usd,
            total_tokens: parsed.total_tokens,
            steps: parsed
                .steps
                .into_iter()
                .map(|s| ScorecardStep {
                    from_state: s.from_state,
                    to_state: s.to_state,
                    role: s.role,
                    actor: s.actor,
                    quality_score: s.quality_score,
                    model: s.model,
                })
                .collect(),
        };

        dated.push((
            mtime,
            ScorecardInstance {
                instance_id,
                scorecard,
            },
        ));
    }

    // Most-recent-first (descending mtime); stable tie-break by instance_id so
    // repeated reads within the same mtime granularity are deterministic.
    dated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.instance_id.cmp(&b.1.instance_id)));

    dated.into_iter().map(|(_, instance)| instance).collect()
}

/// Mirrors the ARTIFACT-QUALITY grader's per-step record — the real, HARSH
/// 0-10 anchored score of the artifact the step produced (distinct from the
/// generic coherence `quality_score` above). `#[serde(default)]` on every
/// field so an entry missing/carrying extra fields still parses.
#[derive(Debug, Deserialize)]
struct RawArtifactQualityStep {
    #[serde(default)]
    to_state: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    artifact: String,
    #[serde(default)]
    artifact_quality_0_10: f64,
    #[serde(default)]
    band: String,
}

/// Mirrors the on-disk `artifact_quality.json` shape:
/// `{"workflow_id":"...","track_id":"track","steps":[...]}`.
#[derive(Debug, Deserialize)]
struct RawArtifactQuality {
    #[serde(default)]
    workflow_id: String,
    #[serde(default)]
    track_id: String,
    #[serde(default)]
    steps: Vec<RawArtifactQualityStep>,
}

/// Read every `<temper_home>/.temper/workflow-measurements/<instance_id>/artifact_quality.json`
/// — a SIBLING of `scorecard.json` in the same per-instance directory, written
/// by a SEPARATE grader (the real, harsh 0-10 anchored artifact-quality score,
/// as opposed to the generic coherence `quality_score` `scorecard.json`
/// carries). Order is not load-bearing here (the aggregation fold means over
/// all matching instances, unlike the recency-ordered `recent` feed scorecards
/// produce) — instances are returned in directory-listing order. Fail-open: a
/// missing `workflow-measurements` directory, an instance with no
/// `artifact_quality.json`, or a malformed one is skipped (logged at `warn`),
/// never surfaced as an error.
pub fn read_artifact_quality_instances(temper_home: &Path) -> Vec<ArtifactQualityInstance> {
    let root = playbook_measurements_dir(temper_home);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut instances: Vec<ArtifactQualityInstance> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let instance_id = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let quality_path = path.join("artifact_quality.json");
        let raw = match std::fs::read_to_string(&quality_path) {
            Ok(text) => text,
            Err(_) => continue, // no artifact_quality.json in this instance dir — skip.
        };
        let parsed: RawArtifactQuality = match serde_json::from_str(&raw) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    outcome = "artifact_quality_parse_failed",
                    path = %quality_path.display(),
                    error = %e,
                    "temper artifact_quality read failed (non-fatal, skipped)"
                );
                continue;
            }
        };

        let workflow_id = if parsed.workflow_id.is_empty() {
            instance_id
        } else {
            parsed.workflow_id
        };

        instances.push(ArtifactQualityInstance {
            workflow_id,
            track_id: parsed.track_id,
            steps: parsed
                .steps
                .into_iter()
                .map(|s| ArtifactQualityStep {
                    to_state: s.to_state,
                    role: s.role,
                    artifact: s.artifact,
                    artifact_quality_0_10: s.artifact_quality_0_10,
                    band: s.band,
                })
                .collect(),
        });
    }

    instances
}
