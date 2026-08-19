//! Filesystem anchor resolution for playbook exemplars.
//!
//! This module intentionally stays separate from `loader::load_from_yaml`.
//! Playbook registration remains lazy with respect to anchors; callers resolve
//! them only when they need exemplar content.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use super::exemplar::{is_valid_band, load_from_markdown, Exemplar, ExemplarLoadError};
use super::types::{AnchorRef, SuccessRubric};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExemplar {
    pub anchor: AnchorRef,
    pub path: PathBuf,
    pub exemplar: Exemplar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorResolution {
    pub exemplars: Vec<ResolvedExemplar>,
    pub warnings: Vec<AnchorResolutionWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorResolutionWarning {
    StalePlaybookVersion {
        instance: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorResolutionError {
    InvalidAnchorBand {
        instance: String,
        band: String,
    },
    MissingAnchor {
        instance: String,
        path: PathBuf,
    },
    DuplicateId {
        id: String,
        paths: Vec<PathBuf>,
    },
    ReadFailed {
        path: PathBuf,
        message: String,
    },
    LoadFailed {
        path: PathBuf,
        source: ExemplarLoadError,
    },
}

impl AnchorResolutionError {
    pub fn code(&self) -> &'static str {
        match self {
            AnchorResolutionError::InvalidAnchorBand { .. } => "exemplar_invalid_anchor_band",
            AnchorResolutionError::MissingAnchor { .. } => "exemplar_anchor_missing",
            AnchorResolutionError::DuplicateId { .. } => "exemplar_duplicate_id",
            AnchorResolutionError::ReadFailed { .. } => "exemplar_read_failed",
            AnchorResolutionError::LoadFailed { source, .. } => source.code(),
        }
    }
}

impl fmt::Display for AnchorResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnchorResolutionError::InvalidAnchorBand { instance, band } => write!(
                f,
                "exemplar_invalid_anchor_band: anchor '{}' uses invalid band '{}'",
                instance, band
            ),
            AnchorResolutionError::MissingAnchor { instance, path } => write!(
                f,
                "exemplar_anchor_missing: anchor '{}' expected file '{}'",
                instance,
                path.display()
            ),
            AnchorResolutionError::DuplicateId { id, paths } => write!(
                f,
                "exemplar_duplicate_id: id '{}' appears in {} files",
                id,
                paths.len()
            ),
            AnchorResolutionError::ReadFailed { path, message } => {
                write!(f, "exemplar_read_failed: '{}': {}", path.display(), message)
            }
            AnchorResolutionError::LoadFailed { path, source } => {
                write!(f, "{}: '{}'", source, path.display())
            }
        }
    }
}

impl std::error::Error for AnchorResolutionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExemplarCoverage {
    pub covered: Vec<String>,
    pub uncovered: Vec<String>,
}

impl ExemplarCoverage {
    pub fn is_complete(&self) -> bool {
        self.uncovered.is_empty()
    }
}

pub fn resolve_anchors(
    kind_dir: &Path,
    rubric: &SuccessRubric,
    expected_playbook_version: &str,
) -> Result<AnchorResolution, AnchorResolutionError> {
    let exemplars_dir = kind_dir.join("exemplars");
    let by_id = load_exemplar_index(&exemplars_dir)?;

    let mut resolved = Vec::new();
    let mut warnings = Vec::new();
    for anchor in &rubric.anchors {
        if !is_valid_band(&anchor.band) {
            return Err(AnchorResolutionError::InvalidAnchorBand {
                instance: anchor.instance.clone(),
                band: anchor.band.clone(),
            });
        }

        let path = exemplars_dir.join(format!("{}.md", anchor.instance));
        if !path.is_file() {
            return Err(AnchorResolutionError::MissingAnchor {
                instance: anchor.instance.clone(),
                path,
            });
        }

        let Some((indexed_path, exemplar)) = by_id.get(&anchor.instance) else {
            return Err(AnchorResolutionError::MissingAnchor {
                instance: anchor.instance.clone(),
                path,
            });
        };

        if exemplar.frontmatter.playbook_version != expected_playbook_version {
            warnings.push(AnchorResolutionWarning::StalePlaybookVersion {
                instance: anchor.instance.clone(),
                expected: expected_playbook_version.to_string(),
                actual: exemplar.frontmatter.playbook_version.clone(),
            });
        }

        resolved.push(ResolvedExemplar {
            anchor: anchor.clone(),
            path: indexed_path.clone(),
            exemplar: exemplar.clone(),
        });
    }

    Ok(AnchorResolution {
        exemplars: resolved,
        warnings,
    })
}

pub fn exemplar_coverage(
    rubric: &SuccessRubric,
    exemplars: &[ResolvedExemplar],
) -> ExemplarCoverage {
    let mut covered_set: BTreeSet<String> = BTreeSet::new();
    for resolved in exemplars {
        for dimension in &resolved.exemplar.frontmatter.dimensions {
            covered_set.insert(dimension.clone());
        }
    }

    let mut covered = Vec::new();
    let mut uncovered = Vec::new();
    for entry in &rubric.dimensions {
        if covered_set.contains(&entry.dimension) {
            covered.push(entry.dimension.clone());
        } else {
            uncovered.push(entry.dimension.clone());
        }
    }
    ExemplarCoverage { covered, uncovered }
}

fn load_exemplar_index(
    exemplars_dir: &Path,
) -> Result<HashMap<String, (PathBuf, Exemplar)>, AnchorResolutionError> {
    let mut by_id: HashMap<String, (PathBuf, Exemplar)> = HashMap::new();
    let mut duplicates: HashMap<String, Vec<PathBuf>> = HashMap::new();

    let Ok(entries) = std::fs::read_dir(exemplars_dir) else {
        return Ok(by_id);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let text =
            std::fs::read_to_string(&path).map_err(|e| AnchorResolutionError::ReadFailed {
                path: path.clone(),
                message: e.to_string(),
            })?;
        let exemplar =
            load_from_markdown(&text).map_err(|source| AnchorResolutionError::LoadFailed {
                path: path.clone(),
                source,
            })?;

        let id = exemplar.frontmatter.id.clone();
        if let Some((first_path, _)) = by_id.get(&id) {
            duplicates
                .entry(id)
                .or_insert_with(|| vec![first_path.clone()])
                .push(path);
        } else {
            by_id.insert(id, (path, exemplar));
        }
    }

    if let Some((id, mut paths)) = duplicates.into_iter().next() {
        paths.sort();
        return Err(AnchorResolutionError::DuplicateId { id, paths });
    }

    Ok(by_id)
}
