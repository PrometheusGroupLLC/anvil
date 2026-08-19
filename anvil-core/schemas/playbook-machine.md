# Playbook Machine Schema (`machine.yaml`)

The `machine.yaml` file in a playbook artifact's directory declares the state
machine, role taxonomy, transitions, and hook pointers for one artifact kind.
The engine parses this file on `catalog` and `describe` calls and validates it
using the types in `anvil-core/src/domain/playbook/`.

## Format

YAML. Consistent with `status.yaml` and all other mechanical files in the
hearth. The schema is strict: every top-level and nested struct uses
`#[serde(deny_unknown_fields)]` — unknown keys are rejected as parse errors.
This enforces the sequential-phase-only constraint: event-trigger fields like
`timeout_advance` or `concurrent_transitions` produce errors, not silent drops.

---

## Top-level keys

| Key | Type | Required | Description |
|---|---|---|---|
| `kind` | string | yes | The artifact kind this playbook governs, e.g. `"track"`, `"proposal"`. Must be unique across all loaded playbook artifacts. |
| `directory` | string | yes | Plural directory name under `forge/`, e.g. `"tracks"`. |
| `registry` | string | yes | Registry file name, e.g. `"tracks.md"`. |
| `parent_kind` | string or null | no | The artifact kind that must exist as a parent for instances of this kind, e.g. `"proposal"`. Omit or `~` if none required. |
| `description` | string | yes | One-line description surfaced via `available_types`. Should identify the kind's purpose clearly enough for an unfamiliar agent. |
| `required_fields` | list of `FieldDescriptor` | yes | Fields required by `begin(artifact_type:...)` creation calls. May be an empty list `[]`. |
| `roles` | list of strings | yes | Role names legal for this playbook. Minimum `["doer", "reviewer"]`. Transitions reference roles from this list. |
| `states` | list of `StateDefinition` | yes | State definitions for this lifecycle. |
| `transitions` | list of `TransitionDefinition` | yes | Transition definitions for this lifecycle. May be an empty list `[]`. |

---

## `FieldDescriptor`

| Key | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Field name, e.g. `"playbook_name"`. |
| `field_type` | string | yes | Type hint, e.g. `"string"`, `"artifact_id"`, `"actor_name"`. |
| `description` | string | yes | Short description surfaced via `available_types`. |

---

## `StateDefinition`

| Key | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | State identifier, e.g. `"spec"`, `"spec_review"`. Must be unique within this playbook's states list. |
| `role_filters` | list of `RoleFilter` | no | Role-based filter classifications for this state. Valid values: `doer_actionable`, `review_pending`, `review_awaiting`, `creator_parent`, `terminal`. Defaults to `[]`. |
| `registry_section` | string | yes | The section in the registry file where instances in this state are listed. |
| `projection_targets` | list of strings | no | Projection files to update on transitions into this state. Defaults to `[]`. |
| `is_review_gate` | boolean | yes | Whether this state is a review gate — enables `complete(satisfaction:...)` semantics on outgoing transitions. Review-gate states must have `required_satisfaction` on every outgoing transition. |
| `is_terminal` | boolean | yes | Whether this state is terminal (no further transitions expected by convention). |
| `hook` | string or null | no | Filename under `hooks/` served by the engine when `begin` enters this state. The file must exist under `hooks/` if specified (validated by the loader). In this track, hook files are discovered but not yet served via `context_text` — that wiring lands in a future per-consumer track. |

---

## `TransitionDefinition`

| Key | Type | Required | Description |
|---|---|---|---|
| `from_state` | string | yes | Source state. Must be declared in this playbook's `states` list. |
| `to_state` | string | yes | Target state. Must be declared in this playbook's `states` list. |
| `required_role` | string | yes | The role required to execute this transition. Must be declared in this playbook's `roles` list. |
| `required_satisfaction` | list of strings or null | see note | Legal satisfaction values for transitions out of review gates. **Required (non-null) when `from_state` is a review gate (`is_review_gate: true`).** Use `~` for non-review-gate transitions. |
| `requires_approver` | boolean | yes | Whether an approver must be named in the snapshot call for this transition. |
| `hook` | string or null | no | Filename under `hooks/` served when `begin` executes this specific transition, if different from the target state's hook. Must exist under `hooks/` if specified. |

---

## `RoleFilter` values

| Value | Meaning |
|---|---|
| `doer_actionable` | Doer (author/implementer) can continue work here. |
| `review_pending` | Reviewer can act here — review artifact is available. |
| `review_awaiting` | Doer-produced; next action requires the reviewer role. |
| `creator_parent` | Creator can create child artifacts while artifact is in this state. |
| `terminal` | No further lifecycle progression expected. |

---

## What the schema does NOT declare

The engine owns the following operations uniformly across all playbook kinds.
These are **NOT** hook-addressable — no `machine.yaml` key controls them:

- **`status.yaml` writes** — every transition records to `status.yaml`; this
  is engine infrastructure, not playbook prose.
- **Registry moves** — moving artifact entries between registry sections on
  state changes is engine-owned. The `registry_section` field on states tells
  the engine which section to move to; the move itself is mechanical.
- **Projection-file updates** — updating derived views (`execution.md`,
  `intent.md`, etc.) on state changes is engine-owned. The
  `projection_targets` field names the files; the update logic is mechanical.
- **Actor three-leg rule** — the add-if-absent / match-no-op / mismatch-append
  invariant on `status.yaml`'s actors table is engine-owned for every
  transition regardless of which playbook is in play.
- **Projection rebuild logic** — full projection regeneration from
  authoritative sources is engine-owned; no playbook artifact triggers or
  configures it.

The schema also does NOT declare:

- Spark's event-log-native storage model (deferred to Track #13).
- External-event triggers, timeouts, concurrent transitions, or any
  non-sequential-phase lifecycle primitive (Validity Condition 1 from
  decision `playbook-as-artifact`).
- The `reference` artifact kind's storage or linking conventions (separate
  future track).

---

## `hooks/*.md` convention

Hook files are Markdown prose under the playbook artifact's `hooks/`
directory. Each file corresponds to one state or transition named in
`machine.yaml`. Format is open — no rigid template is enforced.

Recommended shape (~15–30 lines):

1. A one-paragraph summary of what happens at this state/transition.
2. Action items for the agent, referencing other artifacts via
   `describe()` calls where relevant.
3. Optional "edge cases" or "watch for" section.

In this track, the engine discovers hook files (validates their existence
during `machine.yaml` parsing) but does not yet serve their content via
`begin`'s `context_text`. Serving is wired in a future per-consumer track.

---

## Example

```yaml
kind: playbook
directory: playbooks
registry: playbooks.md
parent_kind: track
description: "Definition of another artifact kind's lifecycle."
required_fields:
  - name: playbook_name
    field_type: string
    description: "Human-readable name for this playbook."
  - name: parent_id
    field_type: artifact_id
    description: "Parent track authorizing this playbook."
  - name: approver
    field_type: actor_name
    description: "Human or reviewer authorizing creation."
roles: [doer, reviewer]
states:
  - name: draft
    role_filters: [doer_actionable]
    registry_section: draft
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: draft_review
    role_filters: [review_pending]
    registry_section: draft
    projection_targets: []
    is_review_gate: true
    is_terminal: false
    hook: draft_review_guidance.md
  - name: active
    role_filters: [creator_parent]
    registry_section: active
    projection_targets: []
    is_review_gate: false
    is_terminal: false
  - name: retired
    role_filters: [terminal]
    registry_section: retired
    projection_targets: []
    is_review_gate: false
    is_terminal: true
transitions:
  - from_state: draft
    to_state: draft_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
  - from_state: draft_review
    to_state: draft
    required_role: reviewer
    required_satisfaction: [rejected]
    requires_approver: false
  - from_state: draft_review
    to_state: active
    required_role: reviewer
    required_satisfaction: [satisfied]
    requires_approver: true
  - from_state: active
    to_state: retired
    required_role: doer
    required_satisfaction: ~
    requires_approver: false
```

---

## Research question resolutions (see also `spec.md` R9)

| Question | Resolution |
|---|---|
| Q1: Schema format | YAML (this file's format). |
| Q2: Cross-artifact hook split | Engine owns mechanical; playbook prose owns editorial. See "NOT declared" section above. |
| Q3: Spark lifecycle shape | Schema is sequential-phase-only; spark's event-log lifecycle is Track #15's concern. |
| Q4: Compile-time seed vs loaded | Seeds are hand-coded Rust struct literals using this same shape (Phase 3). |
| Q5: Bootstrap paradox | `playbook` kind's lifecycle is also seeded in code (Phase 3). |
| Q6: Migration of legacy state constants | Punted to per-consumer migration tracks. |
| Q7: Routing cache policy | Punted to `routing::compute_execution_route` migration track. |
