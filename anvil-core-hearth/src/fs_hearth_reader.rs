use anvil_core::domain::status::FullStatusYaml;
use anvil_core::domain::{ArtifactSummary, ArtifactType, HearthError, ALL_ARTIFACT_TYPES};
use anvil_core::ports::hearth_reader::HearthReaderPort;
use std::collections::HashMap;
use std::path::PathBuf;
use anvil_core::ports::backlog_item_port::BacklogItemPort;

/// Filesystem hearth reader — reads artifact state from a hearth directory.
///
/// Scans `proposals/`, `tracks/`, `milestones/`, `initiatives/`, and
/// `decisions/` subdirectories. For each artifact directory found, reads
/// `status.yaml` to extract the current state. Derives summaries from
/// registry files as the primary source, falling back to the directory
/// name when no registry entry exists.
pub struct FileSystemHearthReader {
    hearth_path: PathBuf,
}

impl FileSystemHearthReader {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

// Uses FullStatusYaml from domain::status (shared parser).

/// Parse a registry file and extract summaries keyed by artifact directory name.
///
/// Registry entries follow the format:
/// `- [Name](path/to/artifact/) — summary description`
///
/// Returns a map from directory name to summary string.
fn parse_registry_summaries(content: &str) -> HashMap<String, String> {
    let mut summaries = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        // Match: - [Name](path/) — summary
        if !line.starts_with("- [") {
            continue;
        }

        // Extract the path between ( and )
        let path_start = match line.find("](") {
            Some(i) => i + 2,
            None => continue,
        };
        let path_end = match line[path_start..].find(')') {
            Some(i) => path_start + i,
            None => continue,
        };
        let path = &line[path_start..path_end];

        // Extract the directory name from the path (last component, strip trailing /)
        let dir_name = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(path);

        // Extract the summary after " — "
        let after_paren = &line[path_end + 1..];
        if let Some(dash_pos) = after_paren.find(" — ") {
            let summary = after_paren[dash_pos + " — ".len()..].trim();
            if !summary.is_empty() {
                summaries.insert(dir_name.to_string(), summary.to_string());
            }
        }
    }

    summaries
}

/// Derive a summary from the directory name when no registry entry exists.
fn summary_from_directory_name(id: &str) -> String {
    // For date-prefixed names like 20260403T1500_forge_lifecycle,
    // skip the timestamp and join the rest with spaces
    let parts: Vec<&str> = id.split('_').collect();
    if parts.len() > 1
        && parts[0].len() >= 8
        && parts[0].chars().all(|c| c.is_ascii_digit() || c == 'T')
    {
        parts[1..].join(" ")
    } else {
        // For non-date-prefixed names like follow-forge-lifecycle
        id.replace('-', " ")
    }
}

impl HearthReaderPort for FileSystemHearthReader {
    fn list_artifacts(&self) -> Result<Vec<ArtifactSummary>, HearthError> {
        // First-read K8 recovery (plan Task 4): Catalog / Checkin may be the
        // FIRST caller after an interrupted backlog transaction. Recovering
        // before the scan is what stops a stale registry row or a partial item
        // from being surfaced as truth. Idempotent; the engine holds the
        // resolved-hearth lock around this call.
        crate::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
            self.hearth_path.clone(),
        )
        .recover()
        .map_err(|e| HearthError::IoError {
            message: format!("backlog recovery before scan: {e}"),
        })?;
        if !self.hearth_path.exists() {
            return Err(HearthError::HearthNotFound {
                path: self.hearth_path.display().to_string(),
                message: "Directory does not exist".to_string(),
            });
        }

        // Pre-load registry summaries for all artifact types
        let mut registry_summaries: HashMap<String, String> = HashMap::new();
        for artifact_type in ALL_ARTIFACT_TYPES {
            let registry_path = self.hearth_path.join(artifact_type.registry_file());
            if let Ok(content) = std::fs::read_to_string(&registry_path) {
                registry_summaries.extend(parse_registry_summaries(&content));
            }
        }

        let mut artifacts = Vec::new();

        for artifact_type in ALL_ARTIFACT_TYPES {
            let directory_names: Vec<&str> = if *artifact_type == ArtifactType::Playbook {
                vec!["playbooks"]
            } else {
                vec![artifact_type.directory_name()]
            };
            for directory_name in &directory_names {
                let type_dir = self.hearth_path.join(directory_name);
                if !type_dir.exists() {
                    continue;
                }

                let entries = std::fs::read_dir(&type_dir).map_err(|e| HearthError::IoError {
                    message: format!("Failed to read {}: {}", type_dir.display(), e),
                })?;

                for entry in entries {
                    let entry = entry.map_err(|e| HearthError::IoError {
                        message: format!("Failed to read directory entry: {}", e),
                    })?;

                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    let id = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let status_path = path.join("status.yaml");
                    if !status_path.exists() {
                        continue;
                    }

                    let content = std::fs::read_to_string(&status_path).map_err(|e| {
                        HearthError::MalformedStatus {
                            artifact_id: id.clone(),
                            message: format!("Failed to read status.yaml: {}", e),
                        }
                    })?;

                    let status: FullStatusYaml = serde_yaml::from_str(&content).map_err(|e| {
                        HearthError::MalformedStatus {
                            artifact_id: id.clone(),
                            message: format!("Invalid YAML: {}", e),
                        }
                    })?;

                    // Resolve state via the shared accessor: top-level `state`,
                    // else the last transition's `to`. A stateless-but-transitioned
                    // artifact resolves to its last transition. A genuinely-
                    // unresolvable artifact (no state AND no transitions) is
                    // surfaced as a degraded "unknown" entry — visible (so
                    // corruption stays observable), not silently skipped, and not
                    // an error that aborts the whole catalog. Other artifacts still
                    // return.
                    // C-d.1 round 8, H-1 (forced touch). `resolve_state_with_events`
                    // is fallible now. This adapter is the SCOPED subject of the
                    // follow-on adapter-sweep track and its own predicates are
                    // deliberately not touched here — but a swallow could not be
                    // left standing at a seam this round made fallible, so
                    // unreadable event evidence PROPAGATES. `None` (genuinely no
                    // state and no transition) keeps its degraded "unknown"
                    // entry, which is a different fact and stays visible.
                    let state = anvil_core::domain::transition_log::resolve_state_with_events(
                        &status, &path,
                    )
                    .map_err(|e| HearthError::IoError {
                        message: format!(
                            "artifact_state_uninspectable: the transition events for '{}' could \
                             not be read: {e}",
                            id
                        ),
                    })?
                    .unwrap_or_else(|| "unknown".to_string());

                    // Summary: prefer registry entry, fall back to directory name
                    let summary = registry_summaries
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| summary_from_directory_name(&id));

                    artifacts.push(ArtifactSummary {
                        id,
                        artifact_type: *artifact_type,
                        state,
                        summary,
                        execution_route: String::new(),
                    });
                }
            }
        }

        Ok(artifacts)
    }

    fn read_playbook_machine_yaml(&self, artifact_id: &str) -> Result<Option<String>, HearthError> {
        let machine_path = self
            .hearth_path
            .join("playbooks")
            .join(artifact_id)
            .join("machine.yaml");
        let legacy_machine_path = self
            .hearth_path
            .join("workflows")
            .join(artifact_id)
            .join("machine.yaml");
        let machine_path = if machine_path.exists() {
            machine_path
        } else if legacy_machine_path.exists() {
            legacy_machine_path
        } else {
            return Ok(None);
        };
        let content = std::fs::read_to_string(&machine_path).map_err(|e| HearthError::IoError {
            message: format!(
                "Failed to read machine.yaml for playbook '{}': {}",
                artifact_id, e
            ),
        })?;
        Ok(Some(content))
    }

    fn list_playbook_hooks(&self, artifact_id: &str) -> Result<Vec<String>, HearthError> {
        let hooks_dir = self
            .hearth_path
            .join("playbooks")
            .join(artifact_id)
            .join("hooks");
        let legacy_hooks_dir = self
            .hearth_path
            .join("workflows")
            .join(artifact_id)
            .join("hooks");
        let hooks_dir = if hooks_dir.exists() {
            hooks_dir
        } else if legacy_hooks_dir.exists() {
            legacy_hooks_dir
        } else {
            return Ok(Vec::new());
        };
        let entries = std::fs::read_dir(&hooks_dir).map_err(|e| HearthError::IoError {
            message: format!(
                "Failed to read hooks/ directory for playbook '{}': {}",
                artifact_id, e
            ),
        })?;
        let mut filenames = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| HearthError::IoError {
                message: format!(
                    "Failed to read hooks/ entry for playbook '{}': {}",
                    artifact_id, e
                ),
            })?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    filenames.push(name.to_string_lossy().to_string());
                }
            }
        }
        filenames.sort();
        Ok(filenames)
    }
}
