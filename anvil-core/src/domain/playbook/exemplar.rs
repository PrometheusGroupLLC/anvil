//! Canonical exemplar Markdown schema and validation.
//!
//! Exemplars live under a playbook kind's `exemplars/` directory as Markdown
//! files with YAML frontmatter. The loadable body is only the distilled,
//! redacted pattern; raw artifacts are rejected at the schema boundary.

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::fmt;

use super::types::{is_valid_quality_dimension, EvidenceClass};

pub const ALLOWED_BANDS: [&str; 4] = ["good", "bad", "trap", "hidden_virtue"];

const RAW_FRONTMATTER_KEYS: [&str; 3] = ["raw", "raw_artifact", "raw_artifact_ref"];
const RAW_ARTIFACT_FENCE: &str = "```raw-artifact";

pub fn is_valid_band(band: &str) -> bool {
    ALLOWED_BANDS.contains(&band)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExemplarFrontmatter {
    pub id: String,
    pub band: String,
    #[serde(default)]
    pub dimensions: Vec<String>,
    pub evidence_class: EvidenceClass,
    #[serde(default)]
    pub outcome_link: Option<OutcomeLink>,
    pub provenance: ExemplarProvenance,
    pub playbook_version: String,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutcomeLink {
    pub authority: String,
    pub opaque_ref: String,
    #[serde(default)]
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExemplarProvenance {
    pub source: String,
    pub corpus: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exemplar {
    pub frontmatter: ExemplarFrontmatter,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExemplarLoadError {
    MissingFrontmatter,
    InvalidFrontmatter { message: String },
    RawFrontmatterKey { key: String },
    RawArtifactBody,
    InvalidBand { band: String },
    UnknownDimension { dimension: String },
    OutcomeLinkRequired,
}

impl ExemplarLoadError {
    pub fn code(&self) -> &'static str {
        match self {
            ExemplarLoadError::MissingFrontmatter => "exemplar_missing_frontmatter",
            ExemplarLoadError::InvalidFrontmatter { .. } => "exemplar_invalid_frontmatter",
            ExemplarLoadError::RawFrontmatterKey { .. } => "exemplar_raw_frontmatter_key",
            ExemplarLoadError::RawArtifactBody => "exemplar_raw_artifact_body",
            ExemplarLoadError::InvalidBand { .. } => "exemplar_invalid_band",
            ExemplarLoadError::UnknownDimension { .. } => "exemplar_unknown_dimension",
            ExemplarLoadError::OutcomeLinkRequired => "exemplar_outcome_link_required",
        }
    }
}

impl fmt::Display for ExemplarLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExemplarLoadError::MissingFrontmatter => {
                write!(f, "exemplar_missing_frontmatter: expected YAML frontmatter")
            }
            ExemplarLoadError::InvalidFrontmatter { message } => {
                write!(f, "exemplar_invalid_frontmatter: {}", message)
            }
            ExemplarLoadError::RawFrontmatterKey { key } => write!(
                f,
                "exemplar_raw_frontmatter_key: raw artifact key '{}' is forbidden",
                key
            ),
            ExemplarLoadError::RawArtifactBody => write!(
                f,
                "exemplar_raw_artifact_body: raw-artifact body fence is forbidden"
            ),
            ExemplarLoadError::InvalidBand { band } => {
                write!(f, "exemplar_invalid_band: '{}' is not allowed", band)
            }
            ExemplarLoadError::UnknownDimension { dimension } => write!(
                f,
                "exemplar_unknown_dimension: '{}' is not a quality dimension",
                dimension
            ),
            ExemplarLoadError::OutcomeLinkRequired => write!(
                f,
                "exemplar_outcome_link_required: artifact_of_consequence requires outcome_link"
            ),
        }
    }
}

impl std::error::Error for ExemplarLoadError {}

pub fn load_from_markdown(markdown: &str) -> Result<Exemplar, ExemplarLoadError> {
    let (frontmatter_text, body) = split_frontmatter(markdown)?;

    if body_contains_raw_artifact_fence(body) {
        return Err(ExemplarLoadError::RawArtifactBody);
    }

    let value: Value = serde_yaml::from_str(frontmatter_text).map_err(|e| {
        ExemplarLoadError::InvalidFrontmatter {
            message: e.to_string(),
        }
    })?;
    reject_raw_frontmatter_keys(&value)?;

    let frontmatter: ExemplarFrontmatter =
        serde_yaml::from_value(value).map_err(|e| ExemplarLoadError::InvalidFrontmatter {
            message: e.to_string(),
        })?;
    validate_frontmatter(&frontmatter)?;

    Ok(Exemplar {
        frontmatter,
        body: body.trim().to_string(),
    })
}

pub fn validate_frontmatter(frontmatter: &ExemplarFrontmatter) -> Result<(), ExemplarLoadError> {
    if !is_valid_band(&frontmatter.band) {
        return Err(ExemplarLoadError::InvalidBand {
            band: frontmatter.band.clone(),
        });
    }
    for dimension in &frontmatter.dimensions {
        if !is_valid_quality_dimension(dimension) {
            return Err(ExemplarLoadError::UnknownDimension {
                dimension: dimension.clone(),
            });
        }
    }
    if frontmatter.evidence_class == EvidenceClass::ArtifactOfConsequence
        && frontmatter.outcome_link.is_none()
    {
        return Err(ExemplarLoadError::OutcomeLinkRequired);
    }
    Ok(())
}

pub(super) fn split_frontmatter(markdown: &str) -> Result<(&str, &str), ExemplarLoadError> {
    let rest = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"))
        .ok_or(ExemplarLoadError::MissingFrontmatter)?;
    if let Some(idx) = rest.find("\n---\n") {
        let frontmatter = &rest[..idx];
        let body = &rest[idx + "\n---\n".len()..];
        return Ok((frontmatter, body));
    }
    if let Some(idx) = rest.find("\r\n---\r\n") {
        let frontmatter = &rest[..idx];
        let body = &rest[idx + "\r\n---\r\n".len()..];
        return Ok((frontmatter, body));
    }
    Err(ExemplarLoadError::MissingFrontmatter)
}

pub(super) fn body_contains_raw_artifact_fence(body: &str) -> bool {
    body.lines()
        .map(str::trim_start)
        .any(|line| line.starts_with(RAW_ARTIFACT_FENCE))
}

pub(super) fn reject_raw_frontmatter_keys(value: &Value) -> Result<(), ExemplarLoadError> {
    let Some(mapping) = value.as_mapping() else {
        return Err(ExemplarLoadError::InvalidFrontmatter {
            message: "frontmatter must be a mapping".to_string(),
        });
    };
    reject_raw_keys_in_mapping(mapping)
}

fn reject_raw_keys_in_mapping(mapping: &Mapping) -> Result<(), ExemplarLoadError> {
    for (key, value) in mapping {
        if let Some(key) = key.as_str() {
            if RAW_FRONTMATTER_KEYS.contains(&key) {
                return Err(ExemplarLoadError::RawFrontmatterKey {
                    key: key.to_string(),
                });
            }
        }
        if let Some(nested) = value.as_mapping() {
            reject_raw_keys_in_mapping(nested)?;
        }
        if let Some(sequence) = value.as_sequence() {
            for item in sequence {
                if let Some(nested) = item.as_mapping() {
                    reject_raw_keys_in_mapping(nested)?;
                }
            }
        }
    }
    Ok(())
}
