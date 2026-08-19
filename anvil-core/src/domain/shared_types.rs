//! Shared value types used across multiple domain handlers and ports.
//!
//! These types were originally colocated with `BeginPort` (retired in
//! Phase 5 of the core CQRS separation track). They now live here because
//! they are consumed by `ArtifactPort`, `SnapshotPort`, `ActorWritePort`,
//! `QueryPort`, the snapshot/complete/begin handlers, and the domain
//! event enum.

use crate::domain::playbook::types::{EvidenceClass, Role, Sensitivity};

/// One caller-claimed evidence item attached to a lifecycle state entry.
///
/// `reference` is deliberately opaque. The domain copies it verbatim and does
/// not parse, normalize, resolve, or dereference it. Keeping this carrier to
/// exactly the typed class and caller reference prevents raw request prose,
/// paths, identities, or referenced content from entering through this seam.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimedEvidence {
    pub class: EvidenceClass,
    pub reference: String,
}

/// Access context supplied by the caller for playbook routing/begin decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub org: String,
    pub space: Option<String>,
    pub role: Role,
    pub clearance: Sensitivity,
}

impl RequestContext {
    /// Default-deny-safe context used when older callers send no access context.
    pub fn default_safe() -> Self {
        Self {
            org: "Foundation".to_string(),
            space: None,
            role: Role::Read,
            clearance: Sensitivity::Internal,
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::default_safe()
    }
}

/// The data needed to write a status.yaml for a new track.
#[derive(Debug, Clone)]
pub struct StatusContent {
    pub version: u32,
    pub kind: String,
    pub state: String,
    /// Parent artifact id, regardless of parent kind. Stored on disk under
    /// the unified `parent_id:` YAML key across all artifact kinds. Legacy
    /// files written with the old per-kind keys (`proposal:` for tracks,
    /// `track:` for playbooks) are still read via serde aliases on
    /// `FullStatusYaml`. Empty when the artifact has no parent
    /// (provenance-optional kinds).
    pub parent_id: String,
    pub actor_name: String,
    pub actor_type: String,
    pub actor_model: String,
    pub actor_provider: String,
    pub actor_context_window: i64,
    pub actor_sdk_version: String,
    pub actor_entrypoint: String,
    pub actor_registered_at: String,
    /// Originating routed turn for idempotent implicit auto-begins. Empty for
    /// ordinary begin calls.
    pub origin_turn: String,
    pub transition_to: String,
    pub transition_at: String,
    pub transition_role: String,
    pub transition_approver: String,
    /// The owner descriptor (kit id or user/Space) the created instance is
    /// destined for. Recorded verbatim; empty when the kind does not carry
    /// one. Serialized to status.yaml (skip-if-empty) by the engine's
    /// `build_initial_artifact_status_yaml`. (Anvil-lane 1b.)
    pub target_owner: String,
    /// Generic machine-declared required fields recorded verbatim on the
    /// created instance. Only the machine's declared `required_fields` names
    /// land here (unrelated bag entries are filtered out at create time).
    /// BTreeMap for deterministic key order. Serialized to status.yaml
    /// (skip-if-empty) by the engine's `build_initial_artifact_status_yaml`.
    pub fields: std::collections::BTreeMap<String, String>,
}

/// A single transition block to append to an existing status.yaml.
/// Used when begin operates on an existing artifact (review / resume flows).
#[derive(Debug, Clone)]
pub struct TransitionContent {
    pub to: String,
    pub at: String,
    pub actor: String,
    pub role: String,
    pub approver: Option<String>,
    pub note: Option<String>,
    /// Optional reviewer satisfaction recorded on the transition (Slice C).
    /// `Some("address_in_next_step")` marks a carry-forward acceptance; `None`
    /// for every other transition — the YAML emitter omits the line so existing
    /// status.yaml transition records stay byte-identical.
    pub satisfaction: Option<String>,
    /// Optional machine-readable event-type discriminator. `Some("adoption")`
    /// marks a governance-adoption reset (an out-of-engine artifact taken back
    /// to its machine's initial state) so downstream consumers can filter it
    /// STRUCTURALLY rather than by parsing `note` prose: the begin-adoption
    /// close-by-comparison predicate (`has_open_begin`) skips an adoption
    /// transition so it never closes the doer's freshly-opened begin, and the
    /// step-measurement stream can tag/suppress the reset. `None` for every
    /// ordinary transition — the YAML emitter omits the line, so existing
    /// transition records stay byte-identical.
    pub event_type: Option<String>,
}

/// The fields of a registry-file entry (tracks.md, proposals.md, etc.)
/// needed by the review flow. Populated by parsing the markdown entry
/// whose first-link href ends with the target artifact id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The human-readable name rendered as the first link's text.
    /// For tracks.md: `- [review spec strand](tracks/...) — ...`
    /// yields `track_name: "review spec strand"`.
    pub track_name: String,
    /// The proposal name rendered as the second link's text, when
    /// present. Empty string if the entry has no proposal link.
    pub proposal_name: String,
}

/// A single entry in the artifact's `activity:` log — a sibling to
/// `actors:`/`transitions:` in status.yaml. Records that an actor entered
/// an artifact in a given state (a begin-marker), independent of the
/// state-machine `transitions:` history. Append-only: the begin-adoption
/// soft-warn and query infer "open" by comparing an entry's `at` against
/// later transitions by the same actor (no closing entry is written).
///
/// `kind` is carried (not implied) so a reserved future `resume` marker
/// appends with no schema change. `Option`-wrapped on `FullStatusYaml` so
/// every existing status.yaml (no `activity:` key) parses unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActivityEntry {
    /// Marker kind. Only "begin" ships in this slice; "resume" reserved.
    pub kind: String,
    /// The actor identity that entered the artifact.
    pub actor: String,
    /// The state the actor entered (the begin-marker's state-scoped key).
    pub state: String,
    /// Wall-clock timestamp stamped by the engine at routing time.
    pub at: String,
    /// The originating conversation id (resume-aware routing). Append-only
    /// addition: existing on-disk markers have no `conversation_id:` key and
    /// read back as the empty string (back-compat). Used to bridge a
    /// continuation message to the conversation's open playbook.
    #[serde(default)]
    pub conversation_id: String,
}

/// An artifact's `activity:` begin-marker log, carrying the successfully
/// parsed entries ALONGSIDE the diagnostics from the per-entry-tolerant parse
/// (see `FullStatusYaml`'s lenient `activity:` deserializer).
///
/// `dropped > 0` means the log is DEGRADED: one or more `activity:` entries
/// failed to deserialize and were skipped so a single malformed marker could
/// not sink the whole `status.yaml` parse (which would take down the shared
/// catalog/checkin scan). This distinction is load-bearing for begin-adoption:
/// a dropped entry may have been an OPEN begin marker, so a consumer that
/// infers "no open begin" from the surviving entries must treat a degraded log
/// CONSERVATIVELY rather than as a clean, truly-empty log — otherwise it could
/// silently nudge a DUPLICATE begin for a session that had, in fact, already
/// begun. The `dropped` count keeps that distinction observable to every
/// consumer instead of collapsing it into an indistinguishable short list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityLog {
    /// The successfully parsed begin-marker entries, in file order. Malformed
    /// entries are ABSENT (never coerced to a bogus empty-state entry) so
    /// `entry.state`/`entry.actor` folds never see a fabricated value.
    pub entries: Vec<ActivityEntry>,
    /// Count of `activity:` entries that failed per-entry deserialize and were
    /// dropped. `0` for a clean (or absent) log.
    pub dropped: usize,
    /// The first drop's diagnostic (`activity[<idx>]: <serde error>`, or a
    /// note that `activity:` was present but not a list). `None` when
    /// `dropped == 0`. Carried so the adapter boundary — which knows the file
    /// PATH the deserializer does not — can emit a path-aware warning.
    pub first_error: Option<String>,
}

impl ActivityLog {
    /// A clean (non-degraded) log wrapping already-parsed entries. Used by
    /// synthetic in-memory/test adapters whose entries are constructed rather
    /// than parsed, so no drop can occur.
    pub fn new(entries: Vec<ActivityEntry>) -> Self {
        Self {
            entries,
            dropped: 0,
            first_error: None,
        }
    }

    /// Whether any `activity:` entry was dropped as malformed — the log is a
    /// partial (degraded) view of the on-disk markers.
    pub fn is_degraded(&self) -> bool {
        self.dropped > 0
    }

    /// Emit a path-aware, persistently observable warning when the log is
    /// degraded. Called at the adapter boundary (which knows `status_path`;
    /// the deserializer does not) so the warning names the offending file and
    /// lands on the engine's stderr — captured by the Foundry supervisor's
    /// logs, matching the core convention for surfaced-but-non-fatal hearth
    /// problems (see `hearth_registry`'s migrate warning). A no-op on a clean
    /// log.
    pub fn warn_if_degraded(&self, status_path: &std::path::Path) {
        if !self.is_degraded() {
            return;
        }
        eprintln!(
            "anvil: WARN {} — dropped {} malformed activity {} (begin-adoption log DEGRADED); first: {}",
            status_path.display(),
            self.dropped,
            if self.dropped == 1 { "entry" } else { "entries" },
            self.first_error.as_deref().unwrap_or("(no detail)"),
        );
    }
}

/// Identity for an agent actor. Used to seed the `actors:` table of
/// an existing status.yaml when the engine records a transition for an
/// agent that has not previously touched this artifact.
///
/// Humans do not have configurations; `seed_actor` dispatches on the
/// `type` field — "agent" writes a configurations list, "human" writes
/// just `type: human`.
/// `Serialize`/`Deserialize` exist because the K8 journal manifest must carry
/// the COMPLETE actor identity (plan Task 4): recovery re-renders `status.yaml`
/// from the manifest alone, so a partially-carried identity would silently lose
/// actor metadata after an interruption.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorIdentity {
    pub name: String,
    pub actor_type: String,
    pub model: String,
    pub provider: String,
    pub context_window: i64,
    pub sdk_version: String,
    pub entrypoint: String,
    pub registered_at: String,
}

/// The suffix a playbook machine gives a state that work is sent BACK into.
///
/// SINGLE-SOURCED HERE BECAUSE IT WAS ALREADY DUPLICATED. `playbook_run_fidelity`
/// counted `revision_cycles` off a private copy of this suffix and
/// `step_two_by_two` decided its one-shot axis off a second private copy. Two
/// folds deciding independently what "sent back" means is how two surfaces come
/// to disagree about whether the same run was clean, and the disagreement is
/// invisible because both are reading the same transitions and both look right.
/// A third consumer arriving (the autonomy case) is what made the duplication
/// worth closing rather than extending.
pub const REVISION_STATE_SUFFIX: &str = "_revision";

/// Whether a state is one work is sent BACK into — the structural definition of
/// a correction, used by every fold that counts one.
pub fn is_revision_state(state: &str) -> bool {
    state.ends_with(REVISION_STATE_SUFFIX)
}

/// The step kind a revision state belongs to (`spec_revision` → `spec`), or
/// `None` when the state is not a revision state.
pub fn revision_step_kind(state: &str) -> Option<&str> {
    state.strip_suffix(REVISION_STATE_SUFFIX)
}
