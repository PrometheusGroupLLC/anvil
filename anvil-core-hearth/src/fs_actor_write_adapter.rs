//! Filesystem implementation of `ActorWritePort`. Owns the YAML
//! actor-block manipulation that previously lived in
//! `fs_begin_adapter::seed_actor` and `fs_snapshot_adapter::seed_actor`,
//! extended to cover all three legs of the uniform actor-write rule.
//!
//! The reads use serde_yaml to interrogate the actors table; the writes
//! are line-based to preserve the surrounding YAML's exact formatting
//! (comments, key order, indentation idioms).

use anvil_core::domain::actor_configuration::{render_upserted_actor_configuration, MalformedPolicy};
use anvil_core::domain::shared_types::ActorIdentity;
use anvil_core::ports::actor_write_port::{ActorWriteError, ActorWritePort};
use std::path::PathBuf;

pub struct FileSystemActorWriteAdapter {
    hearth_path: PathBuf,
}

impl FileSystemActorWriteAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl ActorWritePort for FileSystemActorWriteAdapter {
    fn upsert_actor_configuration(
        &self,
        artifact_path: &str,
        identity: &ActorIdentity,
    ) -> Result<(), ActorWriteError> {
        let status_path = self.hearth_path.join(artifact_path).join("status.yaml");
        if !status_path.exists() {
            return Err(ActorWriteError::NotFound {
                artifact_path: artifact_path.to_string(),
            });
        }
        let content =
            std::fs::read_to_string(&status_path).map_err(|e| ActorWriteError::IoError {
                message: format!("Failed to read status.yaml: {}", e),
            })?;

        let new_content =
            match render_upserted_actor_configuration(&content, identity, MalformedPolicy::Lenient)
                .map_err(|message| ActorWriteError::MalformedStatus {
                    artifact_path: artifact_path.to_string(),
                    message,
                })? {
                Some(rendered) => rendered,
                None => return Ok(()),
            };

        crate::atomic_write::atomic_write(&status_path, new_content.as_bytes()).map_err(
            |e| ActorWriteError::IoError {
                message: format!("Failed to write status.yaml: {}", e),
            },
        )
    }
}
