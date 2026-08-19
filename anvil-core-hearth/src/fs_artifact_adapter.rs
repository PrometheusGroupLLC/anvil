use anvil_core::ports::artifact_port::{ArtifactError, ArtifactPort};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Filesystem implementation of `ArtifactPort`.
///
/// `create_review_doc` is lifted verbatim from
/// `fs_begin_adapter.rs:383-396`. The original is NOT deleted here —
/// Phase 5 removes it once all consumers have migrated.
#[derive(Clone)]
pub struct FileSystemArtifactAdapter {
    hearth_path: PathBuf,
}

impl FileSystemArtifactAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

impl ArtifactPort for FileSystemArtifactAdapter {
    fn create_review_doc(
        &self,
        track_path: &str,
        doc_name: &str,
        header: &str,
    ) -> Result<String, ArtifactError> {
        let full_path = self.hearth_path.join(track_path).join(doc_name);
        if !full_path.exists() {
            std::fs::write(&full_path, header).map_err(|e| ArtifactError::IoError {
                message: format!(
                    "Failed to write review doc '{}': {}",
                    full_path.display(),
                    e
                ),
            })?;
        }
        Ok(full_path.to_string_lossy().into_owned())
    }

    fn scaffold_track_directory(
        &self,
        track_name: &str,
        _parent_id: &str,
        display_name: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError> {
        // Delegate to the kind-agnostic scaffold, preserving the track's
        // placeholder spec.md. (The "track directory already exists" idempotency
        // message is preserved by the generic scaffold's per-directory message.)
        let spec_contents = format!("# {}\n", display_name);
        self.scaffold_artifact_directory(
            "tracks",
            track_name,
            status_yaml,
            &[("spec.md", &spec_contents)],
        )
    }

    fn scaffold_artifact_directory(
        &self,
        directory: &str,
        name: &str,
        status_yaml: &str,
        scaffold_files: &[(&str, &str)],
    ) -> Result<String, ArtifactError> {
        // Derive the directory name from the current timestamp plus a
        // snake-cased version of name — mirrors the behavior of the retired
        // `FileSystemBeginAdapter::create_track_directory`.
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M");
        let snake_name = name.to_lowercase().replace([' ', '-'], "_");
        let (parent_directory, base_dir_name) = if directory.is_empty() {
            let kind = status_kind(status_yaml)?;
            ("runs", format!("{}_{}_{}", timestamp, kind, snake_name))
        } else {
            (directory, format!("{}_{}", timestamp, snake_name))
        };
        let (rel_path, full_path) =
            reserve_unique_directory(&self.hearth_path, parent_directory, &base_dir_name)?;
        crate::atomic_write::atomic_write(
            &full_path.join("status.yaml"),
            status_yaml.as_bytes(),
        )
        .map_err(|e| ArtifactError::IoError {
            message: format!("Failed to write status.yaml: {}", e),
        })?;
        // Optional scaffold files — for the track encoding this is spec.md,
        // which satisfies `build_registry_entry_text`'s `extract_h1` read in the
        // subsequent snapshot call. Decision uses this to create its lifecycle
        // documents before the first snapshot registry entry is built.
        for (filename, contents) in scaffold_files {
            std::fs::write(full_path.join(filename), contents).map_err(|e| {
                ArtifactError::IoError {
                    message: format!("Failed to write {}: {}", filename, e),
                }
            })?;
        }
        Ok(rel_path)
    }

    fn persist_generated_playbook(
        &self,
        owner_home: &str,
        kind: &str,
        machine_yaml: &str,
        hooks: Option<&[(String, String)]>,
        exemplars: Option<&[anvil_core::domain::playbook::candidate::GeneratedExemplarFile]>,
    ) -> Result<String, ArtifactError> {
        // Writes under the explicit `owner_home` ARG, NOT `self.hearth_path`
        // (the field is vestigial for this op by contract). The registry resolves
        // `<owner_home>/playbooks/<kind>/machine.yaml`, so the kind dir lives
        // directly under `playbooks/` (NOT `forge/playbooks/`). The registry
        // still reads legacy `<owner_home>/workflows/...` for compatibility.
        let kind_dir = PathBuf::from(owner_home).join("playbooks").join(kind);
        std::fs::create_dir_all(&kind_dir).map_err(|e| ArtifactError::IoError {
            message: format!(
                "Failed to create playbook dir '{}': {}",
                kind_dir.display(),
                e
            ),
        })?;
        let machine_path = kind_dir.join("machine.yaml");
        persist_machine_yaml_exclusive(&machine_path, kind, machine_yaml)?;
        // Optional hooks/ — only created when supplied (machine.yaml is the
        // minimum). NO status.yaml is ever written.
        if let Some(entries) = hooks {
            if !entries.is_empty() {
                let hooks_dir = kind_dir.join("hooks");
                std::fs::create_dir_all(&hooks_dir).map_err(|e| ArtifactError::IoError {
                    message: format!(
                        "Failed to create hooks dir '{}': {}",
                        hooks_dir.display(),
                        e
                    ),
                })?;
                for (filename, contents) in entries {
                    crate::atomic_write::atomic_write(
                        &hooks_dir.join(filename),
                        contents.as_bytes(),
                    )
                    .map_err(|e| ArtifactError::IoError {
                        message: format!("Failed to write hook '{}': {}", filename, e),
                    })?;
                }
            }
        }
        if let Some(entries) = exemplars {
            if !entries.is_empty() {
                let exemplars_dir = kind_dir.join("exemplars");
                std::fs::create_dir_all(&exemplars_dir).map_err(|e| ArtifactError::IoError {
                    message: format!(
                        "Failed to create exemplars dir '{}': {}",
                        exemplars_dir.display(),
                        e
                    ),
                })?;
                for exemplar in entries {
                    let filename = safe_exemplar_filename(&exemplar.id)?;
                    crate::atomic_write::atomic_write(
                        &exemplars_dir.join(filename),
                        exemplar.markdown.as_bytes(),
                    )
                    .map_err(|e| ArtifactError::IoError {
                        message: format!("Failed to write exemplar '{}': {}", exemplar.id, e),
                    })?;
                }
            }
        }
        Ok(machine_path.to_string_lossy().into_owned())
    }

    fn scaffold_playbook_directory(
        &self,
        playbook_name: &str,
        _parent_id: &str,
        status_yaml: &str,
    ) -> Result<String, ArtifactError> {
        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M");
        let snake_name = playbook_name.to_lowercase().replace([' ', '-'], "_");
        let base_dir_name = format!("{}_{}", timestamp, snake_name);
        let (rel_path, full_path) =
            reserve_unique_directory(&self.hearth_path, "playbooks", &base_dir_name)?;
        crate::atomic_write::atomic_write(
            &full_path.join("status.yaml"),
            status_yaml.as_bytes(),
        )
        .map_err(|e| ArtifactError::IoError {
            message: format!("Failed to write status.yaml: {}", e),
        })?;
        // Placeholder definition.md — satisfies `build_registry_entry_text`'s
        // `extract_h1` read in the subsequent snapshot call.
        let def_contents = format!("# {}\n", playbook_name);
        std::fs::write(full_path.join("definition.md"), def_contents).map_err(|e| {
            ArtifactError::IoError {
                message: format!("Failed to write definition.md: {}", e),
            }
        })?;
        Ok(rel_path)
    }
}

fn safe_exemplar_filename(id: &str) -> Result<String, ArtifactError> {
    let invalid = id.trim().is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id == "."
        || id == ".."
        || id.contains("..");
    if invalid {
        return Err(ArtifactError::IoError {
            message: format!("Invalid exemplar id '{}'", id),
        });
    }
    Ok(format!("{}.md", id))
}

fn reserve_unique_directory(
    hearth_path: &Path,
    directory: &str,
    base_dir_name: &str,
) -> Result<(String, PathBuf), ArtifactError> {
    let parent = hearth_path.join(directory);
    std::fs::create_dir_all(&parent).map_err(|e| ArtifactError::IoError {
        message: format!("Failed to create directory '{}': {}", parent.display(), e),
    })?;

    for suffix in 0..10_000 {
        let dir_name = if suffix == 0 {
            base_dir_name.to_string()
        } else {
            format!("{}-{}", base_dir_name, suffix + 1)
        };
        let full_path = parent.join(&dir_name);
        match std::fs::create_dir(&full_path) {
            Ok(()) => {
                let rel_path = if directory.is_empty() {
                    dir_name
                } else {
                    format!("{}/{}", directory, dir_name)
                };
                return Ok((rel_path, full_path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(ArtifactError::IoError {
                    message: format!(
                        "Failed to create directory '{}': {}",
                        full_path.display(),
                        e
                    ),
                });
            }
        }
    }

    Err(ArtifactError::IoError {
        message: format!(
            "Failed to find available directory name under '{}' for '{}'",
            parent.display(),
            base_dir_name
        ),
    })
}

fn status_kind(status_yaml: &str) -> Result<String, ArtifactError> {
    let value: serde_yaml::Value =
        serde_yaml::from_str(status_yaml).map_err(|e| ArtifactError::IoError {
            message: format!("Failed to parse status.yaml for run kind: {}", e),
        })?;
    value
        .get("kind")
        .and_then(|v| v.as_str())
        .filter(|kind| !kind.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| ArtifactError::IoError {
            message: "Failed to parse status.yaml for run kind: missing kind".to_string(),
        })
}

fn persist_machine_yaml_exclusive(
    machine_path: &Path,
    kind: &str,
    machine_yaml: &str,
) -> Result<(), ArtifactError> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(machine_path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(machine_yaml.as_bytes()) {
                let _ = std::fs::remove_file(machine_path);
                return Err(ArtifactError::IoError {
                    message: format!("Failed to write machine.yaml: {}", e),
                });
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            compare_existing_machine(machine_path, kind, machine_yaml.as_bytes())
        }
        Err(e) => Err(ArtifactError::IoError {
            message: format!("Failed to create machine.yaml exclusively: {}", e),
        }),
    }
}

fn compare_existing_machine(
    machine_path: &Path,
    kind: &str,
    desired_bytes: &[u8],
) -> Result<(), ArtifactError> {
    let path_text = machine_path.display().to_string();
    let existing_bytes =
        std::fs::read(machine_path).map_err(|e| ArtifactError::PlaybookExistingMachineInvalid {
            kind: kind.to_string(),
            path: path_text.clone(),
            message: e.to_string(),
        })?;

    if existing_bytes == desired_bytes {
        return Ok(());
    }

    let existing_yaml = std::str::from_utf8(&existing_bytes).map_err(|e| {
        ArtifactError::PlaybookExistingMachineInvalid {
            kind: kind.to_string(),
            path: path_text.clone(),
            message: e.to_string(),
        }
    })?;
    // Validate the existing target against the hook filenames actually present in
    // its sibling `hooks/` dir — NOT an empty slice. A previously-persisted
    // hook-bearing machine references its own hook files, so reloading it with no
    // hook filenames mis-reports a genuine same-kind duplicate as
    // `PlaybookExistingMachineInvalid` (unknown-hook). This is the authoritative
    // exclusive-create race path; it must use the SAME canonical hook listing as
    // the persist preflight and the registry so all three agree.
    // Validate the existing target against the hook filenames actually present in
    // its sibling `hooks/` dir — NOT an empty slice. A previously-persisted
    // hook-bearing machine references its own hook files, so reloading it with no
    // hook filenames mis-reports a genuine same-kind duplicate as
    // `PlaybookExistingMachineInvalid` (unknown-hook). This is the authoritative
    // exclusive-create race path; it must use the SAME canonical hook listing as
    // the persist preflight and the registry so all three agree.
    let existing_hooks_dir = machine_path
        .parent()
        .map(|parent| parent.join("hooks"))
        .unwrap_or_else(|| Path::new("hooks").to_path_buf());
    // C-d.1 round 6, M-1. Was `loader::list_hook_filenames`, which swallowed a
    // read failure into an empty listing — so an unreadable `hooks/` beside the
    // existing machine reported that machine invalid for referencing hooks it
    // correctly references. The read failure now names itself.
    let existing_hook_filenames =
        anvil_core::domain::playbook::fs_probe::list_hook_files(&existing_hooks_dir).map_err(|e| {
            ArtifactError::PlaybookExistingMachineInvalid {
                kind: kind.to_string(),
                path: path_text.clone(),
                message: format!(
                    "the existing machine's hooks directory {} could not be listed: {e}. An \
                     unreadable hook directory is not an empty one.",
                    existing_hooks_dir.display()
                ),
            }
        })?;
    anvil_core::domain::playbook::loader::load_from_yaml(kind, existing_yaml, &existing_hook_filenames)
        .map_err(|e| ArtifactError::PlaybookExistingMachineInvalid {
            kind: kind.to_string(),
            path: path_text.clone(),
            message: e.to_string(),
        })?;

    Err(ArtifactError::PlaybookDuplicateKind {
        kind: kind.to_string(),
        path: path_text,
    })
}
