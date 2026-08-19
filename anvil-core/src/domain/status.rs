use crate::domain::shared_types::ActivityLog;
use serde::Deserialize;
use std::collections::HashMap;

/// Deserialize the `activity:` field one entry at a time, DROPPING any entry
/// that fails to deserialize into `ActivityEntry` — e.g. a hand-authored
/// begin-marker missing the required `state:` field.
///
/// Buffers the field as a raw YAML value first, then parses each element
/// independently so a single malformed marker cannot fail the whole
/// `status.yaml` parse. Returns an [`ActivityLog`] carrying the survivors
/// PLUS a dropped-entry count and the first drop's diagnostic — the count is
/// what lets begin-adoption consumers distinguish a DEGRADED log (a dropped
/// entry may have been an open begin marker) from a truly-empty one. A dropped
/// marker is simply absent from `entries` (NOT coerced to a bogus empty-state
/// entry), so downstream begin-adoption / live-instance / actor-activity folds
/// — which read `entry.state` — never see a fabricated value.
///
/// The warning itself is NOT emitted here: the deserializer does not know the
/// file path (`eprintln` at this level loses it). The count + first-error are
/// carried out on the [`ActivityLog`] and the adapter boundary — which knows
/// the `status.yaml` path — emits the path-aware warning via
/// [`ActivityLog::warn_if_degraded`].
///
/// Boundary shapes are all tolerated so one bad file can never sink the shared
/// scan: an absent key yields `None` (serde never calls this — `#[serde(default)]`
/// supplies `None`); `activity: null` yields `None`; `activity:` as a scalar or
/// map (structurally wrong) yields a fully-degraded empty log rather than a
/// hard parse failure.
fn deserialize_lenient_activity<'de, D>(deserializer: D) -> Result<Option<ActivityLog>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    let items = match value {
        // Absent-key parity: an explicit `null` is "no log", same as omission.
        serde_yaml::Value::Null => return Ok(None),
        serde_yaml::Value::Sequence(items) => items,
        // `activity:` present but not a list (scalar / map). Structurally wrong
        // — but a hard error here would re-sink the whole file (the exact
        // failure this fix exists to prevent). Degrade to an empty log instead.
        other => {
            return Ok(Some(ActivityLog {
                entries: Vec::new(),
                dropped: 1,
                first_error: Some(format!(
                    "activity: expected a list of markers, found {}",
                    yaml_shape(&other)
                )),
            }));
        }
    };
    let mut entries = Vec::with_capacity(items.len());
    let mut dropped = 0usize;
    let mut first_error: Option<String> = None;
    for (idx, value) in items.into_iter().enumerate() {
        match serde_yaml::from_value::<crate::domain::shared_types::ActivityEntry>(value) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                dropped += 1;
                if first_error.is_none() {
                    first_error = Some(format!("activity[{}]: {}", idx, e));
                }
            }
        }
    }
    Ok(Some(ActivityLog {
        entries,
        dropped,
        first_error,
    }))
}

/// A one-word description of a YAML value's shape, for the degraded-activity
/// diagnostic (`activity:` was present but not a list).
fn yaml_shape(value: &serde_yaml::Value) -> &'static str {
    match value {
        serde_yaml::Value::Null => "null",
        serde_yaml::Value::Bool(_) => "a boolean",
        serde_yaml::Value::Number(_) => "a number",
        serde_yaml::Value::String(_) => "a string",
        serde_yaml::Value::Sequence(_) => "a list",
        serde_yaml::Value::Mapping(_) => "a map",
        serde_yaml::Value::Tagged(_) => "a tagged value",
    }
}

/// Full status.yaml schema — shared across all adapters.
/// Replaces the minimal StatusYaml structs that only deserialized the state field.
#[derive(Debug, Clone, Deserialize)]
pub struct FullStatusYaml {
    pub version: Option<u32>,
    pub kind: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub origin_turn: Option<String>,
    /// Parent artifact id under the unified `parent_id:` key. Aliases read the
    /// legacy `proposal:` (tracks) key. NOTE: `track:` is deliberately NOT an
    /// alias — other hearths (e.g. temper) use a top-level `track:` field as a
    /// self-id alongside a `proposal:` parent, and aliasing both onto parent_id
    /// makes serde reject the file as a duplicate field (which chokes the
    /// engine-wide checkin scan). Legacy anvil workflow-kind parents (written as
    /// `track:`) are migrated to `parent_id:` on disk.
    #[serde(default, alias = "proposal")]
    pub parent_id: Option<String>,
    pub actors: Option<HashMap<String, serde_yaml::Value>>,
    pub transitions: Option<Vec<StatusTransition>>,
    /// Append-only begin-marker log, sibling to `transitions:`.
    /// `Option`-wrapped so existing status.yaml files (no `activity:` key)
    /// parse unchanged — additive, zero migration.
    ///
    /// Deserialized PER-ENTRY-TOLERANT: a single malformed marker (e.g. a
    /// hand-authored entry missing the required `state:` field) is DROPPED
    /// with a stderr warning rather than sinking the whole-file parse. This
    /// is a shared-path fail-open: the catalog/checkin scan reads every
    /// artifact's status.yaml, so one bad marker used to abort the entire
    /// scan and take down every governed operation engine-wide. The
    /// `activity:` log is a soft, append-only signal (already `Option` +
    /// `default` for back-compat), so degrading it per-entry keeps the
    /// artifact's load-bearing top-level state/kind/transitions fully
    /// readable and the artifact visible. Genuinely top-level-corrupt
    /// status.yaml still fails to parse (a real, surfaced error) —
    /// unchanged.
    ///
    /// The parsed value is an [`ActivityLog`], NOT a bare `Vec` — it carries
    /// the dropped-entry count so begin-adoption consumers can tell a degraded
    /// log apart from a truly-empty one. Read the survivors via
    /// [`FullStatusYaml::activity_entries`] and the degradation via
    /// [`FullStatusYaml::activity_dropped`].
    #[serde(default, deserialize_with = "deserialize_lenient_activity")]
    pub activity: Option<ActivityLog>,
    /// Owner kit/author injected at kit-install. Read by the `WorkflowActivity` RPC
    /// read-side to attribute each playbook to the kit that contributed it.
    /// Absent / empty → the query applies the "anvil" default. `Option`-wrapped
    /// + `#[serde(default)]` so every existing status.yaml parses unchanged.
    #[serde(default)]
    pub contributed_by: Option<String>,
}

impl FullStatusYaml {
    /// Resolve the artifact's current state.
    ///
    /// The top-level `state` field is a denormalized cache of the last
    /// transition's `to` — the writer keeps them equal on every transition.
    /// When the top-level field is absent (e.g. a hand-created artifact, or
    /// one transitioned before the writer inserted the field), the last
    /// transition's `to` reproduces exactly what the writer would have
    /// written. State is unresolvable only when there is neither a top-level
    /// `state` nor any transitions.
    ///
    /// Delegates to the single resolution seam
    /// (`crate::domain::transition_log::resolve_state`) so every consumer —
    /// including external callers of this method — shares one definition of
    /// "current state".
    ///
    /// LEGACY / NON-FOLDING: this resolves from the parsed `status.yaml` alone
    /// (top-level `state:`, else last legacy `transitions[]` entry). It does NOT
    /// fold the per-file transition event store. For an on-disk artifact the
    /// authoritative current state is `resolve_state_with_events` /
    /// `QueryPort::read_artifact_state`, which folds the event directory. The
    /// header is re-projected on every transition now
    /// (`anvil_core_hearth::status_header`), so it normally agrees — but it is a
    /// projection, and a hand-edited or merge-mangled one disagrees. Use this
    /// method only for legacy-array-only inputs (e.g. the resolved_state unit
    /// test); never as the on-disk answer.
    pub fn resolved_state(&self) -> Option<String> {
        crate::domain::transition_log::resolve_state(self)
    }

    /// The successfully parsed `activity:` begin-markers (malformed entries
    /// dropped). Empty when the `activity:` key is absent, `null`, or every
    /// entry was malformed. Callers that also need to know whether the log was
    /// DEGRADED must consult [`Self::activity_dropped`] — an empty vec alone
    /// does not distinguish "no markers" from "all markers dropped".
    pub fn activity_entries(&self) -> Vec<crate::domain::shared_types::ActivityEntry> {
        self.activity
            .as_ref()
            .map(|log| log.entries.clone())
            .unwrap_or_default()
    }

    /// The number of `activity:` entries dropped as malformed (`0` when the
    /// log is clean or absent). `> 0` ⇒ the begin-adoption log is a partial
    /// view and must be treated conservatively.
    pub fn activity_dropped(&self) -> usize {
        self.activity.as_ref().map(|log| log.dropped).unwrap_or(0)
    }
}

/// A single transition entry in status.yaml.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusTransition {
    pub to: String,
    pub at: Option<String>,
    pub actor: Option<String>,
    pub role: Option<String>,
    pub approver: Option<String>,
    pub note: Option<String>,
    /// Machine-readable event-type discriminator, folded through from the
    /// per-file transition record. `Some("adoption")` marks a governance
    /// adoption reset; `None` (the common case, and the default for any legacy
    /// record that predates the field) for an ordinary transition. Consumers
    /// filter adoption STRUCTURALLY on this field rather than on `note` prose.
    #[serde(default)]
    pub event_type: Option<String>,
    /// The reviewer verdict recorded on this transition, folded through from
    /// the per-file transition record's `satisfaction`.
    ///
    /// The event store has carried this since the carry-forward slice and the
    /// fold DROPPED it, so nothing reading an artifact's history could tell a
    /// run that was sent back from a run that was waved through — which is
    /// exactly the fact "what it got wrong, and who caught it" is made of. The
    /// verdict and the actor who recorded it live on the SAME step; folding one
    /// without the other is what left the catch invisible.
    ///
    /// `None` for a step nobody reviewed, and NEVER `Some("")`: a run its
    /// author simply moved forward was not approved by anybody, and an empty
    /// string would satisfy a reader that only asks whether the field is there.
    /// The legacy `status.yaml` array has no such column and never will, so a
    /// legacy row folds to `None` rather than acquiring an approval
    /// retroactively.
    #[serde(default)]
    pub satisfaction: Option<String>,
}
