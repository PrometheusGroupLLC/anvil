use anvil_core::domain::shared_types::{ActivityEntry, ActorIdentity, TransitionContent};
use anvil_core::domain::snapshot::SnapshotError;
use anvil_core::domain::status::StatusTransition;
use anvil_core::ports::snapshot_port::SnapshotPort;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// In-memory test adapter for the snapshot handler. Recorders expose
/// every port call so features can assert routing and dispatch
/// behavior without filesystem interaction.
#[derive(Debug, Default)]
pub struct TestSnapshotAdapter {
    // Seeded lookups
    kinds: HashMap<String, String>,
    states: HashMap<String, String>,
    actor_names: HashMap<String, Vec<String>>,
    activity_entries: HashMap<String, Vec<ActivityEntry>>,
    transitions: HashMap<String, Vec<StatusTransition>>,
    existing_registry_entries: Mutex<HashSet<(String, String)>>,
    fixed_generated_names: Mutex<VecDeque<String>>,
    fixed_registry_entry_text: HashMap<String, String>,

    // Recorders
    pub appended_transitions: Mutex<Vec<(String, TransitionContent)>>,
    pub seeded_actors: Mutex<Vec<(String, ActorIdentity)>>,
    pub created_registry_entries: Mutex<Vec<(String, String, String)>>,
    pub moved_registry_entries: Mutex<Vec<(String, String, String)>>,
    pub moved_execution_rows: Mutex<Vec<(String, String)>>,
    pub moved_intent_rows: Mutex<Vec<(String, String, String)>>,
    pub rebuilt_decisions_projection: Mutex<Vec<()>>,
    pub rebuilt_sparks_projection: Mutex<Vec<()>>,
    pub wrote_authoring_projections: Mutex<Vec<(String, String, String)>>,
    pub wrote_artifact_projections: Mutex<Vec<(String, String, String)>>,

    // State-machine projection declaration: (artifact_path, to_state) pairs
    // whose machine declares projection_targets.
    declares_projection: HashSet<(String, String)>,

    // Error injection
    fail_status_append: bool,
    fail_registry_move: bool,
    fail_projection_update: bool,
}

impl TestSnapshotAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_artifact(mut self, id: &str, kind: &str, state: &str) -> Self {
        self.kinds.insert(id.to_string(), kind.to_string());
        self.states.insert(id.to_string(), state.to_string());
        self
    }

    pub fn with_status(mut self, id: &str, actors: &[&str]) -> Self {
        self.actor_names.insert(
            id.to_string(),
            actors.iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    /// Seed the artifact's `activity:` begin-marker log for begin-adoption
    /// detection scenarios.
    pub fn with_activity(mut self, id: &str, activity: Vec<ActivityEntry>) -> Self {
        self.activity_entries.insert(id.to_string(), activity);
        self
    }

    /// Seed the artifact's `transitions:` history (e.g. the creation
    /// transition) for begin-adoption create-exemption scenarios.
    pub fn with_transitions(mut self, id: &str, transitions: Vec<StatusTransition>) -> Self {
        self.transitions.insert(id.to_string(), transitions);
        self
    }

    pub fn with_existing_registry_entry(self, registry_file: &str, artifact_id: &str) -> Self {
        self.existing_registry_entries
            .lock()
            .unwrap()
            .insert((registry_file.to_string(), artifact_id.to_string()));
        self
    }

    pub fn with_fixed_generated_name(self, name: &str) -> Self {
        self.fixed_generated_names
            .lock()
            .unwrap()
            .push_back(name.to_string());
        self
    }

    pub fn with_fixed_registry_entry_text(mut self, artifact_path: &str, text: &str) -> Self {
        self.fixed_registry_entry_text
            .insert(artifact_path.to_string(), text.to_string());
        self
    }

    /// Mark the `(artifact_path, to_state)` pair as one whose machine
    /// declares `projection_targets`, so `state_declares_projection`
    /// returns `true` for it.
    pub fn with_state_declaring_projection(mut self, artifact_path: &str, to_state: &str) -> Self {
        self.declares_projection
            .insert((artifact_path.to_string(), to_state.to_string()));
        self
    }

    pub fn with_fail_status_append(mut self) -> Self {
        self.fail_status_append = true;
        self
    }

    pub fn with_fail_registry_move(mut self) -> Self {
        self.fail_registry_move = true;
        self
    }

    pub fn with_fail_projection_update(mut self) -> Self {
        self.fail_projection_update = true;
        self
    }
}

impl SnapshotPort for TestSnapshotAdapter {
    fn read_artifact_kind(&self, artifact_path: &str) -> Result<String, SnapshotError> {
        self.kinds
            .get(artifact_path)
            .cloned()
            .ok_or_else(|| SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            })
    }

    fn read_artifact_state(&self, artifact_path: &str) -> Result<String, SnapshotError> {
        self.states
            .get(artifact_path)
            .cloned()
            .ok_or_else(|| SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            })
    }

    fn read_artifact_actor_names(&self, artifact_path: &str) -> Result<Vec<String>, SnapshotError> {
        Ok(self
            .actor_names
            .get(artifact_path)
            .cloned()
            .unwrap_or_default())
    }

    fn read_activity_entries(
        &self,
        artifact_path: &str,
    ) -> Result<Vec<ActivityEntry>, SnapshotError> {
        Ok(self
            .activity_entries
            .get(artifact_path)
            .cloned()
            .unwrap_or_default())
    }

    fn read_transitions(
        &self,
        artifact_path: &str,
    ) -> Result<Vec<StatusTransition>, SnapshotError> {
        Ok(self
            .transitions
            .get(artifact_path)
            .cloned()
            .unwrap_or_default())
    }

    fn append_transition(
        &self,
        artifact_path: &str,
        transition: &TransitionContent,
    ) -> Result<(), SnapshotError> {
        if self.fail_status_append {
            return Err(SnapshotError::IoError {
                message: "injected status-append failure".to_string(),
            });
        }
        self.appended_transitions
            .lock()
            .unwrap()
            .push((artifact_path.to_string(), transition.clone()));
        Ok(())
    }

    fn seed_actor(&self, artifact_path: &str, actor: &ActorIdentity) -> Result<(), SnapshotError> {
        self.seeded_actors
            .lock()
            .unwrap()
            .push((artifact_path.to_string(), actor.clone()));
        Ok(())
    }

    fn registry_entry_exists(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<bool, SnapshotError> {
        Ok(self
            .existing_registry_entries
            .lock()
            .unwrap()
            .contains(&(registry_file.to_string(), artifact_id.to_string())))
    }

    fn build_registry_entry_text(
        &self,
        _artifact_kind: &str,
        artifact_path: &str,
        _to_section: &str,
    ) -> Result<String, SnapshotError> {
        match self.fixed_registry_entry_text.get(artifact_path) {
            Some(text) => Ok(text.clone()),
            None => Err(SnapshotError::IoError {
                message: format!(
                    "no fixed_registry_entry_text seeded for '{}'",
                    artifact_path
                ),
            }),
        }
    }

    fn create_registry_entry(
        &self,
        registry_file: &str,
        _artifact_id: &str,
        _artifact_kind: &str,
        to_section: &str,
        entry_text: &str,
    ) -> Result<(), SnapshotError> {
        if self.fail_registry_move {
            return Err(SnapshotError::IoError {
                message: "injected registry-write failure".to_string(),
            });
        }
        self.created_registry_entries.lock().unwrap().push((
            registry_file.to_string(),
            to_section.to_string(),
            entry_text.to_string(),
        ));
        Ok(())
    }

    fn move_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError> {
        if self.fail_registry_move {
            return Err(SnapshotError::IoError {
                message: "injected registry-move failure".to_string(),
            });
        }
        self.moved_registry_entries.lock().unwrap().push((
            registry_file.to_string(),
            artifact_id.to_string(),
            to_section.to_string(),
        ));
        Ok(())
    }

    fn move_execution_row(&self, track_name: &str, to_section: &str) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.moved_execution_rows
            .lock()
            .unwrap()
            .push((track_name.to_string(), to_section.to_string()));
        Ok(())
    }

    fn move_intent_row(
        &self,
        artifact_name: &str,
        kind: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.moved_intent_rows.lock().unwrap().push((
            artifact_name.to_string(),
            kind.to_string(),
            to_section.to_string(),
        ));
        Ok(())
    }

    fn rebuild_decisions_projection(&self) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.rebuilt_decisions_projection.lock().unwrap().push(());
        Ok(())
    }

    fn rebuild_sparks_projection(&self) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.rebuilt_sparks_projection.lock().unwrap().push(());
        Ok(())
    }

    fn write_authoring_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        _at: &str,
        _actor: &str,
        _role: &str,
    ) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.wrote_authoring_projections.lock().unwrap().push((
            artifact_path.to_string(),
            phase_label.to_string(),
            state.to_string(),
        ));
        Ok(())
    }

    fn state_declares_projection(
        &self,
        artifact_path: &str,
        to_state: &str,
    ) -> Result<bool, SnapshotError> {
        Ok(self
            .declares_projection
            .contains(&(artifact_path.to_string(), to_state.to_string())))
    }

    fn write_artifact_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        _at: &str,
        _actor: &str,
        _role: &str,
    ) -> Result<(), SnapshotError> {
        if self.fail_projection_update {
            return Err(SnapshotError::IoError {
                message: "injected projection-update failure".to_string(),
            });
        }
        self.wrote_artifact_projections.lock().unwrap().push((
            artifact_path.to_string(),
            phase_label.to_string(),
            state.to_string(),
        ));
        Ok(())
    }

    fn generate_actor_name(
        &self,
        existing_actor_names: &[String],
    ) -> Result<String, SnapshotError> {
        // Iteratively pop until a non-colliding candidate is found. The
        // lock is released after each candidate — callers with recursive
        // retry semantics would deadlock on a held MutexGuard.
        loop {
            let candidate = {
                let mut queue = self.fixed_generated_names.lock().unwrap();
                queue.pop_front()
            };
            match candidate {
                None => {
                    return Err(SnapshotError::IoError {
                        message: "fixed_generated_names queue exhausted — test scenario must populate enough entries for the expected collision retries.".to_string(),
                    })
                }
                Some(name) => {
                    if existing_actor_names.iter().any(|n| n == &name) {
                        continue;
                    }
                    return Ok(name);
                }
            }
        }
    }
}
