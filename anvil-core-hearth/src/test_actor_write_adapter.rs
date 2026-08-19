//! In-memory `ActorWritePort` implementation for tests. Mirrors the
//! three-leg semantics of `FileSystemActorWriteAdapter` so feature
//! scenarios can drive the rule directly without filesystem I/O.

use anvil_core::domain::shared_types::ActorIdentity;
use anvil_core::ports::actor_write_port::{ActorWriteError, ActorWritePort};
use std::collections::HashMap;
use std::sync::Mutex;

/// Snapshot of one stored configuration entry. Mirrors the on-disk
/// shape (model + provider + the runtime detail fields) the adapter
/// would write so tests can assert on the exact stored data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConfig {
    pub at: String,
    pub model: String,
    pub provider: String,
    pub context_window: i64,
    pub sdk_version: String,
    pub entrypoint: String,
}

impl StoredConfig {
    fn from_identity(identity: &ActorIdentity) -> Self {
        Self {
            at: identity.registered_at.clone(),
            model: identity.model.clone(),
            provider: identity.provider.clone(),
            context_window: identity.context_window,
            sdk_version: identity.sdk_version.clone(),
            entrypoint: identity.entrypoint.clone(),
        }
    }

    /// Configuration equality for the match-no-op leg. Compares on the
    /// runtime params; `at` (timestamp) is intentionally excluded
    /// because the rule keys on whether the agent's identity changed,
    /// not when the entry was recorded.
    fn matches_runtime(&self, identity: &ActorIdentity) -> bool {
        self.model == identity.model
            && self.provider == identity.provider
            && self.context_window == identity.context_window
            && self.sdk_version == identity.sdk_version
            && self.entrypoint == identity.entrypoint
    }
}

#[derive(Debug, Clone)]
struct StoredActor {
    #[allow(dead_code)] // recorded for parity with the on-disk shape; not yet asserted on
    actor_type: String,
    configurations: Vec<StoredConfig>,
}

#[derive(Debug, Default)]
pub struct TestActorWriteAdapter {
    /// Outer key is artifact_path, inner key is actor_name.
    actors: Mutex<HashMap<String, HashMap<String, StoredActor>>>,
    pub upserts: Mutex<Vec<(String, ActorIdentity)>>,
}

impl TestActorWriteAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed an existing actor record at `artifact_path` so scenarios can
    /// drive the match-no-op and mismatch-append legs without first
    /// invoking upsert.
    pub fn with_existing_actor(self, artifact_path: &str, identity: &ActorIdentity) -> Self {
        {
            let mut map = self.actors.lock().unwrap();
            let by_actor = map.entry(artifact_path.to_string()).or_default();
            by_actor.insert(
                identity.name.clone(),
                StoredActor {
                    actor_type: identity.actor_type.clone(),
                    configurations: vec![StoredConfig::from_identity(identity)],
                },
            );
        }
        self
    }

    /// Number of stored configuration entries for a given actor at a
    /// given artifact path. Returns 0 when the actor or path is absent.
    pub fn configurations_count(&self, artifact_path: &str, actor_name: &str) -> usize {
        self.actors
            .lock()
            .unwrap()
            .get(artifact_path)
            .and_then(|by_actor| by_actor.get(actor_name))
            .map(|a| a.configurations.len())
            .unwrap_or(0)
    }

    /// Cloned snapshot of the stored configurations list for a given
    /// actor — empty when the actor or path is absent.
    pub fn configurations_for(&self, artifact_path: &str, actor_name: &str) -> Vec<StoredConfig> {
        self.actors
            .lock()
            .unwrap()
            .get(artifact_path)
            .and_then(|by_actor| by_actor.get(actor_name))
            .map(|a| a.configurations.clone())
            .unwrap_or_default()
    }
}

impl ActorWritePort for TestActorWriteAdapter {
    fn upsert_actor_configuration(
        &self,
        artifact_path: &str,
        identity: &ActorIdentity,
    ) -> Result<(), ActorWriteError> {
        self.upserts
            .lock()
            .unwrap()
            .push((artifact_path.to_string(), identity.clone()));

        let mut map = self.actors.lock().unwrap();
        let by_actor = map.entry(artifact_path.to_string()).or_default();
        match by_actor.get_mut(&identity.name) {
            None => {
                by_actor.insert(
                    identity.name.clone(),
                    StoredActor {
                        actor_type: identity.actor_type.clone(),
                        configurations: vec![StoredConfig::from_identity(identity)],
                    },
                );
            }
            Some(existing) => {
                let latest_matches = existing
                    .configurations
                    .last()
                    .map(|c| c.matches_runtime(identity))
                    .unwrap_or(false);
                if !latest_matches {
                    existing
                        .configurations
                        .push(StoredConfig::from_identity(identity));
                }
            }
        }
        Ok(())
    }
}
