//! The pure gate-check decision — the hard pre-mutation gate.
//!
//! At runtime a harness's pre-tool hook invokes `anvil-hooks gate-check`, which
//! must decide ALLOW or BLOCK before a mutation lands on a forge artifact. The
//! decision logic lives here, kept PURE and free of the stdin/exit/engine-client
//! plumbing so it is unit/brine-testable in isolation: given the edited file's
//! enclosing forge artifact (state + playbook kind), the `hard_enforce` policy,
//! and whether the actor has an open begin session, it returns a [`Verdict`].
//!
//! The fail-OPEN posture is load-bearing: the gate must NEVER wedge a user's
//! editor. Any error resolving the artifact or the begin status degrades to
//! [`Verdict::Allow`].

use std::path::{Path, PathBuf};

/// The gate-check verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Permit the mutation (default / fail-open).
    Allow,
    /// Refuse the mutation — a hard-enforced kind without an open begin session.
    Block,
}

/// Harness-specific posture when a governed target cannot be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Existing harness behavior: resolution uncertainty must not wedge editing.
    FailOpen,
    /// Codex's hard lane: uncertainty on a governed mutation blocks.
    CodexFailClosed,
}

impl FailurePolicy {
    pub fn on_resolution_failure(self) -> Verdict {
        match self {
            FailurePolicy::FailOpen => Verdict::Allow,
            FailurePolicy::CodexFailClosed => Verdict::Block,
        }
    }
}

impl Verdict {
    /// The lowercase wire string ("allow" | "block").
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Block => "block",
        }
    }
}

/// The resolved enclosing forge artifact for an edited file: the artifact
/// directory (the nearest ancestor containing `status.yaml`, within the hearth),
/// plus its current `state` and playbook `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    /// Absolute path to the artifact directory holding `status.yaml`.
    pub artifact_dir: PathBuf,
    /// The artifact directory relative to the hearth root (the form the engine
    /// `begin_adoption_status` RPC expects as `artifact_path`).
    pub relative_path: String,
    pub kind: String,
    pub state: String,
}

/// Whether the artifact's kind is a HARD pre-mutation gate under `hard_enforce`.
pub fn is_hard_enforced(kind: &str, hard_enforce: &[String]) -> bool {
    hard_enforce.iter().any(|k| k == kind)
}

/// The pure gate decision.
///
/// `artifact` is the resolved enclosing forge artifact, or `None` when the edited
/// path is not inside a forge artifact (→ ALLOW, not a gate target).
///
/// `begin_status` carries whether the actor has an open begin session:
///   - `Some(true)`  — open begin → ALLOW
///   - `Some(false)` — no open begin → BLOCK only when hard-enforced
///   - `None`        — the begin lookup ERRORED → fail-OPEN (ALLOW)
///
/// BLOCK iff the kind is hard-enforced AND `begin_status == Some(false)`.
/// Everything else ALLOWs.
pub fn decide(
    artifact: Option<&ResolvedArtifact>,
    hard_enforce: &[String],
    begin_status: Option<bool>,
) -> Verdict {
    decide_with_policy(
        artifact,
        hard_enforce,
        begin_status,
        FailurePolicy::FailOpen,
    )
}

pub fn decide_with_policy(
    artifact: Option<&ResolvedArtifact>,
    hard_enforce: &[String],
    begin_status: Option<bool>,
    failure_policy: FailurePolicy,
) -> Verdict {
    let Some(artifact) = artifact else {
        // Not a forge artifact — never a gate target.
        return Verdict::Allow;
    };

    if !is_hard_enforced(&artifact.kind, hard_enforce) {
        // Soft-tier kind: cooperative warn only, never a hard refuse. Codex
        // fails closed on uncertainty within a hard lane; it does not promote
        // kinds excluded from the manifest's hard-enforcement policy.
        return Verdict::Allow;
    }

    match begin_status {
        // Open begin session present → the mutation is adopted → allow.
        Some(true) => Verdict::Allow,
        // No open begin on a hard-enforced kind → the gate fires.
        Some(false) => Verdict::Block,
        // Harness policy controls uncertainty: general lanes allow, Codex blocks.
        None => failure_policy.on_resolution_failure(),
    }
}

/// Walk up from `edited_path` to the nearest ancestor directory containing a
/// `status.yaml`, bounded to within `hearth_root`. Returns the artifact directory
/// or `None` when the path is not inside any forge artifact under the hearth.
///
/// Pure over the filesystem: the only side effect is the `status.yaml` existence
/// probe. The walk stops at `hearth_root` (inclusive) so an edit OUTSIDE the
/// hearth, or directly in the hearth root, resolves to `None`.
pub fn find_enclosing_artifact_dir(edited_path: &Path, hearth_root: &Path) -> Option<PathBuf> {
    let hearth_root = hearth_root
        .canonicalize()
        .unwrap_or_else(|_| hearth_root.to_path_buf());
    let start = edited_path
        .canonicalize()
        .unwrap_or_else(|_| edited_path.to_path_buf());

    // Begin at the edited path's directory if it is a file, else the path itself.
    let mut current: Option<&Path> = if start.is_file() {
        start.parent()
    } else {
        Some(start.as_path())
    };

    while let Some(dir) = current {
        // Stay confined: never walk above the hearth root.
        if !dir.starts_with(&hearth_root) {
            return None;
        }
        if dir.join("status.yaml").is_file() {
            return Some(dir.to_path_buf());
        }
        if dir == hearth_root {
            return None;
        }
        current = dir.parent();
    }
    None
}

/// The `status.yaml` `kind` and resolved `state` for an artifact directory,
/// parsed via the shared [`crate::domain::status::FullStatusYaml`] schema. Returns
/// `None` when the file is absent/unreadable/unparseable or carries no kind/state
/// (→ the caller fails OPEN).
pub fn read_artifact_kind_and_state(artifact_dir: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(artifact_dir.join("status.yaml")).ok()?;
    let parsed: crate::domain::status::FullStatusYaml = serde_yaml::from_str(&text).ok()?;
    let kind = parsed.kind.clone()?;
    // C-d.1 round 8, H-1 (forced touch, and DECLARED as a residual). This whole
    // function fails OPEN by design — its own doc says so, and the `status.yaml`
    // read two lines above has always swallowed with `.ok()?`. Making the event
    // read fallible without changing the function's contract means an unreadable
    // event store lands in the SAME fail-open arm the unreadable status.yaml
    // already lands in. That is not a regression and it is not a fix: a hook
    // gate that fails open on unreadable governance evidence is the H-2 shape at
    // a different surface, and it belongs to the adapter-sweep track with the
    // rest of the fail-open audit. Recorded in implementation-c.md §41.
    let state = crate::domain::transition_log::resolve_state_with_events(&parsed, artifact_dir)
        .ok()
        .flatten()?;
    Some((kind, state))
}

/// Resolve the enclosing forge artifact for an edited path: walk up to the
/// nearest `status.yaml` within the hearth, then read its kind + state. Returns
/// `None` when the path is not inside a forge artifact (→ ALLOW).
pub fn resolve_artifact(edited_path: &Path, hearth_root: &Path) -> Option<ResolvedArtifact> {
    let artifact_dir = find_enclosing_artifact_dir(edited_path, hearth_root)?;
    let (kind, state) = read_artifact_kind_and_state(&artifact_dir)?;
    let canonical_root = hearth_root
        .canonicalize()
        .unwrap_or_else(|_| hearth_root.to_path_buf());
    let relative_path = artifact_dir
        .strip_prefix(&canonical_root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| artifact_dir.to_string_lossy().to_string());
    Some(ResolvedArtifact {
        artifact_dir,
        relative_path,
        kind,
        state,
    })
}

/// Resolve the open-begin status for `artifact` from the on-disk event log via
/// the [`crate::ports::query_port::QueryPort`] reader the engine also uses. This
/// is the SAME predicate the engine's `begin_adoption_status` RPC evaluates
/// (`has_open_begin` / `has_any_open_begin` over the artifact's `activity:` and
/// `transitions:` event store), so the gate's decision can never disagree with
/// the engine; it just reads the source of truth directly, daemon-free.
///
/// When `actor` is `Some`, the lookup is actor-scoped (open iff THAT actor has an
/// unclosed begin in the artifact's state). When `actor` is `None` (the harness
/// supplied no anvil identity), it is actor-AGNOSTIC (open iff ANY actor does).
///
/// Returns `Some(open)` on a successful read, or `None` on a read error — which
/// the [`decide`] fail-open rule maps to ALLOW.
pub fn resolve_begin_status_from_disk(
    query: &dyn crate::ports::query_port::QueryPort,
    artifact: &ResolvedArtifact,
    actor: Option<&str>,
) -> Option<bool> {
    // Read the log WITH its degradation diagnostics (not just the entries): a
    // dropped begin marker must not read as "no open begin" the way a clean
    // empty log does.
    let log = query.read_activity_log(&artifact.relative_path).ok()?;
    let transitions = query.read_transitions(&artifact.relative_path).ok()?;
    let open = match actor {
        Some(actor) => crate::domain::begin_adoption::has_open_begin(
            &log.entries,
            &transitions,
            actor,
            &artifact.state,
        ),
        None => crate::domain::begin_adoption::has_any_open_begin(
            &log.entries,
            &transitions,
            &artifact.state,
        ),
    };
    // CONSERVATIVE on a DEGRADED log: if the SURVIVING entries show no open
    // begin BUT one or more entries were dropped as malformed, that dropped
    // entry may have been the actor's OPEN begin marker. Returning a confident
    // `Some(false)` here would BLOCK a hard-enforced edit and nudge the actor
    // into a DUPLICATE begin for a session that had, in fact, already begun.
    // Downgrade to the uncertain path (`None` → the caller's fail-OPEN ALLOW,
    // same as a read error) so we never wedge the editor on a log we cannot
    // trust. A surviving open marker (`open == true`) is authoritative and is
    // returned as-is — degradation only ever softens a "closed" verdict, never
    // an "open" one.
    if !open && log.is_degraded() {
        return None;
    }
    Some(open)
}

/// The end-to-end runtime gate: resolve the enclosing artifact, classify it
/// against `hard_enforce`, resolve the open-begin status from disk, and decide.
/// A non-artifact path short-circuits to ALLOW without any begin lookup. Pure
/// over the filesystem (the only effects are the `status.yaml`/event-log reads);
/// fail-OPEN on every error path.
pub fn gate_check(
    query: &dyn crate::ports::query_port::QueryPort,
    edited_path: &Path,
    hearth_root: &Path,
    hard_enforce: &[String],
    actor: Option<&str>,
) -> Verdict {
    let Some(artifact) = resolve_artifact(edited_path, hearth_root) else {
        return Verdict::Allow;
    };
    // Skip the begin lookup entirely for soft kinds (never a gate target).
    if !is_hard_enforced(&artifact.kind, hard_enforce) {
        return Verdict::Allow;
    }
    let begin_status = resolve_begin_status_from_disk(query, &artifact, actor);
    decide(Some(&artifact), hard_enforce, begin_status)
}
