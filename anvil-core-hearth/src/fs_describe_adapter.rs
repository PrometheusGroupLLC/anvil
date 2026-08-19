use anvil_core::domain::describe::{DescribeError, TransitionInfo};
use anvil_core::domain::playbook::registry::{PlaybookRegistry, PLAYBOOK_GENERATION_KIND};
use anvil_core::domain::status::FullStatusYaml;
use anvil_core::domain::ALL_ARTIFACT_TYPES;
use anvil_core::ports::describe_port::{DescribePort, InstanceState};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use anvil_core::ports::backlog_item_port::BacklogItemPort;

/// True only when `dir` is exactly one ordinary path segment — a single
/// `Component::Normal` and nothing else.
///
/// This is the trust boundary for registry-declared machine directories. A
/// machine's `directory:` is authored text (in the playbook_generation builder
/// flow it originates as raw LLM-generated `machine.yaml`), so a manipulated or
/// buggy declaration could carry `..`, an absolute path, or an embedded
/// separator. Any of those, once joined in `read_instance`
/// (`hearth.join(directory).join(id)`), escapes the hearth and turns `describe`
/// into an arbitrary-file-read primitive (`PathBuf::join` with an absolute
/// operand REPLACES the base entirely). Rejecting anything that is not a lone
/// `Normal` component closes that: it excludes `""`, `"."`, `".."`, absolute
/// paths (`RootDir`/`Prefix`), and any multi-segment or separator-bearing value.
/// The trailing equality guard rejects normalized-away forms (e.g. `"secret/"`,
/// whose sole component is `"secret"`).
fn is_safe_scan_directory(dir: &str) -> bool {
    let mut components = Path::new(dir).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(only)), None) => only == OsStr::new(dir),
        _ => false,
    }
}

/// One artifact directory to scan, paired with the kind to report when an
/// instance's `status.yaml` omits `kind:` (the stateless-legacy fallback).
#[derive(Debug, Clone)]
struct ScanDir {
    directory: String,
    fallback_kind: String,
}

/// The directory set every `FileSystemDescribeAdapter` scans by default: each
/// `ALL_ARTIFACT_TYPES` directory, PLUS BOTH playbook-generation storage
/// contract dirs — canonical `playbook_generations` and legacy
/// `workflow_generations` — under kind `playbook_generation`.
///
/// `playbook_generations` is deliberately NOT one of `ALL_ARTIFACT_TYPES` — it
/// is the fixed on-disk directory the playbook-generation meta-playbook writes
/// to (see `registry::PLAYBOOK_GENERATION_KIND`, whose storage contract is
/// intentionally unchanged by the rename). Including it here mirrors the
/// snapshot adapter's `ARTIFACT_DIRS`. Without it, `describe(<generation-id>)`
/// returns `UnknownIdentifier` for every `playbook_generation` instance, which
/// breaks any consumer verifying a generated-playbook registration.
fn base_scan_dirs() -> Vec<ScanDir> {
    let mut dirs: Vec<ScanDir> = ALL_ARTIFACT_TYPES
        .iter()
        .map(|t| ScanDir {
            directory: t.directory_name().to_string(),
            fallback_kind: t.as_str().to_string(),
        })
        .collect();
    // C-d.1 DUAL READ: canonical first, legacy second. Both roots are scanned so
    // a generation written to either is describable; neither is written here.
    for directory in [
        anvil_core::domain::playbook::registry_projection::CANONICAL_GENERATIONS_DIR,
        anvil_core::domain::playbook::registry_projection::LEGACY_GENERATIONS_DIR,
    ] {
        dirs.push(ScanDir {
            directory: directory.to_string(),
            fallback_kind: PLAYBOOK_GENERATION_KIND.to_string(),
        });
    }
    dirs
}

/// Filesystem implementation of DescribePort.
#[derive(Debug, Clone)]
pub struct FileSystemDescribeAdapter {
    hearth_path: PathBuf,
    scan_dirs: Vec<ScanDir>,
}

impl FileSystemDescribeAdapter {
    /// Construct with the core hearth directory set (every `ALL_ARTIFACT_TYPES`
    /// directory plus the legacy `workflow_generations` dir). Sufficient for
    /// every compiled-in kind; registry-declared machine directories are added
    /// via [`FileSystemDescribeAdapter::with_registry`].
    pub fn new(hearth_path: PathBuf) -> Self {
        Self {
            hearth_path,
            scan_dirs: base_scan_dirs(),
        }
    }

    /// Construct scanning the core set UNIONED with every directory declared by
    /// a registered machine (`PlaybookMachine::directory`).
    ///
    /// This is what makes `describe` resolve ANY registry-resolvable kind —
    /// `playbook_generation` today, and future machine kinds (e.g.
    /// `knowledge_lifecycle`) without another hardcode. Dedup is by directory
    /// name: the base entries win for the core dirs, and a machine's own `kind`
    /// is the fallback kind for any additional directory it declares.
    pub fn with_registry(hearth_path: PathBuf, registry: &dyn PlaybookRegistry) -> Self {
        let mut scan_dirs = base_scan_dirs();
        for machine in registry.all_machines() {
            if machine.directory.is_empty() {
                continue;
            }
            // Trust boundary: a registry-declared `directory:` is untrusted
            // (LLM-authored in the playbook_generation flow). Never scan a value
            // that could escape the hearth — a `..`, absolute path, or embedded
            // separator would make `read_instance` join a path outside the
            // hearth (arbitrary-file-read). Skip it loudly instead.
            if !is_safe_scan_directory(&machine.directory) {
                eprintln!(
                    "[fs_describe_adapter] WARNING: ignoring unsafe scan directory \
                     '{}' declared by machine kind '{}' (not a single path \
                     component); describe will not scan it",
                    machine.directory, machine.kind
                );
                continue;
            }
            if scan_dirs.iter().any(|d| d.directory == machine.directory) {
                continue;
            }
            scan_dirs.push(ScanDir {
                directory: machine.directory.clone(),
                fallback_kind: machine.kind.clone(),
            });
        }
        Self {
            hearth_path,
            scan_dirs,
        }
    }

    /// Read the instance rooted at `dir`, or `None` when no `status.yaml` is
    /// present there (so the caller keeps scanning). `fallback_kind` supplies
    /// `kind` when the status file omits it (stateless-legacy fallback).
    fn read_instance_at(
        dir: &Path,
        fallback_kind: &str,
    ) -> Option<Result<InstanceState, DescribeError>> {
        let status_path = dir.join("status.yaml");
        if !status_path.exists() {
            return None;
        }

        let content = match std::fs::read_to_string(&status_path) {
            Ok(content) => content,
            Err(e) => {
                return Some(Err(DescribeError::IoError {
                    message: format!("Failed to read {}: {}", status_path.display(), e),
                }))
            }
        };

        let status: FullStatusYaml = match serde_yaml::from_str(&content) {
            Ok(status) => status,
            Err(e) => {
                return Some(Err(DescribeError::IoError {
                    message: format!("Invalid YAML in {}: {}", status_path.display(), e),
                }))
            }
        };

        // Resolve via the shared accessor (top-level state, else last
        // transition's `to`). last_transition below remains a separate
        // field, unchanged.
        // C-d.1 round 8, H-1 (forced touch). Both seams are fallible now.
        // `describe` is the surface an operator reads to decide what to do
        // next, so unreadable evidence refuses rather than describing an
        // artifact by a stale state.
        let state = match anvil_core::domain::transition_log::resolve_state_with_events(&status, dir) {
            Ok(s) => s.unwrap_or_default(),
            Err(e) => {
                return Some(Err(DescribeError::IoError {
                    message: format!(
                        "artifact_state_uninspectable: the transition events under {} could not \
                         be read: {e}",
                        dir.display()
                    ),
                }))
            }
        };
        let transitions =
            match anvil_core::domain::transition_log::resolve_transitions_with_events(&status, dir) {
                Ok(t) => t,
                Err(e) => {
                    return Some(Err(DescribeError::IoError {
                        message: format!(
                            "transitions_uninspectable: the transition events under {} could not \
                             be read: {e}",
                            dir.display()
                        ),
                    }))
                }
            };
        let kind = status
            .kind
            .clone()
            .unwrap_or_else(|| fallback_kind.to_string());
        let transition_count = transitions.len();

        let last_transition = transitions.last().map(|t| TransitionInfo {
            to: t.to.clone(),
            at: t.at.clone().unwrap_or_default(),
            actor: t.actor.clone().unwrap_or_default(),
            role: t.role.clone().unwrap_or_default(),
        });

        Some(Ok(InstanceState {
            kind,
            state,
            transition_count,
            last_transition,
        }))
    }
}

impl DescribePort for FileSystemDescribeAdapter {
    fn read_instance(&self, artifact_id: &str) -> Result<InstanceState, DescribeError> {
        // First-read K8 recovery (plan Task 4): Describe may be the FIRST
        // caller after an interrupted backlog transaction.
        crate::fs_backlog_item_adapter::FileSystemBacklogItemAdapter::new(
            self.hearth_path.clone(),
        )
        .recover()
        .map_err(|e| DescribeError::IoError {
            message: format!("backlog recovery before instance read: {e}"),
        })?;
        // Scan every configured artifact directory for the matching id.
        for scan in &self.scan_dirs {
            let dir = self.hearth_path.join(&scan.directory).join(artifact_id);
            if let Some(result) = Self::read_instance_at(&dir, &scan.fallback_kind) {
                return result;
            }
        }

        Err(DescribeError::UnknownIdentifier {
            identifier: artifact_id.to_string(),
        })
    }
}
