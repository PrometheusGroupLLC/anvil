use anvil_core::domain::playbook::fs_probe::{self, DirListing, NodeKind};
use anvil_core::domain::shared_types::{ActivityEntry, RegistryEntry};
use anvil_core::domain::status::FullStatusYaml;
use anvil_core::ports::query_port::{OriginTurnArtifact, QueryError, QueryPort};
use std::path::{Path, PathBuf};

/// Filesystem implementation of `QueryPort`.
///
/// Read methods are lifted verbatim from `FileSystemBeginAdapter` (lines
/// 37–170, 275–465 of `fs_begin_adapter.rs`). The original adapter is NOT
/// deleted here — Phase 5 removes it once all consumers have migrated.
///
/// `check_projection_row_unique` is lifted verbatim from
/// `fs_begin_adapter.rs:405-451` to preserve exact error message text so
/// `begin_error_projection_row_ambiguous.feature` assertions survive
/// unchanged post-Phase-3.
#[derive(Clone)]
pub struct FileSystemQueryAdapter {
    hearth_path: PathBuf,
}

impl FileSystemQueryAdapter {
    pub fn new(hearth_path: PathBuf) -> Self {
        Self { hearth_path }
    }
}

/// Parse a registry entry whose first link's href contains the target
/// artifact id. Returns the first-link text as `track_name` and the
/// second-link text (if present) as `proposal_name`.
///
/// Lifted verbatim from `fs_begin_adapter.rs:497-545`.
fn parse_registry_entry(content: &str, artifact_id: &str) -> Option<RegistryEntry> {
    let id_marker_slash = format!("/{}/", artifact_id);
    let id_marker_paren = format!("/{})", artifact_id);
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- [") {
            continue;
        }
        if !(line.contains(&id_marker_slash) || line.contains(&id_marker_paren)) {
            continue;
        }
        let mut links = Vec::new();
        let mut rest = line;
        while let Some(open_text) = rest.find('[') {
            let after_open = &rest[open_text + 1..];
            let close_text = match after_open.find(']') {
                Some(i) => i,
                None => break,
            };
            let text = &after_open[..close_text];
            let after_text = &after_open[close_text + 1..];
            if !after_text.starts_with('(') {
                rest = after_text;
                continue;
            }
            let after_open_paren = &after_text[1..];
            // Balanced-paren scan: artifact ids may contain embedded '(' and ')'
            // (e.g. an id ending `…_(warn_then_enforce;_experiment_gates_as_eval_cases)`).
            // A naive `.find(')')` would stop at the first ')' inside the id,
            // truncating the href so the id-marker checks fail. Instead we
            // track paren depth (starting at 1 for the already-consumed '(')
            // and stop at the ')' that returns depth to zero.
            let close_paren = {
                let mut depth: usize = 1;
                let mut found = None;
                for (i, ch) in after_open_paren.char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                found = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                match found {
                    Some(i) => i,
                    None => break,
                }
            };
            let href = &after_open_paren[..close_paren];
            links.push((text.to_string(), href.to_string()));
            rest = &after_open_paren[close_paren + 1..];
        }
        if let Some((track_text, first_href)) = links.first() {
            if !(first_href.contains(&id_marker_slash) || first_href.contains(&id_marker_paren)) {
                continue;
            }
            let proposal_text = links.get(1).map(|(t, _)| t.clone()).unwrap_or_default();
            return Some(RegistryEntry {
                track_name: track_text.clone(),
                proposal_name: proposal_text,
            });
        }
    }
    None
}

const KIND_DIRS: &[(&str, &str)] = &[
    ("track", "tracks"),
    ("proposal", "proposals"),
    ("milestone", "milestones"),
    ("initiative", "initiatives"),
    ("decision", "decisions"),
    ("learning", "learnings"),
    ("backlog_item", "backlog_items"),
];

/// Locate the directory for an artifact regardless of its kind.
/// First tries artifact_id as a literal relative path (e.g. "tracks/foo"),
/// then falls back to searching per-kind prefix directories (e.g. "foo" → "tracks/foo"),
/// and finally — for domain-machine kinds whose directory is NOT in the legacy
/// `KIND_DIRS` list — scans the immediate subdirectories of the hearth for a
/// child dir named `<artifact_id>` that contains a `status.yaml`. The scan is
/// registry-free (no per-kind code) and is bounded by the `status.yaml` +
/// exact-id requirement so it cannot match an unrelated directory.
///
/// **C-d.1 round 7, H-1.** Every filesystem answer on this path is now fallible.
/// This function is the `QueryPort` twin of `fs_snapshot_adapter`'s function of
/// the same name, doing the same job, which round 6 converted — and it carried
/// all four historical spellings of the swallow unremediated while its sibling
/// one seam over was fixed. Measured on unmutated `579c7b3`, with `tracks/` at
/// mode `0600` and `tracks/T_alpha/status.yaml` on disk in every row:
/// `read_artifact_kind` answered `NotFound`, `read_artifact_state` answered
/// `NotFound`, and `find_artifact_by_kind_origin_turn` answered `Ok(None)` —
/// which is `begin`'s idempotency gate saying *"no artifact exists for this
/// turn"* over one that does, and permitting a duplicate write.
fn locate_artifact_dir(hearth: &Path, artifact_id: &str) -> Result<Option<PathBuf>, QueryError> {
    // 1) Treat as a literal relative path first (supports "tracks/foo" style ids).
    let literal = hearth.join(artifact_id);
    if uninspectable_or_kind(&literal)? != NodeKind::Absent {
        return Ok(Some(literal));
    }
    // 2) Fall back to searching per-kind directories by bare id.
    for (_, dir) in KIND_DIRS {
        let candidate = hearth.join(dir).join(artifact_id);
        if uninspectable_or_kind(&candidate)? != NodeKind::Absent {
            return Ok(Some(candidate));
        }
    }
    // 3) Scan all immediate subdirectories of the hearth for a `<dir>/<id>`
    //    that holds a `status.yaml` (covers domain-machine directories like
    //    `knowledge/` that are not enumerated in KIND_DIRS).
    scan_subdirs_for_artifact(hearth, artifact_id)
}

/// Scan immediate subdirectories of `hearth` for `<subdir>/<artifact_id>/status.yaml`.
/// Returns the artifact directory on the first match. Registry-free directory
/// discovery for non-legacy kinds.
///
/// **C-d.1 round 7, H-1.** Was `read_dir(hearth).ok()?` + `entries.flatten()` +
/// `subdir.is_dir()` + `candidate.join("status.yaml").exists()` — the round-3
/// signature, the dropped per-entry iteration error, the round-4 signature and
/// the round-5 signature, in eleven lines. The listing is now taken once, at the
/// edge, already inspected, so the loop holds facts rather than a root.
fn scan_subdirs_for_artifact(
    hearth: &Path,
    artifact_id: &str,
) -> Result<Option<PathBuf>, QueryError> {
    for entry in listing(hearth)? {
        if entry.kind != NodeKind::Directory {
            continue;
        }
        let candidate = entry.path.join(artifact_id);
        if uninspectable_or_kind(&candidate.join("status.yaml"))? != NodeKind::Absent {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// [`fs_probe::node_kind`] with its `io::Error` mapped onto this channel's "I
/// could not look" answer.
///
/// `IoError` and not `NotFound`: the engine maps `QueryError::NotFound` to gRPC
/// NOT_FOUND — *this artifact is not in this hearth* — and `IoError` to
/// INTERNAL. They are different facts with different operator actions, and
/// collapsing the second into the first is what told an operator that an
/// artifact on disk was absent. Deliberately the same message shape as
/// `fs_snapshot_adapter::uninspectable_or_kind`, because an operator meeting
/// this on either port is meeting the same fact.
fn uninspectable_or_kind(path: &Path) -> Result<NodeKind, QueryError> {
    fs_probe::node_kind(path).map_err(|e| QueryError::IoError {
        message: format!(
            "artifact_lookup_uninspectable: {} exists and could not be inspected: {e}. This is \
             NOT the same answer as \"there is no such artifact\" — reporting it as not-found \
             sends an operator looking for something that is on disk in front of them.",
            path.display()
        ),
    })
}

/// [`fs_probe::list_dir`] with its `io::Error` mapped onto this channel.
///
/// The message names the ONE consequence that makes this the blocking half of
/// the class: a hearth enumeration that comes back short reads as a registry
/// that is genuinely that size, and nothing downstream can tell the difference.
fn listing(dir: &Path) -> Result<DirListing, QueryError> {
    fs_probe::list_dir(dir).map_err(|e| QueryError::IoError {
        message: format!(
            "hearth_scan_uninspectable: failed to enumerate '{}': {e}. An enumeration that cannot \
             be completed is NOT an empty one — answering with the entries that happened to be \
             readable hands back a short list that looks like the whole hearth.",
            dir.display()
        ),
    })
}

impl QueryPort for FileSystemQueryAdapter {
    fn list_artifacts(&self) -> Result<Vec<(String, String)>, QueryError> {
        // Registry-free enumeration: scan the hearth's immediate subdirectories
        // (tracks/, proposals/, knowledge/, …) for child dirs holding a
        // status.yaml, and read each artifact's authoritative `kind:`. Mirrors
        // the scan in `find_artifact_by_kind_origin_turn` but yields every
        // artifact as `(id, kind)`.
        //
        // C-d.1 round 7, H-1. This used to say "a subdir we cannot read is
        // skipped rather than failing the whole enumeration (best-effort, so one
        // corrupt artifact never blocks the resume lookup)", and it was the
        // purest instance of the class on the track: with the hearth ROOT at
        // mode 0600 and two artifacts on disk, this method answered `Ok([])` —
        // an unreadable root reading as an EMPTY registry, with no error and no
        // diagnostic. With `tracks/` at 0600 it answered `Ok` with the OTHER
        // artifact and silently dropped the track. Best-effort is the right
        // posture for an answer about CONTENT (a status.yaml that does not
        // parse is genuinely malformed, and that skip is kept below); it is the
        // wrong posture for an answer about REACHABILITY, because the caller
        // cannot tell a hearth that holds two artifacts from a hearth it could
        // not look inside.
        //
        // L4: the plan specified enumerating the registry *.md files. The
        // status-dir scan is the chosen equivalent — and is in fact stronger:
        // registries only list driven/well-known kinds, whereas the status.yaml
        // is the single source of truth for an artifact's `kind`/`state` and
        // exists for EVERY artifact (including domain-machine kinds whose dir is
        // not a registry, e.g. knowledge/). Scanning status dirs therefore
        // enumerates the same set the resume lookup needs without a registry
        // round-trip, and never misses a kind a registry would not enumerate.
        let mut out: Vec<(String, String)> = Vec::new();
        for entry in listing(&self.hearth_path)? {
            if entry.kind != NodeKind::Directory {
                continue;
            }
            for artifact_entry in listing(&entry.path)? {
                let status_path = artifact_entry.path.join("status.yaml");
                if uninspectable_or_kind(&status_path)? == NodeKind::Absent {
                    continue;
                }
                let content =
                    std::fs::read_to_string(&status_path).map_err(|e| QueryError::IoError {
                        message: format!(
                            "artifact_status_uninspectable: {} exists and could not be read: {e}. \
                             Skipping it would hand back a catalog that is short by one artifact \
                             and complete-looking.",
                            status_path.display()
                        ),
                    })?;
                // A status.yaml that does not PARSE, or that carries no `kind:`,
                // is a readable answer about content — that skip is the
                // best-effort this method is entitled to, and it is not the
                // class. The reads above are answers about REACHABILITY.
                let Ok(status) = serde_yaml::from_str::<FullStatusYaml>(&content) else {
                    continue;
                };
                let Some(kind) = status.kind else {
                    continue;
                };
                out.push((artifact_entry.name, kind));
            }
        }
        // Deterministic order for stable multiplicity tie-breaking + assertions.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn read_artifact_kind(&self, artifact_id: &str) -> Result<String, QueryError> {
        // 1) If the id starts with a known kind prefix, map it directly (fast path
        //    for the 6 legacy kinds).
        for (kind, dir) in KIND_DIRS {
            let prefix = format!("{}/", dir);
            if artifact_id.starts_with(&prefix) {
                return Ok(kind.to_string());
            }
        }
        // 2) Fall back to filesystem search by bare id over the legacy dirs.
        //
        // C-d.1 round 7, H-1. Was `candidate.exists()`, over SIX directories. An
        // uninspectable per-kind root demoted itself to "the artifact is not
        // under this kind" and the search moved on as though it had looked —
        // ending, after (3) also failed for the same reason, in `NotFound` for
        // an artifact whose status.yaml is on disk.
        for (kind, dir) in KIND_DIRS {
            let candidate = self.hearth_path.join(dir).join(artifact_id);
            if uninspectable_or_kind(&candidate)? != NodeKind::Absent {
                return Ok(kind.to_string());
            }
        }
        // 3) Domain-machine kinds: locate the artifact dir (literal path or a
        //    scan-for-status.yaml across all hearth subdirs) and read the
        //    authoritative `kind:` from its status.yaml. No per-kind code.
        if let Some(dir) = locate_artifact_dir(&self.hearth_path, artifact_id)? {
            let status_path = dir.join("status.yaml");
            let content =
                std::fs::read_to_string(&status_path).map_err(|e| QueryError::IoError {
                    message: format!("Failed to read status.yaml for '{}': {}", artifact_id, e),
                })?;
            let status: FullStatusYaml =
                serde_yaml::from_str(&content).map_err(|e| QueryError::MalformedStatus {
                    artifact_id: artifact_id.to_string(),
                    message: format!("Invalid YAML: {}", e),
                })?;
            if let Some(kind) = status.kind {
                return Ok(kind);
            }
        }
        Err(QueryError::NotFound {
            artifact_id: artifact_id.to_string(),
        })
    }

    fn read_artifact_state(&self, artifact_id: &str) -> Result<String, QueryError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_id)?.ok_or_else(|| {
            QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            }
        })?;
        let status_path = dir.join("status.yaml");
        let content = std::fs::read_to_string(&status_path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read status.yaml for '{}': {}", artifact_id, e),
        })?;
        let status: FullStatusYaml =
            serde_yaml::from_str(&content).map_err(|e| QueryError::MalformedStatus {
                artifact_id: artifact_id.to_string(),
                message: format!("Invalid YAML: {}", e),
            })?;
        // Resolve via the shared seam, FOLDING the per-file transition event
        // directory with the legacy array (dual-read). Only genuinely-
        // unresolvable status.yaml (no state AND no transitions, legacy or
        // event) still errors.
        // C-d.1 round 8, H-1. Damaged event evidence is an I/O refusal, not a
        // malformed status: the status.yaml here parsed perfectly. Reporting
        // "your status.yaml is missing 'state'" for a file that is correct is
        // byte-for-byte the collapse MUT-S6 fixed in `read_full_status`, and it
        // was still live one frame down this same call.
        anvil_core::domain::transition_log::resolve_state_with_events(&status, &dir)
            .map_err(|e| QueryError::IoError {
                message: format!(
                    "artifact_state_uninspectable: the transition event evidence for '{}' could \
                     not be read: {e}. An enumeration that cannot be completed is NOT an empty \
                     one — folding it away resolves the artifact to a STALE state and reports it \
                     as fact.",
                    artifact_id
                ),
            })?
            .ok_or_else(|| QueryError::MalformedStatus {
                artifact_id: artifact_id.to_string(),
                message: "Missing 'state' field".to_string(),
            })
    }

    fn read_artifact_status(&self, artifact_id: &str) -> Result<FullStatusYaml, QueryError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_id)?.ok_or_else(|| {
            QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            }
        })?;
        let status_path = dir.join("status.yaml");
        let content = std::fs::read_to_string(&status_path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read status.yaml for '{}': {}", artifact_id, e),
        })?;
        serde_yaml::from_str(&content).map_err(|e| QueryError::MalformedStatus {
            artifact_id: artifact_id.to_string(),
            message: format!("Invalid YAML: {}", e),
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

        // C-d.1 round 7, H-1. This is `begin`'s IDEMPOTENCY GATE
        // (`begin.rs:554`): `Ok(None)` means "no artifact exists for this turn"
        // and `begin` goes on to create one. Its `read_dir` calls were already
        // fallible — and behind them sat `status_path.exists()` and a
        // `read_to_string` that swallowed into `continue`, which is exactly the
        // round-4 half of the defect. Measured on unmutated `579c7b3` with
        // `tracks/T_alpha/status.yaml` (kind: track, origin_turn: turn-1) ON
        // DISK and `tracks/` at 0600: `Ok(None)` — the gate cleared a DUPLICATE
        // WRITE. At 0000 the same fixture answered `IoError`, which is what
        // proves the 0600 answer is a swallow and not a uniform failure.
        let mut matches = Vec::new();
        for entry in listing(&self.hearth_path)? {
            if entry.kind != NodeKind::Directory {
                continue;
            }
            let directory_name = entry.name;
            for artifact_entry in listing(&entry.path)? {
                let artifact_path = artifact_entry.path;
                let status_path = artifact_path.join("status.yaml");
                if uninspectable_or_kind(&status_path)? == NodeKind::Absent {
                    continue;
                }
                let content =
                    std::fs::read_to_string(&status_path).map_err(|e| QueryError::IoError {
                        message: format!(
                            "origin_turn_status_uninspectable: {} exists and could not be read: \
                             {e}. This lookup is begin's idempotency gate — skipping an \
                             unreadable status.yaml answers \"no artifact exists for this turn\" \
                             over one that does, and clears a duplicate write.",
                            status_path.display()
                        ),
                    })?;
                let Ok(status) = serde_yaml::from_str::<FullStatusYaml>(&content) else {
                    continue;
                };
                if status.kind.as_deref() != Some(kind)
                    || status.origin_turn.as_deref() != Some(origin_turn)
                {
                    continue;
                }
                // C-d.1 round 8, H-1. This is `begin`'s idempotency gate: a
                // stale state here clears a DUPLICATE WRITE. Measured on
                // unmutated 63df2ff with `transitions/` at 0600, it answered
                // `Ok(tracks/<id>@implementing)` for an artifact that is
                // `reviewing`.
                let resolved = anvil_core::domain::transition_log::resolve_state_with_events(
                    &status,
                    &artifact_path,
                )
                .map_err(|e| QueryError::IoError {
                    message: format!(
                        "origin_turn_evidence_uninspectable: the transition events under {} could \
                         not be read: {e}. This gate clears a duplicate write; a history read \
                         SHORT resolves a stale state and clears one that should be refused.",
                        artifact_path.display()
                    ),
                })?;
                let Some(state) = resolved else {
                    return Err(QueryError::MalformedStatus {
                        artifact_id: status_path.display().to_string(),
                        message: "Missing 'state' field".to_string(),
                    });
                };
                matches.push(OriginTurnArtifact {
                    artifact_path: format!("{}/{}", directory_name, artifact_entry.name),
                    state,
                });
            }
        }
        matches.sort_by(|a, b| a.artifact_path.cmp(&b.artifact_path));
        Ok(matches.into_iter().next())
    }

    fn read_activity_entries(&self, artifact_id: &str) -> Result<Vec<ActivityEntry>, QueryError> {
        // Delegate to read_activity_log so EVERY filesystem activity read goes
        // through the one degraded-log boundary warning, and the survivors are
        // the same set the degradation-aware consumers see.
        Ok(self.read_activity_log(artifact_id)?.entries)
    }

    fn read_activity_log(
        &self,
        artifact_id: &str,
    ) -> Result<anvil_core::domain::shared_types::ActivityLog, QueryError> {
        let status = self.read_artifact_status(artifact_id)?;
        let log = status.activity.unwrap_or_default();
        // Adapter boundary: emit the path-aware warning the deserializer could
        // not (it has no path). Names the offending status.yaml so a degraded
        // begin-adoption log is observable in the engine's captured logs.
        if log.is_degraded() {
            if let Some(dir) = locate_artifact_dir(&self.hearth_path, artifact_id)? {
                log.warn_if_degraded(&dir.join("status.yaml"));
            }
        }
        Ok(log)
    }

    fn read_transitions(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<anvil_core::domain::status::StatusTransition>, QueryError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_id)?.ok_or_else(|| {
            QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            }
        })?;
        let status = self.read_artifact_status(artifact_id)?;
        anvil_core::domain::transition_log::resolve_transitions_with_events(&status, &dir).map_err(
            |e| QueryError::IoError {
                message: format!(
                    "transitions_uninspectable: the transition event evidence for '{}' could not \
                     be read: {e}. `Ok(0 transitions)` over a history on disk is an artifact's \
                     whole governance record reading as empty, with no error and no diagnostic.",
                    artifact_id
                ),
            },
        )
    }

    fn read_transitions_strict(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<anvil_core::domain::status::StatusTransition>, QueryError> {
        let dir = locate_artifact_dir(&self.hearth_path, artifact_id)?.ok_or_else(|| {
            QueryError::NotFound {
                artifact_id: artifact_id.to_string(),
            }
        })?;
        let status = self.read_artifact_status(artifact_id)?;
        // Fail closed on damaged event evidence: the adoption reset cannot be
        // proven safe if a governing transition might be hiding in an unreadable
        // or unparseable event file. `resolve_transitions_with_events` (lenient)
        // would silently skip the damage.
        anvil_core::domain::transition_log::resolve_transitions_with_events_strict(&status, &dir)
            .map_err(|e| QueryError::AdoptionEvidenceUnreadable {
                artifact_id: artifact_id.to_string(),
                detail: e.to_string(),
            })
    }

    fn read_artifact_text(&self, track_path: &str, filename: &str) -> Result<String, QueryError> {
        let full_path = self.hearth_path.join(track_path).join(filename);
        std::fs::read_to_string(&full_path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read '{}': {}", full_path.display(), e),
        })
    }

    fn read_carry_forward_if_present(
        &self,
        artifact_path: &str,
    ) -> Result<Option<String>, QueryError> {
        let full_path = self
            .hearth_path
            .join(artifact_path)
            .join("carry-forward.md");
        match std::fs::read_to_string(&full_path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(QueryError::IoError {
                message: format!("Failed to read '{}': {}", full_path.display(), e),
            }),
        }
    }

    fn read_op_log(
        &self,
        artifact_path: &str,
        target_document: &str,
    ) -> Result<anvil_core::domain::amendment::OpLog, QueryError> {
        let file = self.hearth_path.join(artifact_path).join(
            anvil_core::ports::op_log_write_port::op_log_file_name(target_document),
        );
        // C-d.1 round 7, H-1. Was `!file.exists()`. The op log is APPEND-ONLY
        // and it is the truth an amendment replay folds over: an unreadable log
        // answering `OpLog::new()` is "this document has never been amended",
        // which is a clean slate over a file that holds the amendments.
        // Measured on unmutated `579c7b3` with a one-op log on disk and the
        // artifact directory at 0600: `Ok(EMPTY)`.
        if uninspectable_or_kind(&file)? == NodeKind::Absent {
            return Ok(anvil_core::domain::amendment::OpLog::new());
        }
        let content = std::fs::read_to_string(&file).map_err(|e| QueryError::IoError {
            message: format!("Failed to read op log '{}': {}", file.display(), e),
        })?;
        serde_yaml::from_str(&content).map_err(|e| QueryError::MalformedStatus {
            artifact_id: artifact_path.to_string(),
            message: format!(
                "Invalid op log YAML for document '{}': {}",
                target_document, e
            ),
        })
    }

    fn read_context_file(&self, relative_path: &str) -> Result<String, QueryError> {
        // Context files live in the hearth's `context/` subdirectory,
        // matching the convention in FileSystemBeginAdapter.
        let full_path = self.hearth_path.join("context").join(relative_path);
        std::fs::read_to_string(&full_path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read context file '{}': {}", relative_path, e),
        })
    }

    fn read_playbook_hook_body(
        &self,
        playbook_id: &str,
        filename: &str,
    ) -> Result<String, QueryError> {
        // Reject path traversal attempts defensively — consistent with the
        // loader's validate_hook_filename posture.
        if filename.contains('/') || filename.contains("..") {
            return Err(QueryError::IoError {
                message: format!(
                    "Unsafe filename '{}': path components ('/', '..') are not allowed",
                    filename
                ),
            });
        }
        let full_path = self
            .hearth_path
            .join("playbooks")
            .join(playbook_id)
            .join("hooks")
            .join(filename);
        let legacy_path = self
            .hearth_path
            .join("workflows")
            .join(playbook_id)
            .join("hooks")
            .join(filename);
        // C-d.1 round 7, H-1. Was `full_path.exists()`. With `playbooks/<id>/
        // hooks/` at mode 0600 and `capture.md` ON DISK, the canonical probe
        // answered `false`, the read fell through to the LEGACY path, and the
        // operator got `Failed to read hook file 'capture.md' for playbook
        // 'pb_one': No such file or directory` — a no-such-file diagnostic,
        // naming neither path, for a hook file that is right there.
        let read_path = match uninspectable_or_kind(&full_path)? {
            NodeKind::Absent => legacy_path,
            _ => full_path,
        };
        std::fs::read_to_string(&read_path).map_err(|e| QueryError::IoError {
            message: format!(
                "Failed to read hook file '{}' for playbook '{}': {}",
                filename, playbook_id, e
            ),
        })
    }

    fn read_registry_entry(
        &self,
        registry_file: &str,
        artifact_id: &str,
    ) -> Result<RegistryEntry, QueryError> {
        let path = self.hearth_path.join(registry_file);
        let content = std::fs::read_to_string(&path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read {}: {}", registry_file, e),
        })?;
        parse_registry_entry(&content, artifact_id).ok_or_else(|| QueryError::IoError {
            message: format!(
                "No registry entry found for '{}' in {}",
                artifact_id, registry_file
            ),
        })
    }

    /// Lifted verbatim from `fs_begin_adapter.rs:405-451` to preserve
    /// exact error message text for feature-assertion fidelity.
    ///
    /// Message formats preserved:
    /// - 0 rows: `"No row matching '{}' in '{}' section of {}"`
    /// - N≥2 rows: `"{} rows matching '{}' in '{}' section of {} — projection has a duplicate-name collision; dedupe or use distinct track names before retrying"`
    fn check_projection_row_unique(
        &self,
        projection_file: &str,
        track_name: &str,
        from_section: &str,
    ) -> Result<(), QueryError> {
        let path = self.hearth_path.join(projection_file);
        let content = std::fs::read_to_string(&path).map_err(|e| QueryError::IoError {
            message: format!("Failed to read {}: {}", projection_file, e),
        })?;
        let lines: Vec<&str> = content.lines().collect();
        let section_pat = format!("## {} (", from_section);
        let section_idx = lines
            .iter()
            .position(|l| l.starts_with(&section_pat))
            .ok_or_else(|| QueryError::IoError {
                message: format!(
                    "Section '## {}' not found in {}",
                    from_section, projection_file
                ),
            })?;
        let row_pat = format!("| {} |", track_name);
        let mut count = 0;
        for l in lines.iter().skip(section_idx + 1) {
            if l.trim_start().starts_with("## ") {
                break;
            }
            if l.starts_with(&row_pat) {
                count += 1;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::parse_registry_entry;

    /// Verifies that `parse_registry_entry` correctly locates a registry entry
    /// whose artifact id contains an embedded `)` — the balanced-paren scan
    /// must consume the entire href rather than stopping at the first ')'.
    ///
    /// The artifact_id is the bare slug (not the full path), matching the
    /// form passed by `begin` via `request.identifier`. The registry href
    /// is `tracks/<id>/`; the id_marker_slash `/<id>/` is found inside it.
    #[test]
    fn parse_registry_entry_id_with_embedded_paren() {
        // Bare artifact id — the slash in `tracks/<id>/` provides the leading '/'.
        let artifact_id =
            "20260620T1200_experiment_gates_(warn_then_enforce;_experiment_gates_as_eval_cases)";
        let content = format!(
            "# Tracks\n\n## implementing\n\n- [Experiment Gates](tracks/{}/)\n",
            artifact_id
        );
        let entry = parse_registry_entry(&content, artifact_id)
            .expect("should find registry entry whose id contains an embedded ')'");
        assert_eq!(entry.track_name, "Experiment Gates");
    }

    /// Verifies that normal ids (no embedded parens) continue to parse correctly.
    #[test]
    fn parse_registry_entry_normal_id() {
        let artifact_id = "20260601T1000_simple_track";
        let content = format!(
            "# Tracks\n\n## implementing\n\n- [Simple Track](tracks/{}/)\n",
            artifact_id
        );
        let entry = parse_registry_entry(&content, artifact_id)
            .expect("should find registry entry for a normal id");
        assert_eq!(entry.track_name, "Simple Track");
    }

    /// Verifies that ids appearing within a two-link entry (track + proposal) still
    /// parse: the first link's href must match; the second link text becomes `proposal_name`.
    #[test]
    fn parse_registry_entry_two_link_line() {
        let artifact_id = "20260601T1000_simple_track";
        let content = "# Tracks\n\n## implementing\n\n- [Simple Track](tracks/20260601T1000_simple_track/) — some summary — [my-proposal](proposals/20260101T0000_my_proposal/)\n";
        let entry = parse_registry_entry(content, artifact_id)
            .expect("should find registry entry for a two-link line");
        assert_eq!(entry.track_name, "Simple Track");
        assert_eq!(entry.proposal_name, "my-proposal");
    }
}
