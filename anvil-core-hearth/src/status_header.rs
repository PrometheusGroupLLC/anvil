//! The `state:` header in an artifact's `status.yaml` is a **derived
//! projection of the transition event store**, kept equal to the fold on every
//! transition.
//!
//! # Why this module exists (the defect it closes)
//!
//! The transition-event upcast (`tracks/20260616T0514_transition_event_store_upcast`)
//! moved every transition into its own file under `<artifact>/transitions/` and
//! specced R8: *"A new transition MUST NOT rewrite a denormalized top-level
//! `status.yaml` `state:` field — that would reintroduce the shared-file
//! mutation R2 forbids."*
//!
//! R8 held for anvil's own readers — every one of them folds the event store
//! through `transition_log` — and broke for everybody else. The header keeps
//! its **creation-time** value forever, so an artifact 14 transitions into its
//! life still declares `state: vision` on disk. Measured on the real hearths at
//! the time of this fix: the a brand proposal was 14 transitions past its
//! header, the relay program was stale, and 7 of 10 sampled tracks were stale.
//! Every consumer that reads the file rather than calling the engine — the
//! tracker's drift panel, `domain::playbook::status_read` (the playbook atlas),
//! `grep`, a human opening the file — was reading a value that had not been
//! true for months.
//!
//! # Why R8's cost argument does not survive contact
//!
//! R8 traded a correct header for freedom from shared-file mutation. That
//! freedom was never actually purchased: **the same governed operations already
//! rewrite `status.yaml`.** `begin` appends an `activity:` marker through
//! `fs_activity_write_adapter`, and every transition upserts the actor block
//! through `fs_actor_write_adapter` (called from `SnapshotPort::seed_actor`,
//! beside `append_transition` in the very same handler). status.yaml is a
//! read-modify-write file on the transition path with or without the header.
//! R8 therefore paid no cost it avoided and bought a permanently-wrong file.
//!
//! What R8 got right and this module keeps: the **history** stays one-file-per-
//! event, so two concurrent transitions still never collide — they write two
//! distinct files. Only the one-line header is shared, and a conflict on it is
//! mechanically resolvable *by construction*: the header is a pure function of
//! `transitions/`, so [`reconcile_status_header`] recomputes the correct value
//! from the merged event set. A git merge that takes either side, or neither,
//! converges on the next transition or the next reconcile run.
//!
//! # Precedence is unchanged: evidence beats the header
//!
//! [`anvil_core::domain::transition_log::fold_state`] still resolves state from the
//! folded event history FIRST and falls back to the header only when there is
//! no transition at all. That ordering is deliberate and load-bearing here: a
//! hand-edited or merge-mangled header must be **detected and repaired**, never
//! trusted. The header is a projection, not a source.
//!
//! # Two callers, two authorities
//!
//! [`reconcile_status_header`] serves both the write path and the offline
//! backfill, and they are NOT the same job — see [`HeaderPolicy`]. The write
//! path just created the newest event, so it projects unconditionally. The
//! backfill knows nothing about how either record was made, so it removes only
//! provable staleness and reports the rest ([`HeaderVerdict::Unverifiable`]).
//! Collapsing the two would either freeze the header again or let an audit pass
//! rewrite a finished track backwards; both were measured on live hearths.

use anvil_core::domain::status::FullStatusYaml;
use anvil_core::domain::status_header::{declared_state_header, set_state_header};
use std::path::{Path, PathBuf};

/// The status.yaml filename, single-sourced for this module's callers.
pub const STATUS_FILE: &str = "status.yaml";

/// What a reconcile pass concluded about one artifact's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderVerdict {
    /// The header already says what the evidence says. Nothing to do.
    Agrees,
    /// The header disagrees with the evidence AND is safe to overwrite, because
    /// it is provably a STALE SNAPSHOT of a transition that really happened.
    ///
    /// Two ways to qualify:
    /// * there is no header at all — writing one materializes the answer the
    ///   engine already gives and destroys nothing; or
    /// * the artifact has a `transitions/` event store (engine-written, the
    ///   truth) AND the header's current value appears somewhere in the folded
    ///   history. That second clause is what makes overwriting safe: the header
    ///   is a value a transition once set, so the only thing lost is its
    ///   staleness.
    Drifted,
    /// The header disagrees with the evidence and CANNOT be adjudicated. Left
    /// exactly as found, reported for a human.
    ///
    /// # Two live cases, both found by running the backfill, both of which a
    /// naive "fold wins" reconciler would have made WORSE
    ///
    /// 1. **The header is ahead, in a record the fold cannot see.**
    ///    `foundry-hearth/tracks/20260702T2010_socket_activation_for_kit_shims`
    ///    declares `state: complete`; its hand-authored `transitions:` array
    ///    stops at `build`; the final state change was recorded only as an
    ///    `activity:` marker; there is NO `transitions/` directory. Trusting the
    ///    array would "repair" a finished track backwards to `build`. Two more
    ///    tracks in the same hearth were identical; 48 artifacts hearth-wide.
    ///
    /// 2. **The header names a state no transition ever set.**
    ///    `foundry-hearth/tracks/20260627T1607_connections_into_foundry_mcp_cli`
    ///    declares `state: abandoned` and holds exactly ONE event — the
    ///    creation seed to `spec`. `abandoned` appears nowhere in its history,
    ///    so it was set by something other than a transition. An events-exist
    ///    rule alone would have reverted a deliberately-abandoned track to
    ///    `spec`.
    ///
    /// Both reduce to one principle: **this tool removes staleness, it does not
    /// arbitrate.** It overwrites a header only where it can prove the header is
    /// an old value of the same log it is being replaced from. Anything else is
    /// a fact for a human, not a value to guess.
    Unverifiable,
}

/// What a reconcile pass did (or, in [`ReconcileMode::Report`], would do) to
/// one artifact's header.
///
/// Every field is populated on every outcome — a caller printing this has the
/// complete before/after without a second read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderReconciliation {
    /// The status.yaml that was inspected.
    pub path: PathBuf,
    /// The `state:` header as it read on disk BEFORE the pass. `None` when the
    /// file carries no top-level `state:` line at all.
    pub declared: Option<String>,
    /// The state the transition evidence resolves to.
    pub resolved: String,
    /// How many per-file transition events back that resolution. `0` means the
    /// resolution came from the legacy `status.yaml` array alone.
    pub event_count: usize,
    /// The pass's conclusion.
    pub verdict: HeaderVerdict,
    /// Whether bytes were actually written. Always `false` in
    /// [`ReconcileMode::Report`]; `true` in [`ReconcileMode::Apply`] exactly
    /// when the verdict is [`HeaderVerdict::Drifted`].
    pub written: bool,
}

/// Whether a reconcile pass writes or only reports.
///
/// The backfill defaults to [`ReconcileMode::Report`] — a repair of governance
/// files never happens as a side effect of looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileMode {
    /// Read and compare only. No byte is written.
    Report,
    /// Rewrite the header when the verdict says it may be.
    Apply,
}

/// How much authority the caller has to overwrite a header it disagrees with.
///
/// The two callers genuinely know different things, and collapsing them would
/// break one of them:
///
/// * The WRITE PATH has just appended a transition event. The fold is the truth
///   *by construction* — the caller created the newest fact in it — so the
///   header is simply re-projected. Anything else would leave a freshly
///   transitioned artifact declaring its old state, which is the whole defect.
/// * The OFFLINE AUDIT has no such knowledge. It is looking at two records made
///   by unknown means and must not arbitrate between them; it only removes
///   provable staleness. See [`HeaderVerdict::Unverifiable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderPolicy {
    /// The caller just recorded the newest transition: project the fold.
    Authoritative,
    /// Offline: overwrite only a header that is provably this log's stale value.
    Conservative,
}

/// Why a header could not be reconciled. Every variant is a REFUSAL — none of
/// them degrades into "the header is fine", because an unreadable artifact
/// silently reported as clean is precisely how this class of bug survives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusHeaderError {
    /// The artifact directory has no `status.yaml`.
    Missing { path: String },
    /// `status.yaml` exists and could not be read (permission, I/O). Distinct
    /// from [`Self::Malformed`]: this file may be perfectly well-formed.
    Unreadable { path: String, detail: String },
    /// `status.yaml` was read and is not parseable YAML.
    Malformed { path: String, detail: String },
    /// The `transitions/` event evidence could not be read. Folding it away
    /// would resolve the artifact to a stale state and then WRITE that stale
    /// state into the header, which is worse than leaving the header alone.
    EvidenceUnreadable { path: String, detail: String },
    /// Neither a header nor any transition — there is no state to project.
    Unresolvable { path: String },
    /// The repaired header could not be written back.
    WriteFailed { path: String, detail: String },
}

impl std::fmt::Display for StatusHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatusHeaderError::Missing { path } => {
                write!(f, "status_missing: no status.yaml at {path}")
            }
            StatusHeaderError::Unreadable { path, detail } => write!(
                f,
                "status_uninspectable: {path} exists and could not be read: {detail}. This is not \
                 a malformed status.yaml — it is one this process could not open."
            ),
            StatusHeaderError::Malformed { path, detail } => {
                write!(f, "status_malformed: {path} is not parseable YAML: {detail}")
            }
            StatusHeaderError::EvidenceUnreadable { path, detail } => write!(
                f,
                "transition_evidence_uninspectable: the transition events beside {path} could not \
                 be read: {detail}. Refusing to project a state header from evidence this process \
                 could not see."
            ),
            StatusHeaderError::Unresolvable { path } => write!(
                f,
                "state_unresolvable: {path} has neither a state: header nor any transition"
            ),
            StatusHeaderError::WriteFailed { path, detail } => {
                write!(f, "status_header_write_failed: {path}: {detail}")
            }
        }
    }
}

impl std::error::Error for StatusHeaderError {}

/// Bring one artifact's `state:` header back into agreement with its
/// transition evidence, reporting exactly what was (or would be) changed.
///
/// The resolved value comes from [`anvil_core::domain::transition_log::fold_state`]
/// over [`anvil_core::domain::transition_log::read_event_files`] — the same single
/// seam every engine read uses — so a repaired header can never disagree with
/// what the engine answers.
///
/// This is IDEMPOTENT and a pure function of what is on disk, which is what
/// makes it safe both as the write-path's second leg and as the standalone
/// backfill: running it twice, or after an interrupted transition, or after a
/// git merge that mangled the line, converges on the same value.
///
/// It OVERWRITES a present header only against engine-written evidence — a
/// non-empty `transitions/` event store. A legacy-array-only artifact whose
/// header disagrees comes back [`HeaderVerdict::Unverifiable`] and is left
/// exactly as found; see that variant for the live case that made this
/// mandatory.
pub fn reconcile_status_header(
    artifact_dir: &Path,
    mode: ReconcileMode,
    policy: HeaderPolicy,
) -> Result<HeaderReconciliation, StatusHeaderError> {
    let status_path = artifact_dir.join(STATUS_FILE);
    let display = status_path.display().to_string();

    let content = match std::fs::read_to_string(&status_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(StatusHeaderError::Missing { path: display })
        }
        Err(e) => {
            return Err(StatusHeaderError::Unreadable {
                path: display,
                detail: e.to_string(),
            })
        }
    };

    let status: FullStatusYaml =
        serde_yaml::from_str(&content).map_err(|e| StatusHeaderError::Malformed {
            path: display.clone(),
            detail: e.to_string(),
        })?;

    // Read the event store ONCE, and keep the count: it is what decides whether
    // a disagreement may be overwritten (see `HeaderVerdict::Unverifiable`).
    let events = anvil_core::domain::transition_log::read_event_files(artifact_dir).map_err(|e| {
        StatusHeaderError::EvidenceUnreadable {
            path: display.clone(),
            detail: e.to_string(),
        }
    })?;
    let event_count = events.len();
    let resolved = anvil_core::domain::transition_log::fold_state(&status, &events).ok_or_else(|| {
        StatusHeaderError::Unresolvable {
            path: display.clone(),
        }
    })?;

    let declared = declared_state_header(&content);
    let verdict = match declared.as_deref() {
        Some(d) if d == resolved => HeaderVerdict::Agrees,
        // No header at all: writing one destroys nothing.
        None => HeaderVerdict::Drifted,
        // The caller just wrote the newest event; the fold is the truth.
        Some(_) if policy == HeaderPolicy::Authoritative => HeaderVerdict::Drifted,
        Some(d) => {
            // Overwrite ONLY a header that is provably a stale snapshot of this
            // artifact's own transition log: engine-written evidence must exist
            // (`event_count > 0`) AND the current header value must appear in
            // that folded history. A header naming a state no transition ever
            // set was written by something else, and this tool does not
            // arbitrate between it and the log.
            let history = anvil_core::domain::transition_log::fold_transitions(
                &anvil_core::domain::transition_log::resolve_transitions(&status),
                &events,
            );
            let header_was_transitioned_to = history.iter().any(|t| t.to == d);
            if event_count > 0 && header_was_transitioned_to {
                HeaderVerdict::Drifted
            } else {
                HeaderVerdict::Unverifiable
            }
        }
    };

    let mut written = false;
    if verdict == HeaderVerdict::Drifted && mode == ReconcileMode::Apply {
        let repaired = set_state_header(&content, &resolved);
        crate::atomic_write::atomic_write(&status_path, repaired.as_bytes()).map_err(
            |e| StatusHeaderError::WriteFailed {
                path: display.clone(),
                detail: e.to_string(),
            },
        )?;
        written = true;
    }

    Ok(HeaderReconciliation {
        path: status_path,
        declared,
        resolved,
        event_count,
        verdict,
        written,
    })
}
