use anvil_core::domain::amendment::OpLog;
use anvil_core::domain::shared_types::{ActivityEntry, ActivityLog, RegistryEntry};
use anvil_core::domain::status::FullStatusYaml;
use anvil_core::ports::query_port::{OriginTurnArtifact, QueryError, QueryPort};
use std::collections::{HashMap, HashSet};

/// In-memory implementation of `QueryPort` for use in brine step
/// definitions and scaffolding features.
///
/// Storage is `HashMap`-backed, no `Mutex` needed since the query
/// adapter is a pure reader from the handler's perspective (no interior
/// mutation). Builder methods take `&mut self` to compose well with the
/// `Given` step pattern in brine.
///
/// Derives `Clone` by cloning the `HashMap`s directly.
#[derive(Debug, Clone, Default)]
pub struct InMemoryQueryAdapter {
    /// artifact_id → (kind, state)
    artifact_kinds: HashMap<String, String>,
    artifact_states: HashMap<String, String>,
    /// artifact_id → FullStatusYaml
    artifact_statuses: HashMap<String, FullStatusYaml>,
    /// (track_path, filename) → content
    artifact_texts: HashMap<(String, String), String>,
    /// relative_path → content
    context_files: HashMap<String, String>,
    /// (playbook_id, filename) → content
    playbook_hook_bodies: HashMap<(String, String), String>,
    /// (registry_file, artifact_id) → RegistryEntry
    registry_entries: HashMap<(String, String), RegistryEntry>,
    /// (projection_file, section, track_name) → row_count
    projection_row_counts: HashMap<(String, String, String), usize>,
    /// (artifact_id, target_document) → OpLog
    op_logs: HashMap<(String, String), OpLog>,
    /// artifact_ids whose STRICT transition-evidence read must fail closed,
    /// modelling an on-disk `transitions/` directory holding an unreadable or
    /// unparseable event file. The lenient `read_transitions` still returns the
    /// seeded (legacy) history; only `read_transitions_strict` refuses — exactly
    /// as the filesystem adapter behaves over a corrupt event store.
    damaged_transition_evidence: HashSet<String>,
}

impl InMemoryQueryAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed an artifact with kind and state. Many builder calls derive
    /// these from a single `with_artifact` seed; finer-grained control
    /// is available via the individual setters below.
    ///
    /// Also seeds a minimal `FullStatusYaml` (kind + state, no activity or
    /// transitions) so the adapter stays internally consistent: callers of
    /// `read_artifact_status` / `read_activity_entries` see a status that
    /// agrees with the seeded kind/state, just as a real on-disk artifact
    /// always has a status.yaml. A later `with_status` call overrides this
    /// (e.g. to seed `activity:` / `transitions:`).
    pub fn with_artifact(&mut self, id: &str, kind: &str, state: &str) -> &mut Self {
        self.artifact_kinds.insert(id.to_string(), kind.to_string());
        self.artifact_states
            .insert(id.to_string(), state.to_string());
        self.artifact_statuses
            .entry(id.to_string())
            .or_insert_with(|| FullStatusYaml {
                version: Some(1),
                kind: Some(kind.to_string()),
                state: Some(state.to_string()),
                origin_turn: None,
                parent_id: None,
                actors: None,
                transitions: None,
                activity: None,
                contributed_by: None,
            });
        self
    }

    /// Seed a file inside an artifact directory.
    pub fn with_artifact_text(
        &mut self,
        track_path: &str,
        filename: &str,
        content: &str,
    ) -> &mut Self {
        self.artifact_texts.insert(
            (track_path.to_string(), filename.to_string()),
            content.to_string(),
        );
        self
    }

    /// Seed a hearth context file.
    pub fn with_context_file(&mut self, relative_path: &str, content: &str) -> &mut Self {
        self.context_files
            .insert(relative_path.to_string(), content.to_string());
        self
    }

    /// Seed a playbook hook body for a `(playbook_id, filename)` pair.
    pub fn with_playbook_hook_body(
        &mut self,
        playbook_id: &str,
        filename: &str,
        content: &str,
    ) -> &mut Self {
        self.playbook_hook_bodies.insert(
            (playbook_id.to_string(), filename.to_string()),
            content.to_string(),
        );
        self
    }

    /// Seed a registry entry.
    pub fn with_registry_entry(
        &mut self,
        registry_file: &str,
        artifact_id: &str,
        track_name: &str,
        proposal_name: &str,
    ) -> &mut Self {
        self.registry_entries.insert(
            (registry_file.to_string(), artifact_id.to_string()),
            RegistryEntry {
                track_name: track_name.to_string(),
                proposal_name: proposal_name.to_string(),
            },
        );
        self
    }

    /// Seed a projection row count for a (file, section, track_name)
    /// triple. `0` and `≥2` produce the expected error messages.
    /// Unseeded triples default to `1` (unique row).
    pub fn with_projection_row_count(
        &mut self,
        projection_file: &str,
        section: &str,
        track_name: &str,
        count: usize,
    ) -> &mut Self {
        self.projection_row_counts.insert(
            (
                projection_file.to_string(),
                section.to_string(),
                track_name.to_string(),
            ),
            count,
        );
        self
    }

    /// Seed a full `FullStatusYaml` for an artifact.
    pub fn with_status(&mut self, artifact_id: &str, status: FullStatusYaml) -> &mut Self {
        self.artifact_statuses
            .insert(artifact_id.to_string(), status);
        self
    }

    /// Seed the per-document op log for an artifact. Mirrors
    /// `with_activity_entries` / `with_context_file`: an unseeded
    /// `(artifact_id, target_document)` reads back as an empty `OpLog`.
    pub fn with_op_log(
        &mut self,
        artifact_id: &str,
        target_document: &str,
        log: OpLog,
    ) -> &mut Self {
        self.op_logs
            .insert((artifact_id.to_string(), target_document.to_string()), log);
        self
    }

    /// Mark an artifact's transition evidence as DAMAGED so a STRICT read
    /// (`read_transitions_strict`, the adoption path) fails closed with
    /// `QueryError::AdoptionEvidenceUnreadable`. The lenient `read_transitions`
    /// is unaffected — mirroring a real `transitions/` directory whose event
    /// file cannot be read or parsed.
    pub fn with_damaged_transition_evidence(&mut self, id: &str) -> &mut Self {
        self.damaged_transition_evidence.insert(id.to_string());
        self
    }
}

impl QueryPort for InMemoryQueryAdapter {
    fn list_artifacts(&self) -> Result<Vec<(String, String)>, QueryError> {
        let mut out: Vec<(String, String)> = self
            .artifact_kinds
            .iter()
            .map(|(id, kind)| (id.clone(), kind.clone()))
            .collect();
        // Deterministic order so multiplicity tie-breaking and assertions are
        // stable regardless of HashMap iteration order.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn read_artifact_kind(&self, artifact_id: &str) -> Result<String, QueryError> {
        self.artifact_kinds
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            })
    }

    fn read_artifact_state(&self, artifact_id: &str) -> Result<String, QueryError> {
        self.artifact_states
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            })
    }

    fn read_artifact_status(&self, artifact_id: &str) -> Result<FullStatusYaml, QueryError> {
        self.artifact_statuses
            .get(artifact_id)
            .cloned()
            .ok_or_else(|| QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            })
    }

    fn find_artifact_by_kind_origin_turn(
        &self,
        kind: &str,
        origin_turn: &str,
    ) -> Result<Option<OriginTurnArtifact>, QueryError> {
        if origin_turn.is_empty() {
            return Ok(None);
        }
        let mut matches = self
            .artifact_statuses
            .iter()
            .filter_map(|(artifact_path, status)| {
                if status.kind.as_deref() == Some(kind)
                    && status.origin_turn.as_deref() == Some(origin_turn)
                {
                    anvil_core::domain::transition_log::resolve_state(status).map(|state| {
                        OriginTurnArtifact {
                            artifact_path: artifact_path.clone(),
                            state,
                        }
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.artifact_path.cmp(&b.artifact_path));
        Ok(matches.into_iter().next())
    }

    fn read_activity_entries(&self, artifact_id: &str) -> Result<Vec<ActivityEntry>, QueryError> {
        // An artifact with no seeded status (but a seeded kind/state) is a
        // valid no-activity case — return an empty log rather than NotFound,
        // so the soft-warn detection reads "no open begin" cleanly.
        Ok(self
            .artifact_statuses
            .get(artifact_id)
            .map(|s| s.activity_entries())
            .unwrap_or_default())
    }

    /// Surface the seeded `ActivityLog` WHOLE — including its `dropped`
    /// degradation count — rather than the default reconstruction (which
    /// forces `dropped = 0`). Begin-adoption's fail-closed-on-damage guard
    /// reads this to refuse adoption when the log is degraded, so the
    /// in-memory adapter must preserve a seeded `dropped > 0`.
    fn read_activity_log(&self, artifact_id: &str) -> Result<ActivityLog, QueryError> {
        Ok(self
            .artifact_statuses
            .get(artifact_id)
            .and_then(|s| s.activity.clone())
            .unwrap_or_else(|| ActivityLog::new(Vec::new())))
    }

    fn read_artifact_text(&self, track_path: &str, filename: &str) -> Result<String, QueryError> {
        self.artifact_texts
            .get(&(track_path.to_string(), filename.to_string()))
            .cloned()
            .ok_or_else(|| QueryError::IoError {
                message: format!("File '{}' not found in artifact '{}'", filename, track_path),
            })
    }

    /// STRICT adoption read: refuse when the artifact's transition evidence was
    /// seeded as damaged, otherwise fall back to the lenient read. Models the
    /// filesystem adapter's fail-closed behavior over a corrupt event store.
    fn read_transitions_strict(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<anvil_core::domain::status::StatusTransition>, QueryError> {
        if self.damaged_transition_evidence.contains(artifact_id) {
            return Err(QueryError::AdoptionEvidenceUnreadable {
                artifact_id: artifact_id.to_string(),
                detail: "transition event file is unreadable or unparseable".to_string(),
            });
        }
        self.read_transitions(artifact_id)
    }

    fn read_op_log(&self, artifact_path: &str, target_document: &str) -> Result<OpLog, QueryError> {
        // An unseeded op log reads back empty (mirrors read_activity_entries).
        Ok(self
            .op_logs
            .get(&(artifact_path.to_string(), target_document.to_string()))
            .cloned()
            .unwrap_or_default())
    }

    fn read_context_file(&self, relative_path: &str) -> Result<String, QueryError> {
        self.context_files
            .get(relative_path)
            .cloned()
            .ok_or_else(|| QueryError::IoError {
                message: format!("Context file '{}' not found", relative_path),
            })
    }

    fn read_playbook_hook_body(
        &self,
        playbook_id: &str,
        filename: &str,
    ) -> Result<String, QueryError> {
        if filename.contains('/') || filename.contains("..") {
            return Err(QueryError::IoError {
                message: format!(
                    "Unsafe filename '{}': path components ('/', '..') are not allowed",
                    filename
                ),
            });
        }
        self.playbook_hook_bodies
            .get(&(playbook_id.to_string(), filename.to_string()))
            .cloned()
            .ok_or_else(|| QueryError::IoError {
                message: format!(
                    "Hook file '{}' not found for playbook '{}'",
                    filename, playbook_id
                ),
            })
    }

    fn read_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<RegistryEntry, QueryError> {
        self.registry_entries
            .get(&(registry_file.to_string(), artifact_id.to_string()))
            .cloned()
            .ok_or_else(|| QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            })
    }

    fn check_projection_row_unique(
        &self,
        projection_file: &str,
        track_name: &str,
        from_section: &str,
    ) -> Result<(), QueryError> {
        let count = self
            .projection_row_counts
            .get(&(
                projection_file.to_string(),
                from_section.to_string(),
                track_name.to_string(),
            ))
            .copied()
            .unwrap_or(1); // default: unique row

        match count {
            0 => Err(QueryError::ProjectionRowNotFound {
                message: format!(
                    "No row matching '{}' in '{}' section of {}",
                    track_name, from_section, projection_file
                ),
            }),
            1 => Ok(()),
            n => Err(QueryError::ProjectionRowAmbiguous {
                message: format!(
                    "{} rows matching '{}' in '{}' section of {} — projection has a duplicate-name collision; dedupe or use distinct track names before retrying",
                    n, track_name, from_section, projection_file
                ),
            }),
        }
    }
}
