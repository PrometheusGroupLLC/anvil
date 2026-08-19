//! Step module for `scorecard_agg.feature` (core domain seam).
//!
//! Exercises the pure `fold_scorecard_measurements` fold over an in-memory
//! `Vec<ScorecardInstance>`, built from a single flattened DataTable where
//! consecutive rows sharing the same `instance` column are grouped into one
//! `Scorecard` (each row contributing one step). Row order is preserved as
//! instance order — the fold treats input order as recency order, so this
//! table doubles as the "most-recent-first" fixture for the recent-feed
//! scenario.

use anvil_core::domain::playbook::scorecard_agg::{
    fold_artifact_quality_into_steps, fold_overall_artifact_quality, fold_scorecard_measurements,
    ArtifactQualityInstance, ArtifactQualityStep, Scorecard, ScorecardInstance,
    ScorecardMeasurementSummary, ScorecardStep,
};
use brine_core::parser::DataTable;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, Params, StepDef};

const INSTANCES_KEY: &str = "sca_instances";
const RESULT_KEY: &str = "sca_result";
const ARTIFACT_INSTANCES_KEY: &str = "sca_artifact_instances";
const OVERALL_ARTIFACT_KEY: &str = "sca_overall_artifact_quality";

fn column_index(table: &DataTable, name: &str) -> Result<usize, String> {
    table
        .headers
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| format!("Missing '{}' column in data table", name))
}

fn parse_instances(table: &DataTable) -> Result<Vec<ScorecardInstance>, String> {
    let instance_col = column_index(table, "instance")?;
    let track_col = column_index(table, "track_id")?;
    let mean_col = column_index(table, "mean_quality")?;
    let from_col = column_index(table, "from_state")?;
    let to_col = column_index(table, "to_state")?;
    let role_col = column_index(table, "role")?;
    let actor_col = column_index(table, "actor")?;
    let quality_col = column_index(table, "quality_score")?;
    let model_col = column_index(table, "model")?;

    let mut instances: Vec<ScorecardInstance> = Vec::new();
    for row in &table.rows {
        let instance_id = row[instance_col].trim().to_string();
        let track_id = row[track_col].trim().to_string();
        let mean_quality: f64 = row[mean_col]
            .trim()
            .parse()
            .map_err(|e| format!("bad mean_quality: {}", e))?;
        let quality_score: f64 = row[quality_col]
            .trim()
            .parse()
            .map_err(|e| format!("bad quality_score: {}", e))?;
        let step = ScorecardStep {
            from_state: row[from_col].trim().to_string(),
            to_state: row[to_col].trim().to_string(),
            role: row[role_col].trim().to_string(),
            actor: row[actor_col].trim().to_string(),
            quality_score,
            model: row[model_col].trim().to_string(),
        };

        // Group consecutive-or-not rows sharing the same instance id into one
        // Scorecard, preserving FIRST-APPEARANCE order (recency order).
        if let Some(existing) = instances.iter_mut().find(|i| i.instance_id == instance_id) {
            existing.scorecard.steps.push(step);
            existing.scorecard.step_count = existing.scorecard.steps.len() as u64;
        } else {
            instances.push(ScorecardInstance {
                instance_id,
                scorecard: Scorecard {
                    track_id,
                    step_count: 1,
                    mean_quality,
                    total_cost_usd: 0.0,
                    total_tokens: 0,
                    steps: vec![step],
                },
            });
        }
    }
    Ok(instances)
}

/// Parse the `artifact quality:` data table into `Vec<ArtifactQualityInstance>`,
/// grouping consecutive-or-not rows sharing the same `instance` column into one
/// `ArtifactQualityInstance` (mirrors `parse_instances` above).
fn parse_artifact_quality_instances(table: &DataTable) -> Result<Vec<ArtifactQualityInstance>, String> {
    let instance_col = column_index(table, "instance")?;
    let track_col = column_index(table, "track_id")?;
    let to_col = column_index(table, "to_state")?;
    let role_col = column_index(table, "role")?;
    let artifact_col = column_index(table, "artifact")?;
    let score_col = column_index(table, "artifact_quality_0_10")?;
    let band_col = column_index(table, "band")?;

    let mut instances: Vec<ArtifactQualityInstance> = Vec::new();
    for row in &table.rows {
        let workflow_id = row[instance_col].trim().to_string();
        let track_id = row[track_col].trim().to_string();
        let artifact_quality_0_10: f64 = row[score_col]
            .trim()
            .parse()
            .map_err(|e| format!("bad artifact_quality_0_10: {}", e))?;
        let step = ArtifactQualityStep {
            to_state: row[to_col].trim().to_string(),
            role: row[role_col].trim().to_string(),
            artifact: row[artifact_col].trim().to_string(),
            artifact_quality_0_10,
            band: row[band_col].trim().to_string(),
        };
        if let Some(existing) = instances.iter_mut().find(|i| i.workflow_id == workflow_id) {
            existing.steps.push(step);
        } else {
            instances.push(ArtifactQualityInstance {
                workflow_id,
                track_id,
                steps: vec![step],
            });
        }
    }
    Ok(instances)
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "scorecards:",
            &[],
            &[(INSTANCES_KEY, "Vec<ScorecardInstance>")],
            |_ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut out = Context::new();
                out.set(INSTANCES_KEY, parse_instances(table)?);
                Ok(out)
            },
        ),
        step_def(
            "no scorecards",
            &[],
            &[(INSTANCES_KEY, "Vec<ScorecardInstance>")],
            |_ctx, _params| {
                let mut out = Context::new();
                out.set(INSTANCES_KEY, Vec::<ScorecardInstance>::new());
                Ok(out)
            },
        ),
        step_def(
            "the scorecard measurements are folded for kind {string} with recent limit {int}",
            &[(INSTANCES_KEY, "Vec<ScorecardInstance>")],
            &[
                (RESULT_KEY, "ScorecardMeasurementSummary"),
                (ARTIFACT_INSTANCES_KEY, "Vec<ArtifactQualityInstance>"),
            ],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let recent_limit = params.get_int(1).ok_or("Expected recent limit")? as usize;
                let instances = ctx
                    .get::<Vec<ScorecardInstance>>(INSTANCES_KEY)
                    .cloned()
                    .unwrap_or_default();
                let result = fold_scorecard_measurements(&instances, &kind, recent_limit);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                // Carry forward an already-seeded "artifact quality:" table (a Map
                // step replaces the WHOLE context — see the note on that step def)
                // so a later "artifact quality is folded into..." step can still
                // see it regardless of Given-clause order.
                if let Some(existing) = ctx.get::<Vec<ArtifactQualityInstance>>(ARTIFACT_INSTANCES_KEY) {
                    out.set(ARTIFACT_INSTANCES_KEY, existing.clone());
                }
                Ok(out)
            },
        ),
        check_def(
            "the scorecard summary has instance_count {int}",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as u32;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                if result.instance_count == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected instance_count {}, got {}",
                        expected, result.instance_count
                    ))
                }
            },
        ),
        check_def(
            "the scorecard summary has overall_mean_quality permille {int}",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected permille")? as i64;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                let actual = (result.overall_mean_quality * 1000.0).round() as i64;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected overall_mean_quality permille {}, got {} (raw {})",
                        expected, actual, result.overall_mean_quality
                    ))
                }
            },
        ),
        check_def(
            "the scorecard summary has {int} steps",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                if result.steps.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} steps, got {}",
                        expected,
                        result.steps.len()
                    ))
                }
            },
        ),
        check_def(
            "the scorecard summary has {int} recent entries",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected count")? as usize;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                if result.recent.len() == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "expected {} recent entries, got {}",
                        expected,
                        result.recent.len()
                    ))
                }
            },
        ),
        check_def(
            "the scorecard step from {string} to {string} role {string} has mean_quality permille {int} sample_count {int}",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let expected_permille = params.get_int(3).ok_or("Expected permille")? as i64;
                let expected_count = params.get_int(4).ok_or("Expected sample_count")? as u32;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                let row = result
                    .step(&from_state, &to_state, &role)
                    .ok_or_else(|| {
                        format!(
                            "no step summary for {} -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                let actual_permille = (row.mean_quality * 1000.0).round() as i64;
                if actual_permille != expected_permille {
                    return Err(format!(
                        "expected mean_quality permille {}, got {} (raw {})",
                        expected_permille, actual_permille, row.mean_quality
                    ));
                }
                if row.sample_count != expected_count {
                    return Err(format!(
                        "expected sample_count {}, got {}",
                        expected_count, row.sample_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "recent entry {int} is instance {string} to_state {string} role {string}",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let position = params.get_int(0).ok_or("Expected position")? as usize;
                let expected_instance = params.get_string(1).ok_or("Expected instance")?.to_string();
                let expected_to_state = params.get_string(2).ok_or("Expected to_state")?.to_string();
                let expected_role = params.get_string(3).ok_or("Expected role")?.to_string();
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                let entry = result
                    .recent
                    .get(position.saturating_sub(1))
                    .ok_or_else(|| format!("no recent entry at position {}", position))?;
                if entry.instance_id != expected_instance {
                    return Err(format!(
                        "expected recent entry {} instance '{}', got '{}'",
                        position, expected_instance, entry.instance_id
                    ));
                }
                if entry.to_state != expected_to_state {
                    return Err(format!(
                        "expected recent entry {} to_state '{}', got '{}'",
                        position, expected_to_state, entry.to_state
                    ));
                }
                if entry.role != expected_role {
                    return Err(format!(
                        "expected recent entry {} role '{}', got '{}'",
                        position, expected_role, entry.role
                    ));
                }
                Ok(())
            },
        ),
        // A Map step REPLACES the whole context with only its declared `provides`
        // keys (see brine runner.rs `retain_keys`) — so this step must carry
        // FORWARD any already-seeded `sca_instances` (from a preceding
        // "scorecards:" Given step) alongside the new artifact-quality table, or
        // the later "the scorecard measurements are folded..." step loses it.
        step_def(
            "artifact quality:",
            &[],
            &[
                (INSTANCES_KEY, "Vec<ScorecardInstance>"),
                (ARTIFACT_INSTANCES_KEY, "Vec<ArtifactQualityInstance>"),
            ],
            |ctx, params| {
                let table = params.data_table().ok_or("Expected data table")?;
                let mut out = Context::new();
                if let Some(existing) = ctx.get::<Vec<ScorecardInstance>>(INSTANCES_KEY) {
                    out.set(INSTANCES_KEY, existing.clone());
                }
                out.set(ARTIFACT_INSTANCES_KEY, parse_artifact_quality_instances(table)?);
                Ok(out)
            },
        ),
        step_def(
            "artifact quality is folded into the scorecard steps for kind {string}",
            &[
                (RESULT_KEY, "ScorecardMeasurementSummary"),
                (ARTIFACT_INSTANCES_KEY, "Vec<ArtifactQualityInstance>"),
            ],
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let kind = params.get_string(0).ok_or("Expected kind")?.to_string();
                let mut result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .cloned()
                    .ok_or("No scorecard summary")?;
                let artifact_instances = ctx
                    .get::<Vec<ArtifactQualityInstance>>(ARTIFACT_INSTANCES_KEY)
                    .cloned()
                    .unwrap_or_default();
                result.steps = fold_artifact_quality_into_steps(result.steps, &artifact_instances, &kind);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                Ok(out)
            },
        ),
        check_def(
            "the scorecard step from {string} to {string} role {string} has artifact_quality {int} sample_count {int}",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let expected_score = params.get_int(3).ok_or("Expected artifact_quality")?;
                let expected_count = params.get_int(4).ok_or("Expected sample_count")? as u32;
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                let row = result
                    .steps
                    .iter()
                    .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
                    .ok_or_else(|| {
                        format!(
                            "no step summary for '{}' -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                let actual = row
                    .artifact_quality
                    .ok_or_else(|| format!("step {} -> {} role {} has no artifact_quality", from_state, to_state, role))?;
                if actual.round() as i64 != expected_score {
                    return Err(format!(
                        "expected artifact_quality {}, got {} (raw {})",
                        expected_score, actual.round() as i64, actual
                    ));
                }
                if row.artifact_sample_count != expected_count {
                    return Err(format!(
                        "expected artifact_sample_count {}, got {}",
                        expected_count, row.artifact_sample_count
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the scorecard step from {string} to {string} role {string} has no artifact_quality",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            |ctx, params| {
                let from_state = params.get_string(0).ok_or("Expected from_state")?.to_string();
                let to_state = params.get_string(1).ok_or("Expected to_state")?.to_string();
                let role = params.get_string(2).ok_or("Expected role")?.to_string();
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .ok_or("No scorecard summary")?;
                let row = result
                    .steps
                    .iter()
                    .find(|s| s.from_state == from_state && s.to_state == to_state && s.role == role)
                    .ok_or_else(|| {
                        format!(
                            "no step summary for '{}' -> {} role {}",
                            from_state, to_state, role
                        )
                    })?;
                if row.artifact_quality.is_some() {
                    return Err(format!(
                        "expected no artifact_quality for {} -> {} role {}, got {:?}",
                        from_state, to_state, role, row.artifact_quality
                    ));
                }
                Ok(())
            },
        ),
        step_def(
            "overall artifact quality is folded from the scorecard steps",
            &[(RESULT_KEY, "ScorecardMeasurementSummary")],
            &[
                (RESULT_KEY, "ScorecardMeasurementSummary"),
                (OVERALL_ARTIFACT_KEY, "(f64, u32)"),
            ],
            |ctx, _params| {
                let result = ctx
                    .get::<ScorecardMeasurementSummary>(RESULT_KEY)
                    .cloned()
                    .ok_or("No scorecard summary")?;
                let overall = fold_overall_artifact_quality(&result.steps);
                let mut out = Context::new();
                out.set(RESULT_KEY, result);
                out.set(OVERALL_ARTIFACT_KEY, overall);
                Ok(out)
            },
        ),
        check_def(
            "the scorecard summary has overall_artifact_quality {int} sample_count {int}",
            &[(OVERALL_ARTIFACT_KEY, "(f64, u32)")],
            |ctx, params| {
                let expected_score = params.get_int(0).ok_or("Expected overall_artifact_quality")?;
                let expected_count = params.get_int(1).ok_or("Expected sample_count")? as u32;
                let (mean, count) = ctx
                    .get::<(f64, u32)>(OVERALL_ARTIFACT_KEY)
                    .ok_or("No overall artifact quality result")?;
                if mean.round() as i64 != expected_score {
                    return Err(format!(
                        "expected overall_artifact_quality {}, got {} (raw {})",
                        expected_score,
                        mean.round() as i64,
                        mean
                    ));
                }
                if *count != expected_count {
                    return Err(format!(
                        "expected overall artifact quality sample_count {}, got {}",
                        expected_count, count
                    ));
                }
                Ok(())
            },
        ),
    ]
}
