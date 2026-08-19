//! Filesystem implementation of `TransitionEventWritePort`.
//!
//! Each transition is its own file under `<artifact_path>/transitions/`. The
//! filename is content-addressed (`<at>_<actor>_<random>.yaml`) plus a random
//! suffix — never a sibling count — so two concurrent writers never collide on
//! a name and never git-merge-conflict (spark-20260412-001). The record is
//! serialized via serde (clean YAML, structural note escaping) and written
//! through `atomic_write` (temp + rename).

use anvil_core::ports::transition_event_write_port::{
    event_file_name, TransitionEventWriteError, TransitionEventWritePort, TransitionRecord,
    TRANSITIONS_DIR,
};
use rand::RngCore;
use std::path::PathBuf;

pub struct FileSystemTransitionEventAdapter {
    hearth_path: PathBuf,
}

impl FileSystemTransitionEventAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

/// Resolve the artifact directory: literal-relative first, then per-kind
/// fallback by bare id. Mirrors `fs_snapshot_adapter::locate_artifact_dir`,
/// duplicated here to keep this adapter self-contained (the snapshot one is
/// private).
///
/// # C-d.1 round 8, H-3 — the write-half twin of the lookup round 6 fixed
///
/// This was a byte-for-byte copy of the loop round 6 converted in
/// `fs_snapshot_adapter`, carrying `literal.exists()` and `candidate.exists()`
/// over six directories, and neither round 6 nor round 7 touched it — **because
/// it is on the WRITE path and every list was a list of readers.**
///
/// `Ok(None)` and `Err` are now different answers, and that distinction is the
/// whole fix: the caller's `unwrap_or_else(|| hearth.join(artifact_path))`
/// fallback could not tell "this artifact is not here" from "I could not look",
/// so an unreadable per-kind directory manufactured a phantom location and
/// `create_dir_all` made it real.
fn locate_artifact_dir(
    hearth: &PathBuf,
    artifact_path: &str,
) -> Result<Option<PathBuf>, TransitionEventWriteError> {
    use anvil_core::domain::playbook::fs_probe::{node_kind, NodeKind};
    // Engine-boundary containment (defense in depth): refuse any escaping path
    // before joining. Mirrors `fs_snapshot_adapter::locate_artifact_dir`.
    if crate::containment::escapes_hearth(hearth, artifact_path) {
        return Ok(None);
    }
    let probe = |p: &std::path::Path| -> Result<NodeKind, TransitionEventWriteError> {
        node_kind(p).map_err(|e| TransitionEventWriteError::IoError {
            message: format!(
                "artifact_location_uninspectable: {} could not be inspected: {e}. Refusing to \
                 file a governance transition without knowing where the artifact is — a \
                 manufactured location writes the event into a phantom artifact and returns Ok.",
                p.display()
            ),
        })
    };
    let literal = hearth.join(artifact_path);
    if probe(&literal)? != NodeKind::Absent {
        return Ok(Some(literal));
    }
    const ARTIFACT_DIRS: &[&str] = &[
        "tracks",
        "proposals",
        "milestones",
        "initiatives",
        "decisions",
        "learnings",
        "backlog_items",
    ];
    for dir in ARTIFACT_DIRS {
        let candidate = hearth.join(dir).join(artifact_path);
        if probe(&candidate)? != NodeKind::Absent {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// The random filename component. `pub` so the K8 preparation allocates its
/// event filename ONCE through the SAME convention and stores it in the journal
/// manifest (plan Task 4) — recovery replays that exact name, never a new one.
pub fn short_random_id() -> String {
    let mut buf = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

/// A high-resolution, write-time timestamp used only for the filename prefix
/// (causal tiebreak). Nanosecond precision so back-to-back transitions in the
/// same wall-clock second still get a monotonic, sortable prefix — without
/// reading any sibling file.
/// `pub` for the same single-sourced K8 preallocation reason as
/// [`short_random_id`]: the causal same-second tie-break must be identical for
/// events written by the generic adapter and by a prepared K8 transition.
pub fn hi_res_prefix() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.9fZ")
        .to_string()
}

impl TransitionEventWritePort for FileSystemTransitionEventAdapter {
    fn append_transition_event(
        &self,
        artifact_path: &str,
        record: &TransitionRecord,
    ) -> Result<(), TransitionEventWriteError> {
        // Engine-boundary containment (defense in depth): a path that escapes
        // the hearth subtree is refused HERE, never written — even the absent-
        // dir fallback join must not land outside the hearth.
        if crate::containment::escapes_hearth(&self.hearth_path, artifact_path) {
            return Err(TransitionEventWriteError::IoError {
                message: format!("artifact_path escapes the hearth: {}", artifact_path),
            });
        }
        // The artifact dir must already exist (it is created at artifact
        // creation).
        //
        // C-d.1 round 8, H-3. This used to read:
        //
        //     let artifact_dir = locate_artifact_dir(..)
        //         .unwrap_or_else(|| self.hearth_path.join(artifact_path));
        //     ...
        //     std::fs::create_dir_all(&events_dir)?;   // <-- CREATES it
        //
        // with a comment saying the fallback existed "so an absent dir surfaces
        // as an I/O error at write time rather than silently picking the wrong
        // location". **The line below it falsified the line above it**:
        // `create_dir_all` manufactured the fallback, so nothing ever surfaced.
        //
        // Measured on unmutated `63df2ff`, one `append_transition_event` per row:
        //
        //   tracks/ mode | result | events in the REAL transitions/ | in a MANUFACTURED one
        //   0755 / 0500 / 0300 |  Ok  |  1  |  0     <- controls
        //   0600               |  Ok  |  0  |  1   *** filed into a phantom artifact ***
        //   0000               |  Ok  |  0  |  1   ***
        //
        // The engine is told the transition was persisted; the artifact's own
        // history never receives it; a phantom `<hearth>/<id>/` directory now
        // exists in the hearth root for the scans to pick up. Composed with H-1
        // — the read side folding the REAL directory — the artifact resolves to
        // its stale prior state while the write reported success.
        let artifact_dir = locate_artifact_dir(&self.hearth_path, artifact_path)?.ok_or_else(
            || TransitionEventWriteError::IoError {
                message: format!(
                    "artifact_not_found: no artifact directory for '{}' under {}. A governance \
                     transition is not filed into a location this adapter invented.",
                    artifact_path,
                    self.hearth_path.display()
                ),
            },
        )?;
        let events_dir = artifact_dir.join(TRANSITIONS_DIR);

        std::fs::create_dir_all(&events_dir).map_err(|e| TransitionEventWriteError::IoError {
            message: format!("Failed to create transitions dir: {}", e),
        })?;

        let file_name = event_file_name(&hi_res_prefix(), record, &short_random_id());
        let file = events_dir.join(file_name);

        let serialized =
            serde_yaml::to_string(record).map_err(|e| TransitionEventWriteError::IoError {
                message: format!("Failed to serialize transition event: {}", e),
            })?;

        crate::atomic_write::atomic_write(&file, serialized.as_bytes()).map_err(|e| {
            TransitionEventWriteError::IoError {
                message: format!("Failed to write transition event: {}", e),
            }
        })
    }
}
