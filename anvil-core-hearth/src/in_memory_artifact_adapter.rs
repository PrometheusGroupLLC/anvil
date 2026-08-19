use anvil_core::ports::artifact_port::{ArtifactError, ArtifactPort};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// In-memory implementation of `ArtifactPort` for use in brine step
/// definitions and scaffolding features.
///
/// Records created review docs in a `Mutex<Vec<(track_path, doc_name, header)>>`
/// for assertion visibility. Pre-existing docs can be simulated via
/// `with_pre_existing_doc` — the idempotency path returns the existing
/// path without re-recording.
/// A recorded `scaffold_track_directory` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldedTrackRecord {
    pub track_name: String,
    pub parent_id: String,
    pub display_name: String,
    pub status_yaml: String,
}

/// A recorded `persist_generated_playbook` call (track 1a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedPlaybookRecord {
    pub owner_home: String,
    pub kind: String,
    pub machine_yaml: String,
    pub exemplar_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct InMemoryArtifactAdapter {
    /// Tracks created docs as `(track_path, doc_name, header)` tuples.
    pub created_docs: Mutex<Vec<(String, String, String)>>,
    /// Tracks scaffolded track directories in call order.
    pub scaffolded_tracks: Mutex<Vec<ScaffoldedTrackRecord>>,
    /// Tracks `persist_generated_playbook` calls in call order (track 1a).
    pub persisted_playbooks: Mutex<Vec<PersistedPlaybookRecord>>,
    /// Pre-seeded docs that already "exist". Keyed on `(track_path, doc_name)`.
    pre_existing: HashMap<(String, String), String>,
    /// Pre-seeded scaffold track_names that already "exist". Idempotency guard.
    pre_existing_scaffolds: HashSet<String>,
}

impl InMemoryArtifactAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate an already-existing review doc at the given path.
    /// `create_review_doc` will return this path without recording a new
    /// creation entry — exercising the idempotency path.
    pub fn with_pre_existing_doc(&mut self, track_path: &str, doc_name: &str) -> &mut Self {
        let path = format!("{}/{}", track_path, doc_name);
        self.pre_existing
            .insert((track_path.to_string(), doc_name.to_string()), path);
        self
    }

    /// Simulate an already-existing scaffold for the given track_name.
    /// `scaffold_track_directory` will return `ArtifactError::IoError`
    /// with the "track directory already exists" message — exercising
    /// the idempotency error path.
    pub fn with_pre_existing_scaffold(&mut self, track_name: &str) -> &mut Self {
        self.pre_existing_scaffolds.insert(track_name.to_string());
        self
    }
}

impl Clone for InMemoryArtifactAdapter {
    fn clone(&self) -> Self {
        Self {
            created_docs: Mutex::new(self.created_docs.lock().unwrap().clone()),
            scaffolded_tracks: Mutex::new(self.scaffolded_tracks.lock().unwrap().clone()),
            persisted_playbooks: Mutex::new(self.persisted_playbooks.lock().unwrap().clone()),
            pre_existing: self.pre_existing.clone(),
            pre_existing_scaffolds: self.pre_existing_scaffolds.clone(),
        }
    }
}

impl ArtifactPort for InMemoryArtifactAdapter {
    fn create_review_doc(
        &self,
        track_path: &str,
        doc_name: &str,
        header: &str,
    ) -> Result<String, ArtifactError> {
        let key = (track_path.to_string(), doc_name.to_string());
        if let Some(existing_path) = self.pre_existing.get(&key) {
            // Idempotency path: return the pre-existing path without
            // recording a new creation.
            return Ok(existing_path.clone());
        }
        // Record the creation.
        self.created_docs.lock().unwrap().push((
            track_path.to_string(),
            doc_name.to_string(),
            header.to_string(),
        ));
        Ok(format!("{}/{}", track_path, doc_name))
    }

    fn scaffold_track_directory(
        &self,
        track_name: &str,
        parent_id: &str,
        display_name: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError> {
        // Idempotency guard: return an error if the track already exists.
        if self.pre_existing_scaffolds.contains(track_name) {
            return Err(ArtifactError::IoError {
                message: format!("track directory already exists: tracks/{}", track_name),
            });
        }
        self.scaffolded_tracks
            .lock()
            .unwrap()
            .push(ScaffoldedTrackRecord {
                track_name: track_name.to_string(),
                parent_id: parent_id.to_string(),
                display_name: display_name.to_string(),
                status_yaml: status_yaml.to_string(),
            });
        Ok(format!("tracks/{}", track_name))
    }

    fn persist_generated_playbook(
        &self,
        owner_home: &str,
        kind: &str,
        machine_yaml: &str,
        _hooks: Option<&[(String, String)]>,
        exemplars: Option<&[anvil_core::domain::playbook::candidate::GeneratedExemplarFile]>,
    ) -> Result<String, ArtifactError> {
        // Record the call for assertion visibility (mirrors scaffolded_tracks).
        // Returns a synthetic path under the owner-home; no actual I/O.
        self.persisted_playbooks
            .lock()
            .unwrap()
            .push(PersistedPlaybookRecord {
                owner_home: owner_home.to_string(),
                kind: kind.to_string(),
                machine_yaml: machine_yaml.to_string(),
                exemplar_ids: exemplars
                    .unwrap_or(&[])
                    .iter()
                    .map(|exemplar| exemplar.id.clone())
                    .collect(),
            });
        Ok(format!("{}/playbooks/{}/machine.yaml", owner_home, kind))
    }

    fn scaffold_playbook_directory(
        &self,
        playbook_name: &str,
        parent_id: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError> {
        // In-memory: record as a scaffolded track with empty display_name.
        self.scaffolded_tracks
            .lock()
            .unwrap()
            .push(ScaffoldedTrackRecord {
                track_name: playbook_name.to_string(),
                parent_id: parent_id.to_string(),
                display_name: String::new(),
                status_yaml: status_yaml.to_string(),
            });
        Ok(format!("playbooks/{}", playbook_name))
    }

    fn scaffold_artifact_directory(
        &self,
        directory: &str,
        name: &str,
        status_yaml: &str,
        scaffold_files: &[(&str, &str)],
    ) -> Result<String, ArtifactError> {
        // Idempotency guard: return an error if the artifact already exists.
        if self.pre_existing_scaffolds.contains(name) {
            return Err(ArtifactError::IoError {
                message: format!(
                    "{} directory already exists: {}/{}",
                    directory, directory, name
                ),
            });
        }
        // Record the scaffold; the display_name slot carries the scaffold
        // filenames (or empty) so tests can assert it if needed.
        self.scaffolded_tracks
            .lock()
            .unwrap()
            .push(ScaffoldedTrackRecord {
                track_name: name.to_string(),
                parent_id: String::new(),
                display_name: scaffold_files
                    .iter()
                    .map(|(f, _)| (*f).to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                status_yaml: status_yaml.to_string(),
            });
        Ok(format!("{}/{}", directory, name))
    }
}
