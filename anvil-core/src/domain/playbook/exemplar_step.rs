//! Step-grain exemplar Markdown schema and validation.
//!
//! `StepExemplar` is the STEP-GRAIN sibling of [`super::exemplar::Exemplar`]
//! (whole-playbook rubric anchors). It is a DISTINCT, first-class schema —
//! not a variant of the playbook exemplar — with its own closed band
//! vocabulary `{good, bad, ceiling, mediocre}` (playbook exemplars use
//! `{good, bad, trap, hidden_virtue}`; the two vocabularies do not overlap
//! semantically even where the strings coincide).
//!
//! Step exemplars calibrate the per-step grader (temper's
//! `step_quality_grader.py`, via its `parse_fm`) against real distilled
//! anchor text for ONE lifecycle state (e.g. "is THIS spec good?"), rather
//! than judging a whole playbook instance. They live beside playbook
//! exemplars under a kind's `exemplars/` directory, named `step-<state>-*.md`
//! by convention (not enforced here — the frontmatter is authoritative).
//!
//! Raw-artifact rejection is shared with playbook exemplars: this module
//! reuses `exemplar::split_frontmatter`, `exemplar::body_contains_raw_artifact_fence`,
//! and `exemplar::reject_raw_frontmatter_keys` rather than reimplementing them,
//! so the two schemas can never drift on what counts as a forbidden raw marker.

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::fmt;

use super::exemplar::{
    body_contains_raw_artifact_fence, reject_raw_frontmatter_keys, split_frontmatter,
    ExemplarLoadError,
};

/// Closed band vocabulary for step-grain exemplars. Distinct from
/// [`super::exemplar::ALLOWED_BANDS`] — `trap` and `hidden_virtue` are
/// playbook-exemplar-only; `ceiling` and `mediocre` are step-exemplar-only.
pub const STEP_ALLOWED_BANDS: [&str; 4] = ["good", "bad", "ceiling", "mediocre"];

const STEP_GRAIN: &str = "step";

pub fn is_valid_step_band(band: &str) -> bool {
    STEP_ALLOWED_BANDS.contains(&band)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StepExemplarFrontmatter {
    pub grain: String,
    pub state: String,
    pub band: String,
    #[serde(default)]
    pub source_instance: Option<String>,
    #[serde(default)]
    pub synthetic: Option<bool>,
    #[serde(default)]
    pub polished: Option<bool>,
    #[serde(default)]
    pub review_file: Option<String>,
    /// Free-text description of what a synthetic `ceiling` anchor represents
    /// (e.g. "score 10 — the flawless ideal, no real instance reaches this").
    /// Only present on real `ceiling`-band anchors observed in the hearth.
    #[serde(default)]
    pub represents: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepExemplar {
    pub frontmatter: StepExemplarFrontmatter,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepExemplarLoadError {
    MissingFrontmatter,
    InvalidFrontmatter { message: String },
    /// A raw-artifact marker was rejected by the shared playbook-exemplar
    /// helpers. Wraps the underlying `ExemplarLoadError` (always
    /// `RawFrontmatterKey` or `RawArtifactBody`) so `.code()` reuses the
    /// SAME code string as the playbook-exemplar schema — one rejection
    /// vocabulary, not two.
    RawArtifact(ExemplarLoadError),
    InvalidGrain { grain: String },
    InvalidBand { band: String },
    SourceRequired { band: String },
}

impl StepExemplarLoadError {
    pub fn code(&self) -> &'static str {
        match self {
            StepExemplarLoadError::MissingFrontmatter => "step_exemplar_missing_frontmatter",
            StepExemplarLoadError::InvalidFrontmatter { .. } => {
                "step_exemplar_invalid_frontmatter"
            }
            StepExemplarLoadError::RawArtifact(inner) => inner.code(),
            StepExemplarLoadError::InvalidGrain { .. } => "step_exemplar_invalid_grain",
            StepExemplarLoadError::InvalidBand { .. } => "step_exemplar_invalid_band",
            StepExemplarLoadError::SourceRequired { .. } => "step_exemplar_source_required",
        }
    }
}

impl fmt::Display for StepExemplarLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepExemplarLoadError::MissingFrontmatter => write!(
                f,
                "step_exemplar_missing_frontmatter: expected YAML frontmatter"
            ),
            StepExemplarLoadError::InvalidFrontmatter { message } => {
                write!(f, "step_exemplar_invalid_frontmatter: {}", message)
            }
            StepExemplarLoadError::RawArtifact(inner) => write!(f, "{}", inner),
            StepExemplarLoadError::InvalidGrain { grain } => write!(
                f,
                "step_exemplar_invalid_grain: '{}' is not 'step'",
                grain
            ),
            StepExemplarLoadError::InvalidBand { band } => {
                write!(f, "step_exemplar_invalid_band: '{}' is not allowed", band)
            }
            StepExemplarLoadError::SourceRequired { band } => write!(
                f,
                "step_exemplar_source_required: band '{}' requires source_instance",
                band
            ),
        }
    }
}

impl std::error::Error for StepExemplarLoadError {}

pub fn load_step_exemplar_from_markdown(
    markdown: &str,
) -> Result<StepExemplar, StepExemplarLoadError> {
    let (frontmatter_text, body) =
        split_frontmatter(markdown).map_err(|_| StepExemplarLoadError::MissingFrontmatter)?;

    if body_contains_raw_artifact_fence(body) {
        return Err(StepExemplarLoadError::RawArtifact(
            ExemplarLoadError::RawArtifactBody,
        ));
    }

    let value: Value = serde_yaml::from_str(frontmatter_text).map_err(|e| {
        StepExemplarLoadError::InvalidFrontmatter {
            message: e.to_string(),
        }
    })?;
    reject_raw_frontmatter_keys(&value).map_err(StepExemplarLoadError::RawArtifact)?;

    let frontmatter: StepExemplarFrontmatter = serde_yaml::from_value(value).map_err(|e| {
        StepExemplarLoadError::InvalidFrontmatter {
            message: e.to_string(),
        }
    })?;
    validate_step_frontmatter(&frontmatter)?;

    Ok(StepExemplar {
        frontmatter,
        body: body.trim().to_string(),
    })
}

pub fn validate_step_frontmatter(
    frontmatter: &StepExemplarFrontmatter,
) -> Result<(), StepExemplarLoadError> {
    if frontmatter.grain != STEP_GRAIN {
        return Err(StepExemplarLoadError::InvalidGrain {
            grain: frontmatter.grain.clone(),
        });
    }
    if !is_valid_step_band(&frontmatter.band) {
        return Err(StepExemplarLoadError::InvalidBand {
            band: frontmatter.band.clone(),
        });
    }
    if frontmatter.band != "ceiling" && frontmatter.source_instance.is_none() {
        return Err(StepExemplarLoadError::SourceRequired {
            band: frontmatter.band.clone(),
        });
    }
    Ok(())
}
