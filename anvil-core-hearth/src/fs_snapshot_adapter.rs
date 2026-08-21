//! Filesystem implementation of `SnapshotPort`. Writes status.yaml,
//! registry files, and projection files for the engine's snapshot
//! command.
//!
//! The structural logic for append_transition, seed_actor, and the
//! common projection/registry helpers mirrors `fs_begin_adapter.rs` —
//! intentionally duplicated per the deterministic-snapshot-engine plan,
//! to be consolidated by the Core CQRS Separation track. Error
//! construction switches to `SnapshotError` at each failure site.

use anvil_core::domain::playbook::fs_probe::{self, NodeKind};
use anvil_core::domain::backlog_item::{
    self as bi, BacklogItem, BacklogPolicy, BacklogTransitionContext, State,
};
use anvil_core::domain::playbook::registry::PlaybookRegistry;
use anvil_core::domain::content_hash::content_hash;
use crate::fs_backlog_item_adapter::FileSystemBacklogItemAdapter;
use anvil_core::ports::backlog_item_port::{
    BacklogItemPort, BacklogStoreError, LoadedBacklogItem, PreparedBacklogCommit,
};
use anvil_core::domain::shared_types::{ActivityEntry, ActorIdentity, TransitionContent};
use anvil_core::domain::snapshot::SnapshotError;
use anvil_core::domain::status::{FullStatusYaml, StatusTransition};
use crate::fs_actor_write_adapter::FileSystemActorWriteAdapter;
use crate::fs_transition_event_adapter::FileSystemTransitionEventAdapter;
use anvil_core::ports::actor_write_port::{ActorWriteError, ActorWritePort};
use anvil_core::ports::snapshot_port::SnapshotPort;
use anvil_core::ports::transition_event_write_port::{
    TransitionEventWriteError, TransitionEventWritePort, TransitionRecord,
};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct FileSystemSnapshotAdapter {
    hearth_path: PathBuf,
}

impl FileSystemSnapshotAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }

    /// The strict K8 store for this hearth. Every backlog read/write on this
    /// adapter delegates here; there is no lenient K8 path.
    pub fn backlog_store(&self) -> FileSystemBacklogItemAdapter {
        FileSystemBacklogItemAdapter::new(self.hearth_path.clone())
    }

    /// Parse the artifact's full status.yaml. Shared by the
    /// `read_activity_entries` / `read_transitions` begin-adoption reads.
    fn read_full_status(&self, artifact_path: &str) -> Result<FullStatusYaml, SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let status_path = dir.join("status.yaml");
        // C-d.1 round 7, H-1, found by the matrix's per-ENTRY rows. This mapped
        // a READ failure onto `MalformedStatus` — so an unreadable status.yaml
        // was reported as a status.yaml that does not parse, which sends an
        // operator to fix a file that is perfectly well formed. Same class, a
        // different collapse: not "absent", but "corrupt". A parse failure below
        // is still `MalformedStatus`, because that one is true.
        let content = std::fs::read_to_string(&status_path).map_err(|e| SnapshotError::IoError {
            message: format!(
                "artifact_status_uninspectable: {} exists and could not be read: {e}. This is not \
                 a malformed status.yaml — it is one this process could not open.",
                status_path.display()
            ),
        })?;
        serde_yaml::from_str(&content).map_err(|e| SnapshotError::MalformedStatus {
            artifact_path: artifact_path.to_string(),
            message: format!("Invalid YAML: {}", e),
        })
    }

    /// Shared writer for the per-artifact projection files (`authoring.md`
    /// for playbook_generation, `projection.md` for generated kinds). Both
    /// fold the transition into a "Current …" header plus an append-only
    /// `## History` list.
    #[allow(clippy::too_many_arguments)]
    fn write_projection_file(
        &self,
        artifact_path: &str,
        file_name: &str,
        phase_label: &str,
        state: &str,
        at: &str,
        actor: &str,
        role: &str,
    ) -> Result<(), SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let path = dir.join(file_name);
        let existing = read_to_string_or_absent(&path)?;
        let mut history: Vec<String> = existing
            .lines()
            .filter(|line| line.starts_with("- "))
            .map(|line| line.to_string())
            .collect();
        history.push(format!(
            "- {} — {} ({}) by {}",
            at, phase_label, state, actor
        ));
        let body = format!(
            "# Authoring Projection\n\nCurrent phase: {}\nCurrent state: {}\nLast actor: {}\nLast role: {}\n\n## History\n{}\n",
            phase_label,
            state,
            actor,
            role,
            history.join("\n")
        );
        crate::atomic_write::atomic_write(&path, body.as_bytes()).map_err(|e| {
            SnapshotError::IoError {
                message: format!("Failed to write {}: {}", path.display(), e),
            }
        })
    }

    /// Extract the display name (first link text) from a registry entry
    /// whose first-link href contains the given artifact id. Returns
    /// None if no entry found or the registry file is missing.
    ///
    /// **C-d.1 round 7, H-1.** Was `read_to_string(&path).ok()?`, and both call
    /// sites answer the resulting `None` with `.unwrap_or_else(|| id)` — so an
    /// unreadable registry silently swapped the human display name for the
    /// directory slug, and the projection row move that follows then matched on
    /// a name that is not in the table. A no-op move over a readable projection
    /// is a snapshot that reports success and leaves the row where it was.
    fn resolve_display_name(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<Option<String>, SnapshotError> {
        let path = self.hearth_path.join(registry_file);
        let content = read_to_string_or_absent(&path)?;
        let id_slash = format!("/{}/", artifact_id);
        let id_paren = format!("/{})", artifact_id);
        for line in content.lines() {
            if !line.contains(&id_slash) && !line.contains(&id_paren) {
                continue;
            }
            // Extract first [text](...).
            let Some(open) = line.find('[') else {
                return Ok(None);
            };
            let after = &line[open + 1..];
            let Some(close) = after.find(']') else {
                return Ok(None);
            };
            let text = &after[..close];
            let after_text = &after[close + 1..];
            if !after_text.starts_with('(') {
                continue;
            }
            let href = &after_text[1..];
            let Some(paren_close) = href.find(')') else {
                return Ok(None);
            };
            let href = &href[..paren_close];
            if href.contains(&id_slash) || href.contains(&id_paren) {
                return Ok(Some(text.to_string()));
            }
        }
        Ok(None)
    }
}

use anvil_core::domain::playbook::registry_projection;

// === Artifact-kind discovery ===============================================

const ARTIFACT_DIRS: &[(&str, &str)] = &[
    ("track", "tracks"),
    ("proposal", "proposals"),
    ("milestone", "milestones"),
    ("initiative", "initiatives"),
    ("decision", "decisions"),
    ("learning", "learnings"),
    // C-d.1 DUAL READ. Both generation kind names resolve under BOTH the
    // canonical `playbook_generations/` and the legacy `workflow_generations/`
    // roots, canonical first. Reads span both; nothing here writes, renames or
    // migrates — the dual read ships BEFORE any migration of the legacy
    // directory, which is the ordering the task requires: nothing is moved out
    // from under a reader. The names come from `registry_projection` so there is
    // ONE owner of the two strings.
    ("playbook_generation", registry_projection::CANONICAL_GENERATIONS_DIR),
    ("workflow_generation", registry_projection::CANONICAL_GENERATIONS_DIR),
    ("playbook_generation", registry_projection::LEGACY_GENERATIONS_DIR),
    ("workflow_generation", registry_projection::LEGACY_GENERATIONS_DIR),
    // K8 backlog items resolve by bare `bi_` id the same way.
    ("backlog_item", "backlog_items"),
];

/// Locate the directory for an artifact regardless of its kind. The
/// artifact_path can be either a full relative path ("tracks/foo") or
/// a bare artifact id ("foo") — both resolve.
/// Public seam onto [`locate_artifact_dir`] for the C-d.1 generations dual-read
/// scenarios.
///
/// The lookup itself is private and stays private; this exposes it so a feature
/// can assert against the PRODUCTION resolution path instead of re-implementing
/// it. Without this, `resolve_generations` is proven only where a step
/// definition calls it directly — which is exactly the "no production caller"
/// gap an independent review found: scenarios proved a library worked and did
/// not prove the system did.
pub fn locate_artifact_dir_for_test(
    hearth: &Path,
    artifact_path: &str,
) -> Result<Option<PathBuf>, SnapshotError> {
    locate_artifact_dir(&hearth.to_path_buf(), artifact_path)
}

fn locate_artifact_dir(
    hearth: &PathBuf,
    artifact_path: &str,
) -> Result<Option<PathBuf>, SnapshotError> {
    // Engine-boundary containment (defense in depth): refuse any path that
    // escapes the hearth subtree BEFORE joining it, so an `..` traversal or an
    // in-hearth symlink-out-of-hearth cannot resolve a directory outside the
    // hearth — even if the shim's normalization was bypassed.
    if crate::containment::escapes_hearth(hearth, artifact_path) {
        return Ok(None);
    }
    // 1) Treat as a literal relative path first.
    //
    // C-d.1 round 6, M-1. This was `literal.exists()`, and it BRACKETED the
    // `resolve_generation_identity` round 5 made fallible — the swallow was
    // removed from the call in the middle and left standing on both sides of
    // it. `exists()` maps EACCES/ESTALE/EIO onto `false`, and every one of this
    // function's call sites turns the resulting `None` into
    // `SnapshotError::NotFound`, which the engine maps to gRPC NOT_FOUND. An
    // artifact directory that exists and cannot be inspected was reported to the
    // operator as an artifact that is not there.
    let literal = hearth.join(artifact_path);
    match uninspectable_or_kind(&literal)? {
        NodeKind::Absent => {}
        _ => return Ok(Some(literal)),
    }
    // 2) Generations get the C-d.1 dual read, which REFUSES a same-identity
    //    collision instead of returning whichever root `ARTIFACT_DIRS` happens
    //    to list first. A silent first-match pick across two roots is the exact
    //    shape this task exists to remove: it resolves, it looks fine, and half
    //    the history is invisible.
    //
    //    SCOPED TO THE IDENTITY BEING ASKED FOR. The first wiring called
    //    `resolve_generations`, which answers a question about the two
    //    DIRECTORIES and fails whole — so ONE colliding generation identity made
    //    every bare-id lookup in the hearth return `None`: tracks, proposals,
    //    decisions, sparks, everything not addressed by full relative path. That
    //    turned a local refusal into a hearth-wide outage, and the state that
    //    triggers it is the one this track's own cutover creates (new
    //    generations under `playbook_generations/` while `workflow_generations/`
    //    still holds the legacy set). A collision degrades the colliding id and
    //    nothing else.
    match registry_projection::resolve_generation_identity(hearth, artifact_path) {
        Ok(Some(found)) => return Ok(Some(found)),
        // Not a generation. Keep looking.
        Ok(None) => {}
        Err(e) => {
            // Loud, and NOT a fallback: refusing to answer is the point. A
            // caller that gets a refusal here cannot mistake it for "resolved" —
            // and it gets one for THIS identity only.
            return Err(SnapshotError::IoError {
                message: e.to_string(),
            });
        }
    }
    // 3) Fall back to searching the remaining per-kind directories by id.
    //
    // C-d.1 round 6, M-1. Was `candidate.exists()`. Same swallow as (1), and
    // worse here: this loop runs over NINE directories, so one uninspectable
    // per-kind root silently demoted itself to "the artifact is not under this
    // kind" and the search moved on as though it had looked.
    for (_, dir) in ARTIFACT_DIRS {
        let candidate = hearth.join(dir).join(artifact_path);
        match uninspectable_or_kind(&candidate)? {
            NodeKind::Absent => {}
            _ => return Ok(Some(candidate)),
        }
    }
    Ok(None)
}

/// [`fs_probe::node_kind`] with its `io::Error` mapped onto the snapshot
/// channel's "I could not look" answer.
///
/// `IoError` and not `NotFound`: the engine maps `NotFound` to gRPC NOT_FOUND —
/// *this artifact is not in this hearth* — and `IoError` to INTERNAL. They are
/// different facts with different operator actions, and collapsing the second
/// into the first is what told an operator that an artifact on disk was absent.
/// Read a file whose ABSENCE is a real answer, and whose unreadability is not.
///
/// **C-d.1 round 7, H-1 — the site class the behavioural matrix surfaced that no
/// module list did.** Seven reads in this adapter were
/// `std::fs::read_to_string(p).unwrap_or_default()`: the registry a projection
/// is rebuilt FROM, and the projection whose preserved narrative lines are
/// carried forward. Every one of them then WRITES. `unwrap_or_default()` on the
/// source means an unreadable registry rebuilds the projection as though the
/// hearth held nothing and the empty result is written over the real one — the
/// class's own consequence (an unreadable input read as an empty one, ending in
/// a permitted write), and the only place on this track where the write is not
/// merely permitted but destructive.
///
/// `NotFound` is still an answer, because a projection that has never been
/// written genuinely has no preserved lines and a registry that does not exist
/// genuinely lists nothing. Every other error refuses.
fn read_to_string_or_absent(path: &Path) -> Result<String, SnapshotError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        // `NotADirectory` alongside `NotFound` for the reason `fs_probe::node_kind`
        // states: both are facts about the path naming nothing, not about the
        // process being unable to look.
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::NotADirectory =>
        {
            Ok(String::new())
        }
        Err(e) => Err(SnapshotError::IoError {
            message: format!(
                "source_uninspectable: {} exists and could not be read: {e}. Treating it as EMPTY \
                 would rebuild what follows from nothing and write that over the real file — an \
                 unreadable input is not an empty one.",
                path.display()
            ),
        }),
    }
}

fn uninspectable_or_kind(path: &Path) -> Result<NodeKind, SnapshotError> {
    fs_probe::node_kind(path).map_err(|e| SnapshotError::IoError {
        message: format!(
            "artifact_lookup_uninspectable: {} exists and could not be inspected: {e}. This is \
             NOT the same answer as \"there is no such artifact\" — reporting it as not-found \
             sends an operator looking for something that is on disk in front of them.",
            path.display()
        ),
    })
}

/// Resolve the legacy-kind static string for an artifact path, if it maps to
/// one of the 6 hardcoded kinds. Returns `None` for domain-machine kinds (which
/// `read_artifact_kind` resolves from status.yaml instead).
///
/// **C-d.1 round 7, H-1.** This loop was `candidate.exists()`, and it sat
/// **35 lines below** the byte-identical loop round 6 converted in
/// `locate_artifact_dir` — the same nine directories, the same swallow,
/// unfixed, in the same file, described in the same commit. The consequence is
/// visible on the port surface without any mutation: with `tracks/` at 0600 and
/// `tracks/T_alpha/status.yaml` on disk, `read_artifact_kind` answered
/// `NotFound` (gRPC NOT_FOUND) while its sibling `read_artifact_state`, reading
/// the SAME artifact at the SAME mode through the FIXED lookup, answered
/// `IoError` (INTERNAL). Two methods of one port disagreeing about whether an
/// artifact exists is the tell that a module list closed the file and not the
/// behaviour.
fn kind_from_path(
    hearth: &PathBuf,
    artifact_path: &str,
) -> Result<Option<&'static str>, SnapshotError> {
    // If the path contains one of the known directory names, map it.
    for (kind, dir) in ARTIFACT_DIRS {
        let prefix = format!("{}/", dir);
        if artifact_path.starts_with(&prefix) {
            return Ok(Some(*kind));
        }
    }
    // Otherwise search per-kind dirs — BARE IDS ONLY.
    //
    // R2.1: the kind the engine resolves is a property of the ARTIFACT, never of
    // the string used to reach it. This loop can only honour that for a bare id.
    // `PathBuf::join` REPLACES its base when the joined segment is absolute, so
    // `hearth.join(dir).join(<absolute path>)` is just `<absolute path>` — for
    // EVERY row. The probe then succeeds on the first iteration and the function
    // returns `ARTIFACT_DIRS[0]`'s kind, so every absolutely-addressed artifact
    // read back as that kind AND had its transition registered in that kind's
    // registry, which it does not belong to.
    //
    // A `<dir>/<id>` relative address is already answered by the prefix match
    // above; anything else non-bare falls through to `kind_from_status_yaml`,
    // which reads the authoritative `kind:` off the artifact itself. Both call
    // sites of this function do exactly that on `None`.
    if std::path::Path::new(artifact_path).is_absolute() || artifact_path.contains('/') {
        return Ok(None);
    }
    for (kind, dir) in ARTIFACT_DIRS {
        let candidate = hearth.join(dir).join(artifact_path);
        if uninspectable_or_kind(&candidate)? != NodeKind::Absent {
            return Ok(Some(*kind));
        }
    }
    Ok(None)
}

/// Read the authoritative `kind:` from an artifact's status.yaml. Used for
/// domain-machine kinds whose directory is not in `ARTIFACT_DIRS` — generalizes
/// kind resolution so the snapshot path drives ANY registry-resolved machine.
///
/// **C-d.1 round 7, H-1.** This used to answer `Option` by contract and catch
/// the fixed lookup's `IoError`, printing it to stderr and returning `None`
/// anyway — so `read_artifact_kind` mapped "I could not look" back onto
/// NOT_FOUND, and round 6's fix reached one of the two methods an operator
/// meets. A warning on stderr next to a wrong answer on the wire is not a
/// diagnostic; it is the wrong answer with a receipt. The refusal now
/// propagates, and `None` means only what it says: every resolver looked, and
/// none of them found a kind.
fn kind_from_status_yaml(
    hearth: &PathBuf,
    artifact_path: &str,
) -> Result<Option<String>, SnapshotError> {
    let Some(dir) = locate_artifact_dir(hearth, artifact_path)? else {
        return Ok(None);
    };
    let status_path = dir.join("status.yaml");
    let content = match std::fs::read_to_string(&status_path) {
        Ok(content) => content,
        // The directory resolved and holds no status.yaml: a real answer, and
        // the next resolver (there is none) would say the same thing.
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::NotADirectory =>
        {
            return Ok(None)
        }
        Err(e) => {
            return Err(SnapshotError::IoError {
                message: format!(
                    "artifact_kind_uninspectable: {} exists and could not be read: {e}. \
                     Answering \"no kind\" here is answering NOT_FOUND for an artifact on disk.",
                    status_path.display()
                ),
            })
        }
    };
    // A status.yaml that does not PARSE is a readable answer about content, and
    // the resolver chain's contract is to move on. That is not the class.
    let Ok(status) = serde_yaml::from_str::<FullStatusYaml>(&content) else {
        return Ok(None);
    };
    Ok(status.kind)
}

// === SnapshotPort impl =====================================================

impl SnapshotPort for FileSystemSnapshotAdapter {
    fn read_artifact_kind(&self, artifact_path: &str) -> Result<String, SnapshotError> {
        // Legacy fast path for the 6 hardcoded kinds; fall back to the
        // authoritative kind in status.yaml for domain-machine kinds (e.g.
        // knowledge_lifecycle) whose directory is not in ARTIFACT_DIRS.
        let resolved = match kind_from_path(&self.hearth_path, artifact_path)? {
            Some(kind) => Some(kind.to_string()),
            None => kind_from_status_yaml(&self.hearth_path, artifact_path)?,
        };
        resolved.ok_or_else(|| SnapshotError::NotFound {
            artifact_path: artifact_path.to_string(),
        })
    }

    fn read_artifact_state(&self, artifact_path: &str) -> Result<String, SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let status_path = dir.join("status.yaml");
        // C-d.1 round 7, H-1, found by the matrix's per-ENTRY rows. This mapped
        // a READ failure onto `MalformedStatus` — so an unreadable status.yaml
        // was reported as a status.yaml that does not parse, which sends an
        // operator to fix a file that is perfectly well formed. Same class, a
        // different collapse: not "absent", but "corrupt". A parse failure below
        // is still `MalformedStatus`, because that one is true.
        let content = std::fs::read_to_string(&status_path).map_err(|e| SnapshotError::IoError {
            message: format!(
                "artifact_status_uninspectable: {} exists and could not be read: {e}. This is not \
                 a malformed status.yaml — it is one this process could not open.",
                status_path.display()
            ),
        })?;
        // Parse the canonical status type and resolve via the shared seam,
        // FOLDING the per-file transition event directory with the legacy
        // array (dual-read). A stateless-but-transitioned artifact — legacy or
        // event-sourced — resolves identically to the other read sites.
        let status: FullStatusYaml =
            serde_yaml::from_str(&content).map_err(|e| SnapshotError::MalformedStatus {
                artifact_path: artifact_path.to_string(),
                message: format!("Invalid YAML: {}", e),
            })?;
        // C-d.1 round 8, H-1 — the same collapse MUT-S6 fixed for the status
        // FILE, one frame down for the event STORE: unreadable evidence is an
        // I/O refusal, not a malformed status.
        anvil_core::domain::transition_log::resolve_state_with_events(&status, &dir)
            .map_err(|e| SnapshotError::IoError {
                message: format!(
                    "artifact_state_uninspectable: the transition event evidence for '{}' could \
                     not be read: {e}. Folding it away resolves the artifact to a STALE state \
                     and reports it as fact.",
                    artifact_path
                ),
            })?
            .ok_or_else(|| SnapshotError::MalformedStatus {
                artifact_path: artifact_path.to_string(),
                message: "Missing 'state' field".to_string(),
            })
    }

    fn read_artifact_actor_names(&self, artifact_path: &str) -> Result<Vec<String>, SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let status_path = dir.join("status.yaml");
        let content =
            std::fs::read_to_string(&status_path).map_err(|e| SnapshotError::IoError {
                message: format!("Failed to read status.yaml: {}", e),
            })?;
        Ok(extract_actor_names(&content))
    }

    fn read_activity_entries(
        &self,
        artifact_path: &str,
    ) -> Result<Vec<ActivityEntry>, SnapshotError> {
        let log = self
            .read_full_status(artifact_path)?
            .activity
            .unwrap_or_default();
        // Surface a degraded begin-adoption log at this adapter boundary too
        // (the snapshot/complete soft-warn reads through here). The soft-warn
        // itself is advisory — it may emit a false "no prior begin" note when a
        // begin marker was dropped — so the load-bearing conservative handling
        // lives in the hard runtime gate (`gate_check`); this warning keeps the
        // degradation observable wherever the file is read.
        if log.is_degraded() {
            if let Ok(Some(dir)) = locate_artifact_dir(&self.hearth_path, artifact_path) {
                log.warn_if_degraded(&dir.join("status.yaml"));
            }
        }
        Ok(log.entries)
    }

    fn read_transitions(
        &self,
        artifact_path: &str,
    ) -> Result<Vec<StatusTransition>, SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let status = self.read_full_status(artifact_path)?;
        anvil_core::domain::transition_log::resolve_transitions_with_events(&status, &dir).map_err(
            |e| SnapshotError::IoError {
                message: format!(
                    "transitions_uninspectable: the transition event evidence for '{}' could not \
                     be read: {e}",
                    artifact_path
                ),
            },
        )
    }

    fn read_backlog_item(&self, bi_id: &str) -> Result<LoadedBacklogItem, SnapshotError> {
        self.backlog_store()
            .load_item(bi_id)
            .map_err(backlog_store_error)
    }

    fn read_backlog_items(&self) -> Result<Vec<LoadedBacklogItem>, SnapshotError> {
        self.backlog_store().load_all().map_err(backlog_store_error)
    }

    fn read_backlog_transition_context(
        &self,
        business_node_id: &str,
    ) -> Result<BacklogTransitionContext, SnapshotError> {
        let store = self.backlog_store();
        let loaded = store.load_all().map_err(backlog_store_error)?;
        let policy = BacklogPolicy::default();
        let in_organ: Vec<BacklogItem> = loaded
            .iter()
            .filter(|l| l.item.business_node_id == business_node_id)
            .map(|l| l.item.clone())
            .collect();
        let positions = bi::materialize_rank(&in_organ, &policy);
        let mut ranked: Vec<BacklogItem> = Vec::new();
        for (id, _) in &positions {
            if let Some(item) = in_organ.iter().find(|i| &i.backlog_item_id == id) {
                ranked.push(item.clone());
            }
        }
        let mut unranked_candidate_ids: Vec<String> = in_organ
            .iter()
            .filter(|i| i.state == State::Candidate && i.rank.is_none())
            .map(|i| i.backlog_item_id.clone())
            .collect();
        unranked_candidate_ids.sort();

        // The aggregate hash covers EVERY published item revision, not only the
        // organ: a cross-organ write still moves the store, and refusing is
        // cheaper than proving isolation.
        let mut parts = Vec::new();
        for l in &loaded {
            parts.push(format!("{}\u{1}{}\u{1}{}", l.id, l.item_hash, l.history_hash));
        }
        let context_hash = content_hash(parts.join("\u{2}").as_bytes());
        let registry_path = self.hearth_path.join("backlog_items.md");
        let registry_hash = match std::fs::read(&registry_path) {
            Ok(bytes) => Some(content_hash(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(SnapshotError::IoError {
                    message: format!("read {}: {e}", registry_path.display()),
                })
            }
        };

        Ok(BacklogTransitionContext {
            business_node_id: business_node_id.to_string(),
            ranked,
            unranked_candidate_ids,
            policy,
            registry_hash,
            context_hash,
        })
    }

    fn commit_backlog_transition(
        &self,
        prepared: PreparedBacklogCommit,
    ) -> Result<(), SnapshotError> {
        self.backlog_store()
            .commit(prepared)
            .map_err(backlog_store_error)
    }

    fn append_transition(
        &self,
        artifact_path: &str,
        transition: &TransitionContent,
    ) -> Result<(), SnapshotError> {
        // K8 closure (plan Task 4): the ordinary append is NOT a K8 writer.
        // Kind is resolved authoritatively from the artifact itself, so an
        // internal `SnapshotCommandHandler::execute` caller, Complete, Amend,
        // or adoption cannot slip a backlog_item through this seam.
        if self.read_artifact_kind(artifact_path).ok().as_deref() == Some("backlog_item") {
            return Err(SnapshotError::BacklogStore {
                message: format!(
                    "'{artifact_path}' is a backlog_item: post-genesis K8 state moves only \
                     through commit_backlog_transition with a prepared capability"
                ),
            });
        }
        // Upcast: the transition HISTORY is ONE FILE PER EVENT under
        // `<artifact>/transitions/`, never an append to the shared status.yaml
        // `transitions:` array. Two concurrent transitions write two distinct
        // files and never git-merge-conflict (spark-20260412-001). Current
        // state is the fold over those events (see `read_artifact_state`).
        //
        // The top-level `state:` header IS rewritten, as a derived projection
        // of that fold (see `hearth::status_header`). The upcast's R8 forbade
        // it and that was the defect: anvil's own readers fold the events, so
        // the header simply kept its creation-time value forever and every
        // consumer reading the FILE — the tracker, the playbook atlas
        // (`domain::playbook::status_read`), a human, `grep` — read a value
        // that had not been true for months (measured: 14 transitions of drift
        // on one proposal, 7 of 10 sampled tracks stale).
        let record = TransitionRecord {
            to: transition.to.clone(),
            at: transition.at.clone(),
            actor: transition.actor.clone(),
            role: transition.role.clone(),
            approver: transition.approver.clone(),
            note: transition.note.clone(),
            satisfaction: transition.satisfaction.clone(),
            event_type: transition.event_type.clone(),
        };
        FileSystemTransitionEventAdapter::new(self.hearth_path.clone())
            .append_transition_event(artifact_path, &record)
            .map_err(|e| match e {
                TransitionEventWriteError::IoError { message } => {
                    SnapshotError::IoError { message }
                }
            })?;

        // ORDERING, chosen deliberately: the EVENT is durable first, then the
        // header is re-derived FROM THE EVENT STORE ON DISK (not from the
        // in-hand `transition`). status.yaml and the event file are two files,
        // so a single atomic write spanning both does not exist; what does
        // exist is a safe order and an idempotent repair.
        //
        // * Evidence-then-projection means the header can never assert a state
        //   no event backs. The reverse order can, and that is the strictly
        //   worse failure — a header claiming governance that did not happen.
        // * If this leg fails, the transition is still recorded and every
        //   engine read (which folds) is still correct; only the projection
        //   lags. That is exactly the pre-fix status quo, so the fix cannot
        //   regress anything by failing.
        // * It fails LOUD rather than silently: the caller is told the header
        //   is stale and `anvil-status-header-reconcile` repairs it, because a
        //   swallowed projection failure is how the header went stale for
        //   months in the first place.
        // * Deriving from disk (not from `transition`) also makes this leg
        //   self-healing: it repairs a header that a hand-edit or a git merge
        //   had already knocked out of agreement, in the same pass.
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        crate::status_header::reconcile_status_header(
            &dir,
            crate::status_header::ReconcileMode::Apply,
            // AUTHORITATIVE: this call just created the newest event in the
            // fold, so the fold is the truth by construction. The offline
            // auditor's conservatism (refusing to overwrite a header it cannot
            // prove stale) is exactly wrong here — it would leave a
            // freshly-transitioned artifact declaring its old state.
            crate::status_header::HeaderPolicy::Authoritative,
        )
        .map(|_| ())
        .map_err(|e| SnapshotError::IoError {
            message: format!(
                "status_header_not_projected: the transition to '{}' for '{}' IS recorded in the \
                 event store, but the status.yaml state: header could not be updated to match: \
                 {e}. Engine reads fold the events and stay correct; every file-level reader now \
                 sees a stale header until `anvil-status-header-reconcile --apply` is run.",
                transition.to, artifact_path
            ),
        })
    }

    fn seed_actor(&self, artifact_path: &str, actor: &ActorIdentity) -> Result<(), SnapshotError> {
        // Resolve the artifact's directory so the writer's relative path
        // matches the on-disk layout. The new ActorWritePort treats
        // `artifact_path` as already-relative-to-hearth, so we strip
        // back to whatever locate_artifact_dir landed on.
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        let relative = dir
            .strip_prefix(&self.hearth_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| artifact_path.to_string());
        let writer = FileSystemActorWriteAdapter::new(self.hearth_path.clone());
        writer
            .upsert_actor_configuration(&relative, actor)
            .map_err(|e| match e {
                ActorWriteError::IoError { message } => SnapshotError::IoError { message },
                ActorWriteError::MalformedStatus {
                    artifact_path,
                    message,
                } => SnapshotError::MalformedStatus {
                    artifact_path,
                    message,
                },
                ActorWriteError::NotFound { artifact_path } => {
                    SnapshotError::NotFound { artifact_path }
                }
            })
    }

    fn registry_entry_exists(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<bool, SnapshotError> {
        // C-d.1 round 7, H-1. Was `!path.exists()`. `Ok(false)` here means "this
        // artifact has no entry in this registry", which is what the caller
        // checks before CREATING one — so an unreadable registry authorized a
        // duplicate entry. Measured on unmutated `579c7b3` with the hearth root
        // at 0600 and a real `tracks.md` naming the artifact: `Ok(false)`.
        let path = self.hearth_path.join(registry_file);
        if uninspectable_or_kind(&path)? == NodeKind::Absent {
            return Ok(false);
        }
        let content = std::fs::read_to_string(&path).map_err(|e| SnapshotError::IoError {
            message: format!("Failed to read {}: {}", registry_file, e),
        })?;
        let id_slash = format!("/{}/", artifact_id);
        let id_paren = format!("/{})", artifact_id);
        for line in content.lines() {
            if line.contains(&id_slash) || line.contains(&id_paren) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn build_registry_entry_text(
        &self,
        artifact_kind: &str,
        artifact_path: &str,
        _to_section: &str,
    ) -> Result<String, SnapshotError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(|| {
            SnapshotError::NotFound {
                artifact_path: artifact_path.to_string(),
            }
        })?;
        // Known legacy kinds have a conventional doc + canonical dir. Domain
        // machine kinds (e.g. knowledge_lifecycle) are NOT in this table — for
        // them, the doc read degrades to empty (the display name falls back to
        // the artifact id), and the canonical_dir is None (the rel_path is taken
        // from the supplied artifact_path verbatim). The fallback condition is
        // "the kind has no conventional doc", NOT a kind literal — so a missing
        // doc never errors.
        let doc_and_dir: Option<(&str, &str)> = match artifact_kind {
            "track" => Some(("spec.md", "tracks")),
            "proposal" => Some(("vision.md", "proposals")),
            "milestone" => Some(("milestone.md", "milestones")),
            "initiative" => Some(("definition.md", "initiatives")),
            "decision" => Some(("definition.md", "decisions")),
            "learning" => Some(("definition.md", "learnings")),
            _ => None,
        };
        // Read the conventional doc when one is declared; degrade a non-existent
        // doc to an empty string so a domain artifact with no doc resolves
        // gracefully (its display name falls back to the id via extract_h1).
        let doc = match doc_and_dir {
            Some((doc_file, _)) => read_to_string_or_absent(&dir.join(doc_file))?,
            None => String::new(),
        };
        let name = extract_h1(&doc).unwrap_or_else(|| {
            let id = artifact_path
                .rsplit('/')
                .next()
                .unwrap_or(artifact_path)
                .to_string();
            id
        });
        let summary = name.to_lowercase();
        let artifact_id = artifact_path
            .rsplit('/')
            .next()
            .unwrap_or(artifact_path)
            .to_string();
        let rel_path = match doc_and_dir {
            Some((_, canonical_dir))
                if artifact_path.starts_with(&format!("{}/", canonical_dir)) =>
            {
                artifact_path.to_string()
            }
            Some((_, canonical_dir)) => format!("{}/{}", canonical_dir, artifact_id),
            // Unknown kind: trust the supplied artifact_path as the rel path.
            None => artifact_path.to_string(),
        };

        let entry = match artifact_kind {
            "track" => {
                // Track entry includes a parent link. Read the unified
                // `parent_id:` key from status.yaml, falling back to the
                // legacy `proposal:` key for un-migrated files.
                let status = std::fs::read_to_string(dir.join("status.yaml")).map_err(|e| {
                    SnapshotError::IoError {
                        message: format!("Failed to read status.yaml: {}", e),
                    }
                })?;
                let proposal_id = status
                    .lines()
                    .find_map(|l| {
                        let t = l.trim();
                        t.strip_prefix("parent_id:")
                            .or_else(|| t.strip_prefix("proposal:"))
                            .map(|s| s.trim().to_string())
                    })
                    .unwrap_or_default();
                let proposal_name = derive_slug(&proposal_id);
                if proposal_id.is_empty() {
                    format!("- [{}]({}/) — {}", name, rel_path, summary)
                } else {
                    format!(
                        "- [{}]({}/) — {} — [{}](proposals/{}/)",
                        name, rel_path, summary, proposal_name, proposal_id
                    )
                }
            }
            "decision" | "learning" => {
                let domain_tags = extract_domain_tags(&doc);
                if domain_tags.is_empty() {
                    format!("- [{}]({}/) — {}", name, rel_path, summary)
                } else {
                    format!(
                        "- [{}]({}/) — {} — domain: {}",
                        name,
                        rel_path,
                        summary,
                        domain_tags.join(", ")
                    )
                }
            }
            _ => format!("- [{}]({}/) — {}", name, rel_path, summary),
        };
        Ok(entry)
    }

    fn create_registry_entry(
        &self,
        registry_file: &str,
        _artifact_id: &str,
        _artifact_kind: &str,
        to_section: &str,
        entry_text: &str,
    ) -> Result<(), SnapshotError> {
        let path = self.hearth_path.join(registry_file);
        // C-d.1 round 7, H-1. Was `path.exists()`. The `else` branch SEEDS a
        // fresh registry from a bare heading and the write that follows replaces
        // the file — so an unreadable registry took the seeding branch and the
        // whole registry was rewritten as a one-line stub. On a local POSIX
        // filesystem the mode that defeats the `stat` also defeats the write, so
        // I could not construct a reproduction where this ends in the clobber
        // (measured: `IoError` at every unreadable mode). It is converted
        // because `EMFILE`, `EIO` and `ESTALE` carry no such courtesy.
        let content = if uninspectable_or_kind(&path)? != NodeKind::Absent {
            std::fs::read_to_string(&path).map_err(|e| SnapshotError::IoError {
                message: format!("Failed to read {}: {}", registry_file, e),
            })?
        } else {
            // Seed with a minimal heading derived from the registry file
            // name when the file is absent.
            let registry_heading = registry_file
                .strip_suffix(".md")
                .map(|s| capitalize(s))
                .unwrap_or_else(|| "Registry".to_string());
            format!("# {}\n", registry_heading)
        };
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let section_header = format!("## {}", to_section);
        let section_idx = match lines.iter().position(|l| l.trim() == section_header) {
            Some(idx) => idx,
            None => {
                // Create section at end
                if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                    lines.push(String::new());
                }
                lines.push(section_header.clone());
                lines.push(String::new());
                lines.len() - 2
            }
        };
        let mut insert_idx = section_idx + 1;
        while insert_idx < lines.len() && lines[insert_idx].trim().is_empty() {
            insert_idx += 1;
        }
        // Insert a blank line between the section header and a newly-
        // inserted entry if the current position is immediately after
        // the header.
        if insert_idx == section_idx + 1 {
            lines.insert(insert_idx, String::new());
            insert_idx += 1;
        }
        lines.insert(insert_idx, entry_text.to_string());
        let new_content = ensure_trailing_newline(lines.join("\n"));
        crate::atomic_write::atomic_write(&path, new_content.as_bytes()).map_err(|e| {
            SnapshotError::IoError {
                message: format!("Failed to write {}: {}", registry_file, e),
            }
        })
    }

    fn move_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError> {
        let path = self.hearth_path.join(registry_file);
        let content = std::fs::read_to_string(&path).map_err(|e| SnapshotError::IoError {
            message: format!("Failed to read {}: {}", registry_file, e),
        })?;
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        let id_slash = format!("/{}/", artifact_id);
        let id_paren = format!("/{})", artifact_id);
        let entry_idx = lines
            .iter()
            .position(|l| l.contains(&id_slash) || l.contains(&id_paren));
        let removed_entry = match entry_idx {
            Some(idx) => lines.remove(idx),
            None => {
                return Err(SnapshotError::IoError {
                    message: format!("Entry '{}' not found in {}", artifact_id, registry_file),
                })
            }
        };

        let header = format!("## {}", to_section);
        let target_idx = match lines.iter().position(|l| l.trim() == header) {
            Some(i) => i,
            None => {
                // Create the target section at end.
                if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                    lines.push(String::new());
                }
                lines.push(header.clone());
                lines.push(String::new());
                lines.len() - 2
            }
        };

        let mut insert_idx = target_idx + 1;
        while insert_idx < lines.len() && lines[insert_idx].trim().is_empty() {
            insert_idx += 1;
        }
        lines.insert(insert_idx, removed_entry);
        if insert_idx > 0 && !lines[insert_idx - 1].trim().is_empty() {
            lines.insert(insert_idx, String::new());
        }

        let new_content = ensure_trailing_newline(lines.join("\n"));
        crate::atomic_write::atomic_write(&path, new_content.as_bytes()).map_err(|e| {
            SnapshotError::IoError {
                message: format!("Failed to write {}: {}", registry_file, e),
            }
        })
    }

    fn move_execution_row(&self, track_id: &str, to_section: &str) -> Result<(), SnapshotError> {
        let path = self.hearth_path.join("projections/execution.md");
        // Resolve the display name from tracks.md so the row match
        // hits the human-readable table entry (e.g., "Sample Track")
        // rather than the directory-id slug.
        let display_name = self
            .resolve_display_name("tracks.md", track_id)?
            .unwrap_or_else(|| track_id.to_string());
        move_projection_row_inner(&path, &display_name, "", to_section, "track")
    }

    fn move_intent_row(
        &self,
        artifact_id: &str,
        kind: &str,
        to_section: &str,
    ) -> Result<(), SnapshotError> {
        let path = self.hearth_path.join("projections/intent.md");
        let registry_file = match kind {
            "proposal" => "proposals.md",
            "milestone" => "milestones.md",
            _ => {
                return Err(SnapshotError::InvalidArgument {
                    reason: format!("unknown kind '{}' for intent.md move", kind),
                })
            }
        };
        let display_name = self
            .resolve_display_name(registry_file, artifact_id)?
            .unwrap_or_else(|| artifact_id.to_string());
        move_intent_row_inner(&path, &display_name, kind, to_section)
    }

    fn rebuild_decisions_projection(&self) -> Result<(), SnapshotError> {
        // Counter/content rebuild from the decisions.md registry, per
        // the "Checkpoint/compaction model" truth invariant. We preserve
        // the existing projection's `base_snapshot` (only the human
        // owns that value) and increment `incremental_count` instead of
        // resetting it. The narrative body (count line + Active
        // tensions + Recently resolved) is regenerated; any other
        // content in the existing projection is also preserved.
        let registry_path = self.hearth_path.join("decisions.md");
        let projection_path = self.hearth_path.join("projections/decisions.md");
        let registry = read_to_string_or_absent(&registry_path)?;

        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        let mut section_entries: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut current: Option<String> = None;
        for line in registry.lines() {
            let trimmed = line.trim();
            if let Some(stripped) = trimmed.strip_prefix("## ") {
                current = Some(stripped.to_string());
                continue;
            }
            if trimmed.starts_with("- ") {
                if let Some(section) = &current {
                    *counts.entry(section.clone()).or_insert(0) += 1;
                    section_entries
                        .entry(section.clone())
                        .or_default()
                        .push(trimmed.to_string());
                }
            }
        }

        let count_line = format!(
            "Tension: {} | Investigating: {} | Decided: {} | Retired: {}",
            counts.get("tension").copied().unwrap_or(0),
            counts.get("investigating").copied().unwrap_or(0),
            counts.get("decided").copied().unwrap_or(0),
            counts.get("retired").copied().unwrap_or(0),
        );

        let empty_vec: Vec<String> = Vec::new();
        let tensions = section_entries.get("tension").unwrap_or(&empty_vec);
        let active_tensions_block = if tensions.is_empty() {
            "_None_".to_string()
        } else {
            tensions.join("\n")
        };
        let decided = section_entries.get("decided").unwrap_or(&empty_vec);
        let recent_decided: Vec<String> = decided.iter().take(5).cloned().collect();
        let recent_block = if recent_decided.is_empty() {
            "_None_".to_string()
        } else {
            recent_decided.join("\n")
        };

        let existing = read_to_string_or_absent(&projection_path)?;
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let base_snapshot_line = existing
            .lines()
            .find(|l| l.starts_with("base_snapshot:"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("base_snapshot: {}", now));
        let incremental_count = existing
            .lines()
            .find(|l| l.starts_with("incremental_count:"))
            .and_then(|l| {
                l.split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<i64>().ok())
            })
            .unwrap_or(0);
        let frontmatter = format!(
            "---\nincremental_count: {}\n{}\nlast_updated: {}\nafter_event: \"projection-update: decisions rebuild\"\n---",
            incremental_count + 1,
            base_snapshot_line,
            now
        );
        let body = format!(
            "\n\n# Anvil — Decisions\n\n{}\n\n## Active tensions\n\n{}\n\n## Recently resolved\n\n{}\n",
            count_line, active_tensions_block, recent_block
        );
        let combined = format!("{}{}", frontmatter, body);
        if let Some(parent) = projection_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SnapshotError::IoError {
                message: format!("Failed to create projections dir: {}", e),
            })?;
        }
        crate::atomic_write::atomic_write(&projection_path, combined.as_bytes()).map_err(
            |e| SnapshotError::IoError {
                message: format!("Failed to write decisions projection: {}", e),
            },
        )
    }

    fn rebuild_sparks_projection(&self) -> Result<(), SnapshotError> {
        let sparks_path = self.hearth_path.join("sparks/sparks.md");
        let projection_path = self.hearth_path.join("projections/sparks.md");
        let sparks_source = read_to_string_or_absent(&sparks_path)?;

        let mut spark_ids: Vec<String> = Vec::new();
        let mut disposition_targets: HashSet<String> = HashSet::new();
        let mut annotation_count: usize = 0;
        let mut current_section: Option<String> = None;
        for line in sparks_source.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("## ") {
                current_section = Some(rest.to_string());
                if rest.starts_with("annotation:") {
                    annotation_count += 1;
                }
                continue;
            }
            if let Some(section) = &current_section {
                if section.starts_with("spark:") {
                    if let Some(id_val) = trimmed.strip_prefix("id:") {
                        spark_ids.push(id_val.trim().to_string());
                    }
                } else if section.starts_with("disposition:") {
                    if let Some(target) = trimmed.strip_prefix("target:") {
                        disposition_targets.insert(target.trim().to_string());
                    }
                }
            }
        }
        let untriaged = spark_ids
            .iter()
            .filter(|id| !disposition_targets.contains(*id))
            .count();

        // Read existing projection to preserve narrative lines.
        let existing = read_to_string_or_absent(&projection_path)?;

        // Preserve any line starting with "Last reflection:" — this line
        // is owned by the terminal-transition hook (and a future rebuild),
        // not this engine. If no such line exists we leave it absent
        // rather than synthesising one.
        let preserved_reflection = existing
            .lines()
            .find(|l| l.trim_start().starts_with("Last reflection:"))
            .map(|s| s.to_string());

        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (base_snapshot_line, incremental_count_line) = {
            let base = existing
                .lines()
                .find(|l| l.starts_with("base_snapshot:"))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("base_snapshot: {}", now));
            let count = existing
                .lines()
                .find(|l| l.starts_with("incremental_count:"))
                .and_then(|l| {
                    l.split(':')
                        .nth(1)
                        .and_then(|s| s.trim().parse::<i64>().ok())
                })
                .unwrap_or(0);
            (base, format!("incremental_count: {}", count + 1))
        };

        let frontmatter = format!(
            "---\n{}\n{}\nlast_updated: {}\nafter_event: \"projection-update: sparks rebuild\"\n---",
            incremental_count_line, base_snapshot_line, now
        );
        let body = match preserved_reflection {
            Some(line) => format!(
                "\n\n# Anvil — Sparks\n\nUntriaged sparks: {}\nAnnotations: {}\n\n{}\n",
                untriaged, annotation_count, line
            ),
            None => format!(
                "\n\n# Anvil — Sparks\n\nUntriaged sparks: {}\nAnnotations: {}\n",
                untriaged, annotation_count
            ),
        };
        let combined = format!("{}{}", frontmatter, body);
        if let Some(parent) = projection_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SnapshotError::IoError {
                message: format!("Failed to create projections dir: {}", e),
            })?;
        }
        crate::atomic_write::atomic_write(&projection_path, combined.as_bytes()).map_err(
            |e| SnapshotError::IoError {
                message: format!("Failed to write sparks projection: {}", e),
            },
        )
    }

    fn write_authoring_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        at: &str,
        actor: &str,
        role: &str,
    ) -> Result<(), SnapshotError> {
        self.write_projection_file(
            artifact_path,
            "authoring.md",
            phase_label,
            state,
            at,
            actor,
            role,
        )
    }

    fn write_artifact_projection(
        &self,
        artifact_path: &str,
        phase_label: &str,
        state: &str,
        at: &str,
        actor: &str,
        role: &str,
    ) -> Result<(), SnapshotError> {
        self.write_projection_file(
            artifact_path,
            "projection.md",
            phase_label,
            state,
            at,
            actor,
            role,
        )
    }

    fn state_declares_projection(
        &self,
        artifact_path: &str,
        to_state: &str,
    ) -> Result<bool, SnapshotError> {
        // Resolve the artifact's kind (legacy fast path, else status.yaml).
        let resolved = match kind_from_path(&self.hearth_path, artifact_path)? {
            Some(kind) => Some(kind.to_string()),
            None => kind_from_status_yaml(&self.hearth_path, artifact_path)?,
        };
        let Some(kind) = resolved else {
            return Ok(false);
        };
        let reg = anvil_core::domain::playbook::hearth_registry::HearthPlaybookRegistry::new(
            self.hearth_path.clone(),
        );
        let machine = match reg.machine_for(&kind) {
            Some(m) => m,
            None => return Ok(false),
        };
        let declares = machine
            .states
            .iter()
            .find(|s| s.name == to_state)
            .map(|s| !s.projection_targets.is_empty())
            .unwrap_or(false);
        Ok(declares)
    }

    fn generate_actor_name(
        &self,
        existing_actor_names: &[String],
    ) -> Result<String, SnapshotError> {
        let words = load_words().map_err(|e| SnapshotError::IoError {
            message: format!("Failed to load word list: {}", e),
        })?;
        if words.is_empty() {
            return Err(SnapshotError::IoError {
                message: "Word list is empty after filtering".to_string(),
            });
        }
        for _ in 0..10 {
            let idx = random_u32().map_err(|e| SnapshotError::IoError { message: e })? as usize
                % words.len();
            let suffix =
                random_u32().map_err(|e| SnapshotError::IoError { message: e })? % 1_000_000;
            let candidate = format!("{}-{:06}", words[idx], suffix);
            if !existing_actor_names.iter().any(|n| n == &candidate) {
                return Ok(candidate);
            }
        }
        Err(SnapshotError::IoError {
            message:
                "failed to generate a unique actor name after 10 attempts — actors table saturation or insufficient entropy".to_string(),
        })
    }

    fn write_carry_forward(
        &self,
        artifact_path: &str,
        content: &str,
    ) -> Result<String, SnapshotError> {
        // Write `<artifact_path>/carry-forward.md` as a primary artifact. The
        // artifact dir must already exist (created at artifact creation); fall
        // back to a literal join so an absent dir surfaces as an I/O error.
        let artifact_dir = locate_artifact_dir(&self.hearth_path, artifact_path)?
            .unwrap_or_else(|| self.hearth_path.join(artifact_path));
        let file = artifact_dir.join("carry-forward.md");
        // Append-only guard (spec R3.3): never silently overwrite.
        //
        // C-d.1 round 7, H-1. Was `file.exists()`. This is the guard whose whole
        // job is to refuse a write, and `false` is its clearing answer — the
        // exact shape of the class. Same honest limit as `create_registry_entry`:
        // no local mode makes the `stat` fail while the write succeeds, so the
        // measured consequence is a misleading error rather than an overwrite.
        if uninspectable_or_kind(&file)? != NodeKind::Absent {
            return Err(SnapshotError::IoError {
                message: format!("carry-forward.md already exists for {}", artifact_path),
            });
        }
        crate::atomic_write::atomic_write(&file, content.as_bytes()).map_err(|e| {
            SnapshotError::IoError {
                message: format!("Failed to write carry-forward.md: {}", e),
            }
        })?;
        Ok(file.display().to_string())
    }
}

// === Private helpers =======================================================

fn ensure_trailing_newline(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Recognise an `actors:` key-line in any of its legal YAML shapes:
/// block-style header (`actors:`), inline empty mapping
/// (`actors: {}`), or explicit null (`actors: null`, `actors: ~`).
/// Matches only at zero indentation so nested keys named `actors:` in
/// indented blocks don't spuriously trip.
fn is_actors_header(line: &str) -> bool {
    if line.starts_with(' ') || line.is_empty() {
        return false;
    }
    let trimmed = line.trim();
    if trimmed == "actors:" {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("actors:") {
        let rest = rest.trim();
        return rest == "{}" || rest == "null" || rest == "~";
    }
    false
}

fn extract_actor_names(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let actors_idx = match lines.iter().position(|l| is_actors_header(l)) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let mut names = Vec::new();
    for j in (actors_idx + 1)..lines.len() {
        let line = lines[j];
        if !line.starts_with(' ') && !line.is_empty() {
            break;
        }
        // Actor-level entries are two-space-indented and end with ':'.
        if line.starts_with("  ") && !line.starts_with("    ") {
            if let Some(trimmed) = line.trim_start().strip_suffix(':') {
                names.push(trimmed.to_string());
            }
        }
    }
    names
}

fn extract_h1(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn extract_domain_tags(markdown: &str) -> Vec<String> {
    // Reads frontmatter `domain:` key. Supports both inline list and
    // per-line list formats.
    let mut in_frontmatter = false;
    let mut saw_start = false;
    let mut collected: Vec<String> = Vec::new();
    let mut in_domain_list = false;
    for line in markdown.lines() {
        if line.trim() == "---" {
            if !saw_start {
                saw_start = true;
                in_frontmatter = true;
                continue;
            } else if in_frontmatter {
                break;
            }
        }
        if !in_frontmatter {
            continue;
        }
        if let Some(val) = line.trim().strip_prefix("domain:") {
            let val = val.trim();
            if val.starts_with('[') && val.ends_with(']') {
                for t in val[1..val.len() - 1].split(',') {
                    let tag = t.trim().trim_matches('"').trim_matches('\'');
                    if !tag.is_empty() {
                        collected.push(tag.to_string());
                    }
                }
            } else if val.is_empty() {
                in_domain_list = true;
            } else {
                collected.push(val.trim_matches('"').trim_matches('\'').to_string());
            }
        } else if in_domain_list {
            if let Some(tag) = line.trim().strip_prefix("- ") {
                let tag = tag.trim().trim_matches('"').trim_matches('\'');
                if !tag.is_empty() {
                    collected.push(tag.to_string());
                }
            } else if !line.starts_with(' ') {
                in_domain_list = false;
            }
        }
    }
    collected
}

/// Derive a kebab-case slug from a timestamped snake_case artifact id.
///
/// **Assumed input shape:** `{YYYYMMDDTHHMM}_{snake_case_name}` — the
/// convention used by tracks and proposals. The timestamp prefix is
/// stripped and underscores become hyphens:
/// `"20260411T2021_anvil_workflow_engine"` → `"anvil-workflow-engine"`.
///
/// **Narrow use:** only called for proposal parent-name derivation in
/// `build_registry_entry_text`'s `track` branch. Initiatives, decisions,
/// and learnings use kebab-case ids without a timestamp prefix and
/// should NOT be passed through this function — their ids are already
/// the desired slug.
fn derive_slug(id: &str) -> String {
    let after_ts = id.splitn(2, '_').nth(1).unwrap_or(id);
    after_ts.replace('_', "-")
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn decrement_section_count(lines: &mut [String], section_label: &str) {
    let section_pat = format!("## {} (", section_label);
    if let Some(idx) = lines.iter().position(|l| l.starts_with(&section_pat)) {
        let current: i64 = lines[idx]
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        let next = (current - 1).max(0);
        lines[idx] = format!("## {} ({})", section_label, next);
    }
}

/// Insert a row under `## {section} (N)`, creating the table if absent.
fn insert_projection_row(
    lines: &mut Vec<String>,
    section_label: &str,
    row: &str,
    kind: &str,
) -> Result<(), SnapshotError> {
    let section_pat = format!("## {} (", section_label);
    let section_idx = match lines.iter().position(|l| l.starts_with(&section_pat)) {
        Some(i) => i,
        None => {
            // Append at end.
            if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
            lines.push(format!("## {} (0)", section_label));
            lines.len() - 1
        }
    };
    let current_count: i64 = lines[section_idx]
        .split('(')
        .nth(1)
        .and_then(|s| s.split(')').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    lines[section_idx] = format!("## {} ({})", section_label, current_count + 1);
    let mut insert_idx = section_idx + 1;
    let mut has_table = false;
    let header_marker = match kind {
        "track" => "| Track",
        _ => "| Name",
    };
    while insert_idx < lines.len() {
        let trimmed = lines[insert_idx].trim();
        if trimmed.starts_with("## ") {
            break;
        }
        if trimmed.starts_with(header_marker) {
            has_table = true;
        }
        if has_table && (trimmed.starts_with("## ") || trimmed.is_empty()) {
            break;
        }
        insert_idx += 1;
    }
    if !has_table {
        lines.insert(section_idx + 1, String::new());
        match kind {
            "track" => {
                lines.insert(section_idx + 2, "| Track | Proposal |".to_string());
                lines.insert(section_idx + 3, "|-------|----------|".to_string());
            }
            _ => {
                lines.insert(section_idx + 2, "| Name |".to_string());
                lines.insert(section_idx + 3, "|------|".to_string());
            }
        }
        lines.insert(section_idx + 4, row.to_string());
    } else {
        lines.insert(insert_idx, row.to_string());
    }
    Ok(())
}

/// Move a bullet-list row in `intent.md` between H3 state sections
/// scoped by an H2 group header (`## Proposals` or `## Milestones`).
/// The row shape is `- **Name** — summary.` The H3 headers carry a
/// `(N)` count suffix that gets decremented on source and incremented
/// on target. Target section is created at the end of the H2 group if
/// absent.
fn move_intent_row_inner(
    path: &PathBuf,
    name: &str,
    kind: &str,
    to_section: &str,
) -> Result<(), SnapshotError> {
    let content = std::fs::read_to_string(path).map_err(|e| SnapshotError::IoError {
        message: format!("Failed to read {}: {}", path.display(), e),
    })?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Update frontmatter counters.
    for line in &mut lines {
        if line.starts_with("incremental_count:") {
            let current: i64 = line
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            *line = format!("incremental_count: {}", current + 1);
        }
        if line.starts_with("last_updated:") {
            *line = format!(
                "last_updated: {}",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
            );
        }
        if line.starts_with("after_event:") {
            let snake_name = name.to_lowercase().replace(' ', "-");
            let to_snake = to_section.to_lowercase().replace(' ', "-");
            *line = format!(
                "after_event: \"transition({}): {} → {}\"",
                kind, snake_name, to_snake
            );
        }
    }

    let group_header = match kind {
        "proposal" => "## Proposals",
        "milestone" => "## Milestones",
        other => {
            return Err(SnapshotError::InvalidArgument {
                reason: format!("unknown intent kind '{}'", other),
            })
        }
    };
    let group_start = lines
        .iter()
        .position(|l| l.trim() == group_header)
        .ok_or_else(|| SnapshotError::IoError {
            message: format!("Section '{}' not found in intent.md", group_header),
        })?;
    let group_end = ((group_start + 1)..lines.len())
        .find(|&i| lines[i].starts_with("## ") && !lines[i].starts_with("### "))
        .unwrap_or(lines.len());

    // Locate the bullet row matching `name` within the group range.
    let row_marker_prefix = format!("- **{}** ", name);
    let row_marker_exact = format!("- **{}**", name);
    let row_idx = ((group_start + 1)..group_end).find(|&i| {
        let l = lines[i].trim_start();
        l == row_marker_exact.as_str() || l.starts_with(&row_marker_prefix)
    });

    // If found, remove and decrement source H3 count.
    let (removed_row, group_end) = match row_idx {
        Some(idx) => {
            // Find the most recent H3 above idx to decrement.
            let mut source_h3_idx: Option<usize> = None;
            for i in (group_start..idx).rev() {
                if lines[i].starts_with("### ") {
                    source_h3_idx = Some(i);
                    break;
                }
                if lines[i].starts_with("## ") && !lines[i].starts_with("### ") {
                    break;
                }
            }
            let removed = lines.remove(idx);
            if let Some(h3_idx) = source_h3_idx {
                if let Some(paren) = lines[h3_idx].find('(') {
                    if let Some(paren_close) = lines[h3_idx].find(')') {
                        let count: i64 = lines[h3_idx][paren + 1..paren_close]
                            .trim()
                            .parse()
                            .unwrap_or(1);
                        let next = (count - 1).max(0);
                        let label = lines[h3_idx][4..paren].trim().to_string();
                        lines[h3_idx] = format!("### {} ({})", label, next);
                    }
                }
            }
            (removed, group_end - 1)
        }
        None => (format!("- **{}**", name), group_end),
    };

    // Insert under the target H3, creating it if absent.
    let target_header_prefix = format!("### {} (", to_section);
    let target_h3_idx =
        ((group_start + 1)..group_end).find(|&i| lines[i].starts_with(&target_header_prefix));
    let target_h3_idx = match target_h3_idx {
        Some(i) => i,
        None => {
            // Insert a new H3 at the end of the group. group_end is no longer
            // read after this match arm — the caller computes its insert
            // position from target_h3_idx + 1 below — so we deliberately do
            // not bump it here.
            let insert_at = group_end;
            lines.insert(insert_at, format!("### {} (0)", to_section));
            insert_at
        }
    };
    if let Some(paren) = lines[target_h3_idx].find('(') {
        if let Some(paren_close) = lines[target_h3_idx].find(')') {
            let count: i64 = lines[target_h3_idx][paren + 1..paren_close]
                .trim()
                .parse()
                .unwrap_or(0);
            let label = lines[target_h3_idx][4..paren].trim().to_string();
            lines[target_h3_idx] = format!("### {} ({})", label, count + 1);
        }
    }
    // Insert the bullet row after the H3 header (and any existing rows
    // go after — we prepend to keep newest-first, matching intent.md's
    // convention of listing active items first).
    let mut insert_at = target_h3_idx + 1;
    // Skip any blank line immediately after the header.
    if insert_at < lines.len() && lines[insert_at].trim().is_empty() {
        insert_at += 1;
    }
    lines.insert(insert_at, removed_row);

    let new_content = ensure_trailing_newline(lines.join("\n"));
    crate::atomic_write::atomic_write(path, new_content.as_bytes()).map_err(|e| {
        SnapshotError::IoError {
            message: format!("Failed to write {}: {}", path.display(), e),
        }
    })
}

fn move_projection_row_inner(
    path: &PathBuf,
    name: &str,
    _from_section: &str,
    to_section: &str,
    kind: &str,
) -> Result<(), SnapshotError> {
    let content = std::fs::read_to_string(path).map_err(|e| SnapshotError::IoError {
        message: format!("Failed to read {}: {}", path.display(), e),
    })?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Update frontmatter.
    for line in &mut lines {
        if line.starts_with("incremental_count:") {
            let current: i64 = line
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            *line = format!("incremental_count: {}", current + 1);
        }
        if line.starts_with("last_updated:") {
            *line = format!(
                "last_updated: {}",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
            );
        }
        if line.starts_with("after_event:") {
            let snake_name = name.to_lowercase().replace(' ', "-");
            let to_snake = to_section.to_lowercase().replace(' ', "-");
            *line = format!(
                "after_event: \"transition({}): {} → {}\"",
                kind, snake_name, to_snake
            );
        }
    }

    // Find the row matching `name` in any section and remove it.
    let row_pat = format!("| {} |", name);
    let row_idx = lines.iter().position(|l| l.starts_with(&row_pat));
    let removed = match row_idx {
        Some(idx) => {
            // Decrement the FROM section count by scanning backward for
            // the most-recent `## Label (N)` header before idx.
            let mut from_label: Option<String> = None;
            for i in (0..idx).rev() {
                if let Some(rest) = lines[i].strip_prefix("## ") {
                    if let Some(paren_idx) = rest.find('(') {
                        from_label = Some(rest[..paren_idx].trim().to_string());
                        break;
                    }
                }
            }
            let removed = lines.remove(idx);
            if let Some(label) = from_label {
                decrement_section_count(&mut lines, &label);
            }
            removed
        }
        None => {
            // Row doesn't exist — synthesise one and insert.
            match kind {
                "track" => format!("| {} |  |", name),
                _ => format!("| {} |", name),
            }
        }
    };
    insert_projection_row(&mut lines, to_section, &removed, kind)?;

    let new_content = ensure_trailing_newline(lines.join("\n"));
    crate::atomic_write::atomic_write(path, new_content.as_bytes()).map_err(|e| {
        SnapshotError::IoError {
            message: format!("Failed to write {}: {}", path.display(), e),
        }
    })
}

fn load_words() -> Result<Vec<String>, String> {
    let path = "/usr/share/dict/words";
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
    let re_upper = regex_match("[A-Z][a-z]{3,7}");
    let re_lower = regex_match("[a-z]{4,8}");
    let upper: Vec<String> = content
        .lines()
        .filter(|l| re_upper(l))
        .map(|s| s.to_string())
        .collect();
    if !upper.is_empty() {
        return Ok(upper);
    }
    let lower: Vec<String> = content
        .lines()
        .filter(|l| re_lower(l))
        .map(|s| {
            let mut cs = s.chars();
            match cs.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
            }
        })
        .collect();
    Ok(lower)
}

fn regex_match(pattern: &'static str) -> impl Fn(&&str) -> bool {
    move |line: &&str| {
        // Pattern is either "[A-Z][a-z]{3,7}" or "[a-z]{4,8}".
        let first_upper = pattern.starts_with("[A-Z]");
        let mut chars = line.chars();
        let first = match chars.next() {
            None => return false,
            Some(c) => c,
        };
        if first_upper {
            if !first.is_ascii_uppercase() {
                return false;
            }
        } else {
            if !first.is_ascii_lowercase() {
                return false;
            }
        }
        let rest_min = if first_upper { 3 } else { 3 }; // minus 1 for first char
        let rest_max = if first_upper { 7 } else { 7 };
        let mut count = 0;
        for c in chars {
            if !c.is_ascii_lowercase() {
                return false;
            }
            count += 1;
            if count > rest_max {
                return false;
            }
        }
        count >= rest_min
    }
}

fn random_u32() -> Result<u32, String> {
    let mut buf = [0u8; 4];
    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|e| format!("Failed to open /dev/urandom: {}", e))?;
    f.read_exact(&mut buf)
        .map_err(|e| format!("Failed to read /dev/urandom: {}", e))?;
    Ok(u32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    //! Regression tests for spark-20260503-002 — note escaping in transition
    //! records. The record now lives in a per-file transition event
    //! (`<artifact>/transitions/*.yaml`), serialized via serde, so `"`/`\`
    //! escaping is STRUCTURAL: the event file must parse cleanly and round-trip
    //! the exact note. (Pre-upcast this was a hand-rolled status.yaml escape;
    //! the invariant is the same, the surface moved.)
    use super::*;
    use anvil_core::domain::transition_log::read_event_files;
    use std::fs;
    use tempfile::TempDir;

    fn make_track_dir(hearth: &std::path::Path) -> std::path::PathBuf {
        let dir = hearth.join("tracks/regression_track");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("status.yaml"),
            "version: 1\nkind: track\nstate: spec\nactors: {}\ntransitions:\n  - to: spec\n    at: \"2026-01-01T00:00:00Z\"\n    actor: nick\n    role: spec\n",
        )
        .unwrap();
        fs::write(
            hearth.join("tracks.md"),
            "# Tracks\n\n## Spec\n\n- [T](tracks/regression_track/)\n",
        )
        .unwrap();
        dir
    }

    /// Append a transition carrying `note`, then return the parsed-back note
    /// from the single event file the write produced.
    fn append_and_read_back_note(note: &str) -> Option<String> {
        let tmp = TempDir::new().unwrap();
        let dir = make_track_dir(tmp.path());
        let adapter = FileSystemSnapshotAdapter::new(tmp.path().to_path_buf());
        let transition = TransitionContent {
            to: "plan".to_string(),
            at: "2026-01-01T01:00:00Z".to_string(),
            actor: "nick".to_string(),
            role: "approve".to_string(),
            approver: Some("nick".to_string()),
            note: Some(note.to_string()),
            satisfaction: None,
            event_type: None,
        };
        adapter
            .append_transition("tracks/regression_track", &transition)
            .expect("append_transition");
        // The event file must parse cleanly (structural escaping) and carry the
        // exact note back.
        let events = read_event_files(&dir).expect("read the event store");
        assert_eq!(events.len(), 1, "expected exactly one event file");
        events[0].record.note.clone()
    }

    #[test]
    fn note_with_double_quote_round_trips() {
        let note = r#"Approved with "quoted" wording."#;
        assert_eq!(append_and_read_back_note(note).as_deref(), Some(note));
    }

    #[test]
    fn note_ending_with_double_quote_round_trips() {
        // The exact reproduction case from spark-20260503-002.
        let note = r#"... and updates the relevant projection."#;
        assert_eq!(append_and_read_back_note(note).as_deref(), Some(note));
    }

    #[test]
    fn note_with_backslash_round_trips() {
        let note = r#"Windows-style path C:\Users\nick survives."#;
        assert_eq!(append_and_read_back_note(note).as_deref(), Some(note));
    }

    #[test]
    fn note_with_nested_quotes_and_backslashes_round_trips() {
        let note = r#"Mixed: "quote" + \backslash + "end""#;
        assert_eq!(append_and_read_back_note(note).as_deref(), Some(note));
    }

    #[test]
    fn plain_note_still_works() {
        let note = "A simple unquoted note.";
        assert_eq!(append_and_read_back_note(note).as_deref(), Some(note));
    }

    #[test]
    fn transition_reprojects_state_header_without_touching_the_legacy_array() {
        // AC5 at the unit seam: recording a transition writes an event file,
        // leaves the legacy status.yaml `transitions:` array untouched, and
        // re-projects the top-level `state:` header onto the folded state.
        let tmp = TempDir::new().unwrap();
        make_track_dir(tmp.path());
        let adapter = FileSystemSnapshotAdapter::new(tmp.path().to_path_buf());
        let transition = TransitionContent {
            to: "plan".to_string(),
            at: "2026-01-01T01:00:00Z".to_string(),
            actor: "nick".to_string(),
            role: "approve".to_string(),
            approver: None,
            note: None,
            satisfaction: None,
            event_type: None,
        };
        adapter
            .append_transition("tracks/regression_track", &transition)
            .expect("append_transition");
        let status = fs::read_to_string(tmp.path().join("tracks/regression_track/status.yaml"))
            .expect("read status.yaml");
        assert!(
            !status.contains("to: plan"),
            "status.yaml array must not gain the new transition:\n{}",
            status
        );
        assert!(
            status.lines().any(|l| l == "state: plan"),
            "top-level state must be re-projected onto the folded state 'plan':\n{}",
            status
        );
        // The current state still resolves to the new target via the fold.
        assert_eq!(
            adapter
                .read_artifact_state("tracks/regression_track")
                .unwrap(),
            "plan"
        );
    }
}

/// Carry a strict K8 store rejection through unchanged. Flattening it into a
/// generic warning is exactly the failure mode the plan forbids.
fn backlog_store_error(e: BacklogStoreError) -> SnapshotError {
    SnapshotError::BacklogStore {
        message: e.to_string(),
    }
}
