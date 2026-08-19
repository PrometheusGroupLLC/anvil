//! Hearth-backed [`OwnerResolver`]: derives a playbook's owner from the
//! `contributed_by` field in its on-disk status.yaml.
//!
//! `contributed_by` is injected into a playbook's status.yaml at kit-install
//! time — it lives in status.yaml, NOT machine.yaml (the registry only reads
//! machine.yaml, so `PlaybookMachine.owner_kit` is empty for hearth-loaded
//! machines). This adapter resolves the kind to its on-disk `playbook_id`
//! (directory name under `<hearth>/playbooks/`) via a registry, then reads that
//! directory's status.yaml for the `contributed_by` line. The legacy
//! `<hearth>/workflows/` root is still checked when the canonical one has no
//! entry for the kind — see the dual read in `owner_for`.
//!
//! Missing status.yaml, missing field, or empty value → `None`; the domain
//! query then applies the "anvil" default.

use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::status::FullStatusYaml;
use anvil_core::domain::artifact_activity::OwnerResolver;
use std::path::{Path, PathBuf};

pub struct FileSystemOwnerResolver<'a> {
    hearth_path: PathBuf,
    registry: &'a dyn PlaybookRegistry,
}

impl<'a> FileSystemOwnerResolver<'a> {
    pub fn new(hearth_path: &Path, registry: &'a dyn PlaybookRegistry) -> Self {
        Self {
            hearth_path: hearth_path.to_path_buf(),
            registry,
        }
    }
}

impl<'a> OwnerResolver for FileSystemOwnerResolver<'a> {
    fn owner_for(&self, kind: &str) -> Option<String> {
        let playbook_id = self.registry.playbook_id_for(kind)?;
        let status_path = self
            .hearth_path
            .join("playbooks")
            .join(&playbook_id)
            .join("status.yaml");
        let legacy_status_path = self
            .hearth_path
            .join("workflows")
            .join(&playbook_id)
            .join("status.yaml");
        let status_path = if status_path.exists() {
            status_path
        } else {
            legacy_status_path
        };
        let contents = std::fs::read_to_string(&status_path).ok()?;
        let parsed: FullStatusYaml = serde_yaml::from_str(&contents).ok()?;
        parsed.contributed_by.filter(|s| !s.trim().is_empty())
    }
}
