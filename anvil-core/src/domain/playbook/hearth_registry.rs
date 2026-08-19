//! `HearthPlaybookRegistry` — on-disk adapter for the `PlaybookRegistry` port.
//!
//! Reads `forge/playbooks/*/machine.yaml` from a hearth directory at construction
//! time, keying each loaded `PlaybookMachine` by its `kind` field.
//!
//! ## Always-reload policy (spec R3)
//!
//! Per-call construction IS the always-reload mechanism. The engine constructs a
//! fresh `HearthPlaybookRegistry` on each `describe` RPC call. Each construction
//! performs a full scan of `{hearth_path}/playbooks/*/machine.yaml`. A legacy
//! `{hearth_path}/playbooks/` directory is RENAMED to `playbooks/` once, on
//! first touch (see `new`); it is never scanned in place.
//!
//! THE BOTH-DIRECTORIES CASE IS NOW A REFUSAL, not a silent no-op. It used to
//! be: the migration was guarded by `if !canonical.exists() && legacy.exists()`,
//! so a hearth carrying BOTH was left alone — nothing renamed, `playbooks/`
//! never scanned, and every definition under it VANISHED SILENTLY from this
//! registry. It is now a `PlaybookLoadError::HearthDirectoryCollision` naming
//! both roots and listing every shadowed definition, and
//! [`registration_blocked`](HearthPlaybookRegistry::registration_blocked)
//! answers "may a caller write here" with it. This means
//! every `describe` call reflects the current on-disk state — no restart, no
//! rebuild. The `machine_for` method itself returns a reference into `self.map`;
//! there is no per-call I/O after construction.
//!
//! ## Excluded directories, and why they are tracked
//!
//! A directory under the canonical root that the scan did NOT resolve (no
//! `machine.yaml`, unparseable, dropped by an authoring gate) used to be a bare
//! `continue`. It is now recorded in `self.excluded` and carried into
//! [`projection`](HearthPlaybookRegistry::projection), because a projection that
//! silently omits a directory is how an operator concludes a definition was
//! never there. The projection is therefore derived from THIS registry's loaded
//! set and THIS registry's exclusions — not from a second directory listing
//! computed somewhere else that could disagree with what actually loaded.
//!
//! ## Load-error channel
//!
//! Malformed `machine.yaml` files do not fail construction. Errors are
//! accumulated in `self.errors` and exposed via `invalid_artifacts()`. Artifacts
//! that fail to load are simply absent from `self.map`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::playbook::fs_probe;
use crate::domain::playbook::load_error::PlaybookLoadError;
use crate::domain::playbook::loader::{load_from_yaml_with, LoaderEnforcement};
use crate::domain::playbook::registry_projection::{
    self, ExcludedDirectory, ProjectedDefinition,
};
use crate::domain::playbook::registry::{
    playbook_generation_aliases, PlaybookRegistry, PlaybookSource,
};
use crate::domain::playbook::types::PlaybookMachine;

/// On-disk `PlaybookRegistry` backed by `machine.yaml` files in the hearth.
///
/// Constructed via `new(hearth_path)`. All disk I/O happens during construction;
/// `machine_for` returns references into the populated `map`.
pub struct HearthPlaybookRegistry {
    hearth_path: PathBuf,
    /// Resolved machines keyed by `kind` field from each `machine.yaml`.
    map: HashMap<String, PlaybookMachine>,
    /// Maps each loaded machine's `kind` to its on-disk artifact directory name
    /// (the `playbook_id` used for hook-body reads, canonical-first under
    /// `{hearth}/playbooks/`). Note the reader is broader than this registry:
    /// `fs_hearth_reader.rs`'s `read_playbook_machine_yaml` and
    /// `list_playbook_hooks` fall back to `{hearth}/playbooks/` for BOTH
    /// `machine.yaml` and `hooks/` when the canonical path is absent.
    kind_to_id: HashMap<String, String>,
    /// Errors accumulated during construction (malformed files, I/O failures).
    errors: Vec<PlaybookLoadError>,
    /// Directories present under the canonical root that the scan did NOT
    /// resolve into a registered machine, with the reason. Feeds `projection`.
    excluded: Vec<ExcludedDirectory>,
}

impl HearthPlaybookRegistry {
    /// Construct a registry by scanning `{hearth_path}/playbooks/*/machine.yaml`.
    ///
    /// A legacy `{hearth_path}/workflows/` directory is migrated by a one-time
    /// rename to `playbooks/` before the scan, so existing hearths remain
    /// readable. Nothing is scanned from the legacy location itself, and the
    /// rename is skipped entirely when `playbooks/` already exists — see the
    /// both-directories caveat in the module doc.
    ///
    /// Construction always succeeds. Malformed artifacts are accumulated in
    /// `self.errors` and excluded from `self.map`.
    pub fn new(hearth_path: PathBuf) -> Self {
        Self::new_with_enforcement(hearth_path, LoaderEnforcement::OFF)
    }

    /// Measurement-enforcing variant of [`new`](Self::new).
    ///
    /// Identical scan, but every `machine.yaml` is loaded through
    /// `loader::load_from_yaml_enforcing` instead of `load_from_yaml`: a
    /// machine with a measured state lacking `success_criteria`, or with no
    /// `outcome_predicate`, fails to load and is surfaced via
    /// `invalid_artifacts()` (`PlaybookLoadError::MeasurementDefinitionMissing`)
    /// instead of registering.
    ///
    /// This is the DARK-GATED loader-side enforcement path — this pure
    /// library reads no environment, so the caller (the anvil-engine binary)
    /// decides whether to call this or `new` based on
    /// `ANVIL_ENFORCE_MEASUREMENT_DEFINITION`. Do not call this
    /// unconditionally until the hearth fleet is backfilled with per-step
    /// `success_criteria` + `outcome_predicate`s — otherwise every
    /// unbackfilled machine silently drops out of the registry.
    pub fn new_enforcing(hearth_path: PathBuf) -> Self {
        Self::new_with_enforcement(hearth_path, LoaderEnforcement::MEASUREMENT)
    }

    /// General constructor honoring an explicit [`LoaderEnforcement`] — the
    /// loader-side counterpart to `execute_enforcing_with` on the persist
    /// handler. `new` (⇒ `OFF`) and `new_enforcing` (⇒ `MEASUREMENT`) reduce to
    /// this exactly. The engine resolves BOTH dark-gate flags
    /// (`ANVIL_ENFORCE_MEASUREMENT_DEFINITION` + `ANVIL_ENFORCE_EVIDENCE_OBLIGATION`)
    /// into one enforcement config and constructs the registry through here
    /// (T-EEC-1 P4), so the loader/registry seam obeys the obligation gate in
    /// lockstep with the persist and intake seams. This pure library reads no
    /// environment — the engine binary owns the flag reads.
    ///
    /// Every `machine.yaml` is loaded through `load_from_yaml_with(enforcement)`:
    /// under measurement enforcement a machine failing the DEFINE block is
    /// surfaced via `invalid_artifacts()`
    /// (`PlaybookLoadError::MeasurementDefinitionMissing`); under obligation
    /// enforcement a DRIVEN measured pair lacking an obligation
    /// (`EvidenceObligationMissing`) or a FREE machine declaring one
    /// (`EvidenceObligationOnFreeRegister`) is surfaced the same way instead of
    /// registering. Do NOT enable either flag until the fleet is backfilled —
    /// otherwise unbackfilled machines silently drop out of the registry.
    pub fn new_with_enforcement(hearth_path: PathBuf, enforcement: LoaderEnforcement) -> Self {
        let (map, kind_to_id, errors, excluded) = Self::load_all(&hearth_path, enforcement);
        // Make measurement-enforcement drops LOUD. Without this, a machine that
        // fails the measurement-definition gate vanishes from the registry
        // silently: its `begin()`/`catalog` calls fail downstream with no trace,
        // which reads as "no traffic" rather than "this kind was dropped"
        // (confirmed root-cause of a live proposal/milestone/spark outage). Emit a
        // startup-visible WARNING per dropped machine naming the exact missing
        // pieces. Only measurement drops are diagnosed here (the helper matches
        // `MeasurementDefinitionMissing`); it is a no-op when measurement is off.
        if enforcement.measurement_definition {
            Self::warn_dropped_under_enforcement(&hearth_path, &errors);
        }
        HearthPlaybookRegistry {
            hearth_path,
            map,
            kind_to_id,
            errors,
            excluded,
        }
    }

    /// Emit a startup-visible WARNING for every machine that measurement
    /// enforcement dropped, naming the machine and the exact missing pieces
    /// (count of measurements lacking `success_criteria`, whether the
    /// `outcome_predicate` is absent/blank) plus the loader's first-failure
    /// detail. Runs only under enforcement and only iterates the (rare) drop
    /// set, so it re-reads just the offending machines to compute counts.
    fn warn_dropped_under_enforcement(hearth_path: &Path, errors: &[PlaybookLoadError]) {
        for err in errors {
            let PlaybookLoadError::MeasurementDefinitionMissing {
                artifact_id,
                detail,
            } = err
            else {
                continue;
            };
            let (missing_criteria, missing_predicate, kind) =
                Self::diagnose_measurement_gaps(hearth_path, artifact_id);
            eprintln!(
                "[hearth_registry] WARNING: measurement enforcement DROPPED playbook '{}'{}: \
                 {} measurement(s) missing success_criteria{}. First failure: {}. \
                 This kind will NOT register — begin()/catalog for it fail until backfilled.",
                artifact_id,
                kind.map(|k| format!(" (kind '{}')", k)).unwrap_or_default(),
                missing_criteria,
                if missing_predicate {
                    "; outcome_predicate missing/blank"
                } else {
                    ""
                },
                detail,
            );
        }
    }

    /// Re-parse a single dropped machine to count exactly what enforcement is
    /// missing: the number of `measurement_by_role` specs with an empty/absent
    /// `success_criteria`, whether the `outcome_predicate.terminal_state` is
    /// blank/absent, and the machine's `kind` (for the operator-facing message).
    /// Best-effort: an unparseable machine returns `(0, false, None)` and the
    /// caller falls back to the loader's first-failure detail.
    fn diagnose_measurement_gaps(
        hearth_path: &Path,
        artifact_id: &str,
    ) -> (usize, bool, Option<String>) {
        let yaml_path = hearth_path
            .join("playbooks")
            .join(artifact_id)
            .join("machine.yaml");
        let Ok(text) = std::fs::read_to_string(&yaml_path) else {
            return (0, false, None);
        };
        let Ok(machine) = serde_yaml::from_str::<PlaybookMachine>(&text) else {
            return (0, false, None);
        };
        let missing_criteria = machine
            .states
            .iter()
            .flat_map(|s| s.measurement_by_role.values())
            .filter(|spec| {
                spec.success_criteria
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            })
            .count();
        let missing_predicate = machine
            .outcome_predicate
            .as_ref()
            .map(|p| p.terminal_state.trim().is_empty())
            .unwrap_or(true);
        (missing_criteria, missing_predicate, Some(machine.kind.clone()))
    }

    /// Scan `{hearth_path}/playbooks/*/machine.yaml` and load each file.
    ///
    /// Returns a `(map, errors)` pair. Successful loads are added to the map
    /// keyed by the machine's `kind` field. Failed loads are accumulated in
    /// `errors`. Duplicate `kind` registrations are reported as a
    /// `PlaybookLoadError::DuplicateKindRegistration`.
    ///
    /// `enforcement` selects the authoring gates run by `load_from_yaml_with`
    /// for every file in the scan (see
    /// [`new_with_enforcement`](Self::new_with_enforcement)).
    fn load_all(
        hearth_path: &Path,
        enforcement: LoaderEnforcement,
    ) -> (
        HashMap<String, PlaybookMachine>,
        HashMap<String, String>,
        Vec<PlaybookLoadError>,
        Vec<ExcludedDirectory>,
    ) {
        let mut map: HashMap<String, PlaybookMachine> = HashMap::new();
        let mut kind_to_id: HashMap<String, String> = HashMap::new();
        let mut errors: Vec<PlaybookLoadError> = Vec::new();
        let mut excluded: Vec<ExcludedDirectory> = Vec::new();

        // One-time data migration: move a legacy `workflows/` hearth dir to
        // `playbooks/` before scanning (idempotent; foundry's registrar does the
        // same — whichever component touches the hearth first migrates it).
        //
        // C9, FAIL LOUD. This used to `eprintln!` a warning and CONTINUE, then
        // scan canonical only. The consequence was not cosmetic: a failed move
        // left Foundry writing definitions into a legacy location this scanner
        // does not read, and the only trace was a log line. The move's failure is
        // now recorded as a load error, so the caller sees it in
        // `invalid_artifacts()` instead of inferring it from stderr.
        //
        // The scan still proceeds so a partially-migrated hearth reports
        // EVERYTHING that is wrong rather than only the first thing — but the
        // error is carried, and `registration_blocked()` is how a caller asks
        // whether it may write. That function now EXISTS; when this comment
        // first shipped it named a function that was never written, which is the
        // failure mode a doc comment is worst at surfacing.
        //
        // Two distinct hearth-directory failures reach this channel, and they
        // are kept distinct because they demand different operator actions: a
        // failed MOVE is retryable, a COLLISION needs a merge decision.
        //
        // C-d.1 round 5, M-1: THE ROUND-4 "GUARD AHEAD OF THE MUTATION" IS GONE,
        // because it was a TAUTOLOGY and nothing could detect its absence.
        //
        // It read `match hearth_root_diagnosis(..) { Err(e) => push(e), Ok(_) =>
        // migrate(..) }`. But `migrate_legacy_hearth_dir` IS
        // `hearth_root_diagnosis` followed by its consequence — it calls the
        // diagnosis first and propagates its `Err` — so the outer match was
        // behaviorally identical to calling `migrate` directly on EVERY input,
        // plus one redundant filesystem read. The reviewer measured exactly that
        // (reverting it left anvil-core 1362/1362 green), and re-measuring a
        // structure that cannot fail is not coverage of it.
        //
        // The property the removed code claimed — "the guard is evaluated before
        // the mutation is authorized" — is not lost: it is where it always
        // actually was, INSIDE `migrate_legacy_hearth_dir`, whose refusing
        // branches all return before the `rename`, and which is pinned by
        // `The hearth-root guard answers a legacy-only hearth without moving it`.
        // A guard whose deletion nothing can detect is decoration; the honest
        // move is to delete it and keep the one that carries the property.
        if let Err(e) = registry_projection::migrate_legacy_hearth_dir(hearth_path) {
            errors.push(Self::hearth_root_error(e));
        }
        for directory_name in [registry_projection::CANONICAL_HEARTH_DIR] {
            Self::load_directory(
                hearth_path,
                directory_name,
                &mut map,
                &mut kind_to_id,
                &mut errors,
                &mut excluded,
                enforcement,
            );
        }

        (map, kind_to_id, errors, excluded)
    }

    /// Map a hearth-ROOT failure onto the load-error channel, keeping the three
    /// operator actions distinct: a failed MOVE is retryable, a COLLISION needs
    /// a merge decision, and an UNREADABLE-or-non-directory root needs the
    /// filesystem fixed before either question can even be asked.
    fn hearth_root_error(
        e: registry_projection::RegistryProjectionError,
    ) -> PlaybookLoadError {
        use registry_projection::RegistryProjectionError as E;
        let detail = e.to_string();
        match e {
            E::HearthDirectoryCollision { .. } => {
                PlaybookLoadError::HearthDirectoryCollision { detail }
            }
            E::HearthRootUnreadable { .. } | E::HearthRootNotADirectory { .. } => {
                PlaybookLoadError::HearthRootUnreadable { detail }
            }
            _ => PlaybookLoadError::HearthDirectoryMoveFailed { detail },
        }
    }

    fn load_directory(
        hearth_path: &Path,
        directory_name: &str,
        map: &mut HashMap<String, PlaybookMachine>,
        kind_to_id: &mut HashMap<String, String>,
        errors: &mut Vec<PlaybookLoadError>,
        excluded: &mut Vec<ExcludedDirectory>,
        enforcement: LoaderEnforcement,
    ) {
        let playbooks_dir = hearth_path.join(directory_name);
        let dir_iter = match std::fs::read_dir(&playbooks_dir) {
            Ok(iter) => iter,
            // An ABSENT root is an empty registry and no error — a hearth with
            // no definitions is a real, ordinary state.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            // C-d.1 round 4. Every OTHER `read_dir` failure used to return here
            // too, so a canonical root that exists and cannot be enumerated —
            // unreadable permissions, a file where a directory belongs, a stale
            // mount — produced an EMPTY REGISTRY with zero diagnostic and
            // `registration_blocked() == None`. The registry then reported that
            // this hearth serves nothing, which is indistinguishable from a
            // hearth that genuinely holds nothing, and a writer was cleared to
            // write into it.
            Err(e) => {
                errors.push(PlaybookLoadError::HearthRootUnreadable {
                    detail: format!(
                        "hearth_root_unreadable: {} EXISTS and could not be enumerated: {e}. \
                         An empty registry is not the same answer as an unreadable root.",
                        playbooks_dir.display()
                    ),
                });
                return;
            }
        };

        for entry in dir_iter {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    errors.push(PlaybookLoadError::HearthRootUnreadable {
                        detail: format!(
                            "hearth_root_unreadable: an entry under {} could not be read: {e}. \
                             A partial listing is a complete-looking answer to an incomplete \
                             question.",
                            playbooks_dir.display()
                        ),
                    });
                    return;
                }
            };
            let artifact_dir = entry.path();
            // C-d.1 round 5. This was `artifact_dir.is_dir()`. `read_dir`
            // SUCCEEDING and the per-entry `stat` FAILING is what a canonical
            // root at mode `0600`/`0400` does, and what a stale network mount
            // does: every entry silently left the scan and the registry reported
            // that this hearth serves nothing — indistinguishable from a hearth
            // that genuinely holds nothing, with `registration_blocked() == None`
            // clearing a writer.
            match fs_probe::node_kind(&artifact_dir) {
                Ok(fs_probe::NodeKind::Directory) => {}
                Ok(_) => continue,
                Err(e) => {
                    errors.push(PlaybookLoadError::HearthRootUnreadable {
                        detail: format!(
                            "hearth_root_unreadable: {} could not be inspected: {e}. An entry that \
                             cannot be inspected is not an entry that is not there.",
                            artifact_dir.display()
                        ),
                    });
                    return;
                }
            }

            let artifact_id = artifact_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unknown>".to_string());

            let machine_yaml_path = artifact_dir.join("machine.yaml");
            // C-d.1 round 5: was `machine_yaml_path.exists()`. "Absent" and "I
            // could not tell" were the same token, and the first one demotes the
            // directory into a benign exclusion while the second must block.
            match fs_probe::node_kind(&machine_yaml_path) {
                Ok(fs_probe::NodeKind::Absent) => {
                    // Recorded, not swallowed: this directory is present on disk
                    // and the loader resolved nothing from it. The projection
                    // states it.
                    excluded.push(ExcludedDirectory {
                        id: artifact_id,
                        reason: "no machine.yaml — the loader resolved no definition".to_string(),
                    });
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    errors.push(PlaybookLoadError::HearthRootUnreadable {
                        detail: format!(
                            "hearth_root_unreadable: {} could not be inspected: {e}. A definition \
                             that cannot be inspected is not a definition that is absent.",
                            machine_yaml_path.display()
                        ),
                    });
                    return;
                }
            }

            // Read the file; I/O failures are surfaced as YamlParseError.
            let yaml_text = match std::fs::read_to_string(&machine_yaml_path) {
                Ok(text) => text,
                Err(e) => {
                    errors.push(PlaybookLoadError::YamlParseError {
                        artifact_id: artifact_id.clone(),
                        line: 0,
                        column: 0,
                        message: e.to_string(),
                    });
                    excluded.push(ExcludedDirectory {
                        id: artifact_id.clone(),
                        reason: format!("machine.yaml unreadable: {e}"),
                    });
                    continue;
                }
            };

            // C-d.1 round 6, M-1. This listing used to swallow its own read
            // failure into an empty vec, so an unreadable `hooks/` made the
            // loader reject the machine for referencing a hook FILE THAT IS ON
            // DISK. The failure is now propagated, and it blocks registration
            // for the same reason every other uninspectable input on this path
            // does: the loaded set is not the set on disk.
            let hook_file_names = match Self::hook_file_names(&artifact_dir) {
                Ok(names) => names,
                Err(e) => {
                    errors.push(PlaybookLoadError::HearthRootUnreadable {
                        detail: format!(
                            "hearth_root_unreadable: the hooks directory under {} could not be \
                             listed: {e}. A hook directory that cannot be read is not a hook \
                             directory that is empty — reading it as empty rejects the machine \
                             for naming a hook that is present, which sends an operator to fix a \
                             machine.yaml that is correct.",
                            artifact_dir.display()
                        ),
                    });
                    return;
                }
            };
            let load_result =
                load_from_yaml_with(&artifact_id, &yaml_text, &hook_file_names, enforcement);
            match load_result {
                Ok(machine) => {
                    // Graph contiguity gate (registration seam): a machine that
                    // parses but can't flow (unreachable state, non-terminal
                    // dead-end, or terminal-unreachable trap) never registers —
                    // it is surfaced as an invalid artifact instead.
                    if let Err(e) =
                        crate::domain::playbook::loader::validate_contiguity(&machine, &artifact_id)
                    {
                        excluded.push(ExcludedDirectory {
                            id: artifact_id.clone(),
                            reason: e.to_string(),
                        });
                        errors.push(e);
                        continue;
                    }
                    let kind = machine.kind.clone();
                    // Duplicate kind detection.
                    if let Some(existing) = map.get(&kind) {
                        let existing_id = existing.kind.clone();
                        errors.push(PlaybookLoadError::DuplicateKindRegistration {
                            artifact_ids: format!("{},{}", existing_id, artifact_id),
                            kind: kind.clone(),
                        });
                        // Keep the first registration; skip the duplicate — and
                        // SAY SO in the projection: the skipped directory is on
                        // disk and serves nothing.
                        excluded.push(ExcludedDirectory {
                            id: artifact_id.clone(),
                            reason: format!(
                                "duplicate kind {kind:?} — an earlier directory already registered it"
                            ),
                        });
                    } else {
                        kind_to_id.insert(kind.clone(), artifact_id.clone());
                        map.insert(kind, machine);
                    }
                }
                Err(e) => {
                    excluded.push(ExcludedDirectory {
                        id: artifact_id.clone(),
                        reason: e.to_string(),
                    });
                    errors.push(e);
                }
            }
        }
    }

    fn hook_file_names(artifact_dir: &Path) -> std::io::Result<Vec<String>> {
        // Delegate to the single canonical regular-file hook-listing filter so
        // registry construction, the persist preflight, and the write-boundary
        // race reload agree byte-for-byte on which hooks a machine resolves.
        fs_probe::list_hook_files(&artifact_dir.join("hooks"))
    }

    /// Return the load errors accumulated during construction.
    ///
    /// Callers can inspect this to surface malformed artifact diagnostics without
    /// needing to panic or abort construction.
    /// The on-disk artifact id the loader resolved a governed kind FROM.
    ///
    /// C9 needs this: `playbooks.md` is built from the LOADED SET, and a loaded
    /// machine knows its `kind` but not the directory it came from. Without this
    /// the only way to name the directory is to list it — which is exactly the
    /// listing-derived projection C9 forbids.
    pub fn artifact_id_for_kind(&self, kind: &str) -> Option<String> {
        self.kind_to_id.get(kind).cloned()
    }

    pub fn invalid_artifacts(&self) -> &[PlaybookLoadError] {
        &self.errors
    }

    /// The hearth-DIRECTORY failure that blocks writing to this hearth, if any.
    ///
    /// THIS IS THE FUNCTION THE MODULE DOC HAS NAMED SINCE C9 AND THAT DID NOT
    /// EXIST. A caller about to write a definition asks this first: a `Some`
    /// means the canonical root is not the root this hearth's definitions will
    /// be read from, so writing produces an artifact nothing serves.
    ///
    /// Only the hearth-LEVEL variants block. A single malformed `machine.yaml`
    /// does NOT — that artifact is invalid, the hearth is fine.
    ///
    /// C-d.1 round 4 adds the third: a root that EXISTS and cannot be
    /// enumerated. It blocks for the same reason the other two do — the loaded
    /// set is not the set on disk, so a write produces an artifact nothing
    /// serves — and it is the case that previously produced an empty registry
    /// and a cleared writer.
    pub fn registration_blocked(&self) -> Option<&PlaybookLoadError> {
        self.errors.iter().find(|e| {
            matches!(
                e,
                PlaybookLoadError::HearthDirectoryMoveFailed { .. }
                    | PlaybookLoadError::HearthDirectoryCollision { .. }
                    | PlaybookLoadError::HearthRootUnreadable { .. }
            )
        })
    }

    /// Directories under the canonical root the scan did not resolve.
    pub fn excluded_directories(&self) -> &[ExcludedDirectory] {
        &self.excluded
    }

    /// The LOADED SET as the projection consumes it: one entry per registered
    /// machine, carrying the on-disk basename it was loaded from and its
    /// governed kind. Derived from `self.map` + `self.kind_to_id` — the same
    /// state `machine_for` answers from, so the projection cannot describe a
    /// registry other than this one.
    pub fn loaded_definitions(&self) -> Vec<ProjectedDefinition> {
        let mut out: Vec<ProjectedDefinition> = self
            .map
            .values()
            .map(|m| ProjectedDefinition {
                id: self
                    .kind_to_id
                    .get(&m.kind)
                    .cloned()
                    .unwrap_or_else(|| m.kind.clone()),
                governed_kind: m.kind.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Render the canonical `playbooks.md` body for THIS registry.
    ///
    /// The production home of `registry_projection::build_projection`. The
    /// projection is a function of what this registry loaded and what it
    /// excluded — never of a directory listing taken separately, which is the
    /// whole point of C9 and the reason this method exists on the registry
    /// rather than beside it.
    ///
    /// Writing the rendered body to `{hearth}/playbooks.md` on the LIVE hearth
    /// is deliberately not done here: that is a hearth data change bound by the
    /// cutover gate (`NG-DATA-CUTOVER`). Rendering is not.
    pub fn projection(&self) -> String {
        registry_projection::build_projection(&self.loaded_definitions(), &self.excluded)
    }

    /// The artifact ids that measurement enforcement dropped from the registry
    /// (`MeasurementDefinitionMissing`). Distinct from [`invalid_artifacts`]
    /// (all load errors) so callers can surface the enforcement-specific drop
    /// set — the diagnosable signal an unbackfilled kind produces instead of an
    /// invisible begin-failure.
    pub fn measurement_dropped_artifacts(&self) -> Vec<&str> {
        self.errors
            .iter()
            .filter_map(|e| match e {
                PlaybookLoadError::MeasurementDefinitionMissing { artifact_id, .. } => {
                    Some(artifact_id.as_str())
                }
                PlaybookLoadError::EvidenceObligationMissing { .. }
                | PlaybookLoadError::EvidenceObligationOnFreeRegister { .. } => None,
                _ => None,
            })
            .collect()
    }
}

impl PlaybookRegistry for HearthPlaybookRegistry {
    fn machine_for<'a>(&'a self, kind: &str) -> Option<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        self.map
            .get(kind)
            .or_else(|| playbook_generation_aliases(kind).iter().find_map(|k| self.map.get(*k)))
    }

    fn playbook_id_for(&self, kind: &str) -> Option<String> {
        self.kind_to_id
            .get(kind)
            .or_else(|| playbook_generation_aliases(kind).iter().find_map(|k| self.kind_to_id.get(*k)))
            .cloned()
    }

    fn source_for(&self, kind: &str) -> Option<PlaybookSource> {
        self.kind_to_id
            .get(kind)
            .or_else(|| playbook_generation_aliases(kind).iter().find_map(|k| self.kind_to_id.get(*k)))
            .map(|playbook_id| PlaybookSource {
                hearth: Some(self.hearth_path.clone()),
                playbook_id: playbook_id.clone(),
            })
    }

    fn all_machines<'a>(&'a self) -> Vec<&'a PlaybookMachine>
    where
        Self: 'a,
    {
        // `self.map` holds only successfully-loaded machines; malformed
        // artifacts live in `self.errors` and are absent here by construction
        // (AC9: active candidacy = registry-resolvability).
        self.map.values().collect()
    }

    /// The port-level view of [`Self::registration_blocked`], which is what a
    /// production WRITER holds. Same predicate, same two variants; this is the
    /// path that makes the block a block instead of a report.
    fn registration_blocked_detail(&self) -> Option<String> {
        self.registration_blocked().map(|e| e.to_string())
    }
}
