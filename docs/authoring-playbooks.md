# Authoring and Registering an Anvil Playbook

This guide covers everything needed to author a new **playbook**, ship it, register it, and verify
it is live.

A **playbook** is the reusable lifecycle definition artifact — a `machine.yaml` state machine plus
its hook bodies. One execution driven by a playbook is a **playbook run**. This guide uses that
vocabulary throughout; `docs/vocabulary.md` is canonical.

Every legacy token below is a deliberate, labeled reference to a removed surface, a persisted key,
or a compatibility alias — never live vocabulary.

This file replaces the retired `docs/authoring-workflows.md`. That guide documented three surfaces
under names the engine no longer exposes (`persist_workflow`, `candidate_workflow_intake`,
Foundry's `WorkflowRegistrar`), one manifest key the installer no longer reads
(`workflows.definitions`), and a `workflow_generation` guided machine that no longer ships a
`machine.yaml`. Its still-true content is carried forward here against re-derived paths; its
removed-surface prose is retired with it.

Canonical living example: `playbooks/track_lifecycle/machine.yaml` in this repo.

---

## 1. The model

### Playbook = machine

A playbook is a **state machine declared in `machine.yaml`**. The machine defines:

- the artifact `kind` it governs (`track`, `decision`, `learning`, …);
- the ordered set of **states** an artifact of that kind passes through;
- the legal **transitions** between those states;
- **hooks** — markdown files under `hooks/`, served to the acting agent as `context_text`;
- **measurements** — `intent`, `expected_output`, and `success_criteria` strings the engine returns
  alongside hook content, so the agent knows what to produce and how it will be judged;
- an **`outcome_predicate`** naming the terminal state and the check that decides whether a run
  actually landed its outcome.

Artifacts created with `begin(artifact_type: <kind>)` are governed by the matching machine for
their entire lifetime.

### Step = state

Each entry in `states:` is one step. States carry:

| Field | Purpose |
|---|---|
| `name` | Snake_case identifier (`spec`, `spec_review`, …) |
| `role_filters` | Who may act |
| `registry_section` | Which heading in the registry file this state maps to |
| `projection_targets` | Registry/projection files updated on entry |
| `is_review_gate` | `true` on states that expect a satisfaction verdict |
| `is_terminal` | `true` on end states (`completed`, `abandoned`, …) |
| `hooks_by_role` | Map from role name to hook filename under `hooks/` |
| `measurement_by_role` | Map from role name to `{intent, expected_output, success_criteria, evidence_obligation}` |

### `register: free | driven`

`register` controls routing:

| Value | Meaning |
|---|---|
| `driven` (default) | The router surfaces this playbook as a candidate when `anvil_orchestrate` is called; it appears in `catalog` available types |
| `free` | Never a router candidate; beginnable out of band via `begin(artifact_type: <kind>)`. Use it for lifecycle support artifacts created by explicit human intent |

Omitting `register` defaults to `driven`.

### Hook-based content model

For each `(state, role)` pair where an agent must act, the machine names a hook file:

```yaml
hooks_by_role:
  doer: spec-writing.md
  reviewer: spec-review.md
```

Hook files live at `<playbook-dir>/hooks/<filename>`. Their content is the **complete instruction
set** for the agent in that `(state, role)` pair. When `begin` resolves an artifact's current
`(state, role)`, the engine returns the hook's text as `context_text`.

A state may also carry a single top-level `hook:` field, used by older machines. `hooks_by_role` is
preferred: it allows a different hook per role in the same state.

**Hook paths in prose.** Any documented hook path — in a skill, in a hook body, in a design note —
must be written `playbooks/<kind>/hooks/<file>.md`. `workflows/.../hooks/...` is a dead path; the
engine scans `playbooks/` (see §4). `anvil-core/features/agent_hook_paths_resolve.feature` resolves
every such documented path against **staged kit content** and fails the build if one is stale.

### Measurement enforcement

The engine registers playbooks through an **enforcing** loader when
`ANVIL_ENFORCE_MEASUREMENT_DEFINITION=1`. Under enforcement, a machine whose measured states lack
`success_criteria`, or that has no `outcome_predicate`, silently **drops out of the registry** — its
`begin`/`catalog` calls then fail downstream with no trace. `scripts/build-kit.sh` runs the same
enforcing load over the staged playbooks (`cargo run --release --example enforcement_bundle_check
-p anvil-core -- <KIT_DIR>`) and refuses to publish a kit that would drop a machine.

---

## 2. The `machine.yaml` shape

Minimal annotated schema:

```yaml
# Required: the governed artifact kind. Must be unique across registered playbooks.
# This is NOT the string `playbook` — see the three-kind-fields note below.
kind: my_kind

# Required: free | driven (default: driven)
register: driven

# Routing triggers. Only meaningful for driven playbooks. `description` is the
# routing-oriented instruction surfaced to candidate selection; state the
# NOT-this contrasts explicitly. `triggers` are concrete example phrases.
route:
  description: "Route here when the user asks to ... NOT for ..."
  triggers:
    - "start my thing"

# Required: plural directory name under the hearth where runs live.
directory: my_kinds

# Required: registry file updated on state transitions.
registry: my_kinds.md

# Optional: kind of parent artifact required on creation. Omit (~) if none.
parent_kind: ~

# One-line description shown in catalog available types.
description: "What this playbook does, in one sentence."

# Fields required at begin() time.
required_fields:
  - name: name
    field_type: string
    description: A human-readable name for this artifact

roles:
  - doer
  - reviewer

states:
  - name: draft
    role_filters: [doer_actionable]
    registry_section: draft
    projection_targets: [my_kinds.md]
    is_review_gate: false
    is_terminal: false
    hooks_by_role:
      doer: draft.md
    measurement_by_role:
      doer:
        intent: "Author the draft artifact."
        expected_output: "A draft.md ready for review."
        success_criteria: "A draft.md exists and states X, Y, and Z as falsifiable claims."

  - name: draft_review
    role_filters: [review_pending]
    registry_section: draft
    projection_targets: [my_kinds.md]
    is_review_gate: true
    is_terminal: false
    hooks_by_role:
      reviewer: draft-review.md
    measurement_by_role:
      reviewer:
        intent: "Review the draft for completeness and correctness."
        expected_output: "A review entry with a satisfied sign-off or specific findings."
        success_criteria: "Each finding names a file and a falsifiable defect."

  - name: completed
    role_filters: [terminal]
    registry_section: completed
    projection_targets: [my_kinds.md]
    is_review_gate: false
    is_terminal: true

transitions:
  - from_state: draft
    to_state: draft_review
    required_role: doer
    required_satisfaction: ~
    requires_approver: false

  - from_state: draft_review
    to_state: completed
    required_role: reviewer
    required_satisfaction: [satisfied]
    requires_approver: true

# Required under measurement enforcement.
outcome_predicate:
  terminal_state: completed
  check: the artifact reached completed with the reviewed draft recorded
```

**Second example:** `playbooks/decision_lifecycle/machine.yaml` — a `free`-register playbook with
branching transitions (tension → investigating → decided, or tension → decided directly).

### Three `kind` fields that stay distinct

For a directory such as `playbooks/track_lifecycle/`:

- manifest `anvil_kind: track` identifies the governed artifact kind and drives registrar dedup;
- `machine.yaml` `kind: track` identifies the state machine and keys the engine registry;
- `status.yaml` `kind:` identifies the definition **directory** as a playbook artifact.

Only the third names the artifact class. Do not rewrite the first two.

---

## 3. Package → install → registrar

This is the real current path for a playbook shipped inside a kit:

```text
source playbook directories            anvil/playbooks/<id>/
  → kit build and staged manifest      scripts/build-kit.sh → dist/anvil-kit/
  → FKM download / hash check / extraction
  → versioned plugin-cache install
  → read playbooks.definitions
  → Foundry PlaybookRegistrar
  → <hearth>/playbooks/<artifact>/
  → Anvil's per-request registry scan
```

### 3a — Source layout

Ship the playbook in the kit repo at `<kit>/playbooks/<id>/`:

```text
playbooks/<id>/
  machine.yaml      required — the build discovers playbooks by this file's presence
  definition.md     optional prose
  status.yaml       optional — the definition artifact's own lifecycle record
  hooks/*.md        one file per (state, role) pair named in hooks_by_role
  skills/*/SKILL.md optional playbook-scoped skills
```

`scripts/build-kit.sh` enumerates `playbooks/*` and **skips any directory without a
`machine.yaml`**. A directory that carries hooks but no machine is not packaged, is not installed,
and is not registered — so nothing shipped may document a hook path inside it.

### 3b — Kit build

`scripts/build-kit.sh` stages the kit at `dist/anvil-kit/` (override with `KIT_DIR`). The
content-staging step is factored into `scripts/stage-kit-content.sh`, which build-kit.sh invokes;
it copies each discovered playbook to `<KIT_DIR>/playbooks/<id>/` and the top-level `skills/` tree
to `<KIT_DIR>/skills/`, and prints the discovered playbook ids. The build then writes, into the
staged `foundry-manifest.json`:

```json
"playbooks": {
  "definitions": [
    { "id": "track_lifecycle", "path": "playbooks/track_lifecycle/", "anvil_kind": "track" }
  ]
}
```

`anvil_kind` is read out of each staged `machine.yaml`'s `kind:` line. The build writes **only** the
canonical `playbooks` key: the installer's manifest type declares `playbooks` with
`alias = "workflows"`, so emitting both keys makes deserialization reject the manifest as a
duplicate field.

`scripts/publish-kit.sh` then publishes `$KIT_DIR` to the Foundry Kit Marketplace.

### 3c — Install and registration

Foundry downloads and extracts the kit, installs it into the versioned plugin cache, wires
platforms, and then reads the manifest. Its reader accepts `playbooks.definitions`, with singular
`playbook.definitions` as a fallback; absent, malformed, or legacy `workflows` data yields an
**empty list**. Foundry's `PlaybookRegistrar` copies each declared definition into
`<hearth>/playbooks/<artifact>/` and stamps `contributed_by: <kit>` attribution.

Two registrar branches matter when you re-publish:

| Branch | Match | Effect |
|---|---|---|
| Existing governed kind | Top-level `machine.yaml` `kind`, with the directory suffix as fallback | Updates attribution and, where ownership permits, owner/visibility/org. It does **not** copy the payload, and does not rewrite lifecycle kind or state. |
| No existing governed kind | No declared-kind or suffix match | Recursively copies the whole payload directory, then stamps attribution/access. |

**Consequence for authors:** reinstalling a kit does **not** repair an already-registered kind's
machine, hooks, or status. A hook body edit reaches an existing hearth only when that kind's
definition directory was actually removed first (for example by uninstalling a sole-owning kit).
Do not assume "bump the kit version and reinstall" republishes hook content.

Registration is **filesystem-shaped**. A kit's engine never reaches into a user's Anvil install; a
kit ships playbook directories and Foundry places them. There is no `register_playbook` RPC, and no
Anvil code change is needed for a kit to extend Anvil.

---

## 4. How the engine finds a playbook

Anvil does not hold a pushed registry. On each request it scans
`{hearth_path}/playbooks/*/machine.yaml`. Resolution order is:

1. the **request hearth** — the `hearth_path` carried by the call (a project's own
   `<project>-hearth/playbooks/`);
2. the **global playbooks hearth** — `--global-playbooks-hearth`, else the
   `ANVIL_GLOBAL_PLAYBOOKS_HEARTH` environment variable;
3. Anvil's compiled-in seed machines.

Before scanning, the registry performs a one-time idempotent rename of a legacy
`{hearth_path}/workflows/` directory to `{hearth_path}/playbooks/`. That is a migration affordance,
not a supported location: once canonical `playbooks/` exists, a sibling legacy directory is neither
merged nor inspected.

Because the scan is per request, a newly landed `machine.yaml` is picked up by the next
`describe`/`catalog` — **no engine restart, no Anvil code change**.

### Per-project playbooks

A playbook placed in a project's own hearth (`<project>/<project>-hearth/playbooks/<kind>/`) is read
for requests carrying that `hearth_path`. Use this for project-local playbooks that should not be
globally installed.

---

## 5. Authoring without a kit

### `persist_playbook` — the engine-facing write primitive

`persist_playbook` writes a `machine.yaml` (and optional hook bodies) to a pre-resolved
`owner_home`, landing at `<owner_home>/playbooks/<kind>/`.

```text
persist_playbook(
  owner_home:     "/abs/path/to/target-hearth",
  kind:           "my_kind",
  machine_yaml:   "<full machine.yaml text>",
  hooks:          [{ name: "intent.md", content: "..." }],   # optional
  actor_name:     "...",
  actor_type:     "agent",
  actor_model:    "...",
  actor_provider: "..."
)
```

The engine validates the machine before writing. Every `hook:` reference in `machine.yaml` must name
a file carried in `hooks`, or the persist is rejected. A byte-identical existing file succeeds
idempotently with no event; a different machine under the same kind is rejected as a duplicate kind
registration.

Use it when a `machine.yaml` was generated programmatically and must land in a specific hearth.
Foundry resolves `owner_home` from an owner descriptor before the call.

### `candidate_playbook_intake` — guided generation from a Lore candidate

`candidate_playbook_intake` begins a seeded `playbook_generation` builder run from a Lore
`CandidatePlaybook` — a structured observation that a new process pattern has emerged. It seeds the
builder with the proposed states, route description, route triggers, projection targets, and
evidence; the generation lifecycle then drives gathering → modeling → testing, and the terminal
transition persists the generated machine to the resolved `target_owner`.

Use it when Lore has surfaced a candidate pattern, or when a user's request matches no existing
driven playbook (`anvil_orchestrate` returns no match).

> The `playbook_generation` builder is engine-driven. The old advice to
> `begin(artifact_type: workflow_generation)` against a hand-authored machine under
> `playbooks/20260528T2321_workflow_generation/` no longer applies: that directory ships no
> `machine.yaml`, so it is not packaged and not registered. It survives only as historical hook
> prose.

---

## 6. Build → distribute → verify → retire-skill

Skills are markdown files distributed with the kit. **Retiring a skill is instant and global** —
removing it removes it everywhere, including in sessions currently relying on it. The safe sequence:

1. **Build.** Author `machine.yaml` and hooks. Verify locally (§7).
2. **Distribute.** Land the machine in the target hearth via the kit path (§3) or `persist_playbook`
   (§5). Confirm it appears in `catalog` from a foreign hearth before proceeding.
3. **Verify live.** Begin a real artifact with `begin(artifact_type: <kind>)` from a project hearth.
   Walk one step; confirm the hook content is what you authored.
4. **Retire the skill** only after the machine is verified live.

The rule: **the machine is live before the skill disappears, never after.**

---

## 7. How to verify

### 7a — Catalog the hearth

Call `catalog` from a session whose hearth resolves to where the playbook landed. The response lists
the available artifact types the engine knows about; the new `kind` should appear with the
`description` from `machine.yaml`. If it is missing, check in this order:

- the directory is `<hearth>/playbooks/<kind>/`, not `<hearth>/workflows/<kind>/`;
- `machine.yaml` is present and parses (invalid machines are logged as invalid artifacts);
- under measurement enforcement, every measured state has `success_criteria` and the machine has an
  `outcome_predicate` — otherwise the machine is dropped silently
  (`cargo run --release --example enforcement_bundle_check -p anvil-core -- <dir>` reproduces the
  drop and names it);
- for a kit-shipped playbook, the staged `foundry-manifest.json` actually declares it under
  `playbooks.definitions`.

### 7b — Begin from a foreign hearth

```text
begin(
  artifact_type:  "my_kind",
  name:           "test-run",
  approver:       "nick",
  actor_name:     "...",
  actor_type:     "agent",
  actor_model:    "...",
  actor_provider: "..."
)
```

A successful `begin` returns `context_text` (the hook body for the first state) and the measurement
(`intent` + `expected_output`). Verify both against what you authored. An unknown artifact type
means the machine is not resolving; empty or wrong `context_text` means the `hooks_by_role` mapping
or the hook filename is wrong.

### 7c — Walk a transition

Call `complete` on the artifact from 7b to move it to the first review gate and confirm the engine
returns the reviewer's hook body. This exercises transition logic end to end.

---

## Further reading

- `docs/vocabulary.md` — the canonical vocabulary this guide uses
- `playbooks/track_lifecycle/machine.yaml` — full lifecycle with review gates and reflection
- `playbooks/decision_lifecycle/machine.yaml` — free-register, branching transitions
- `scripts/build-kit.sh` and `scripts/stage-kit-content.sh` — the packaging path §3b describes
- `anvil-core/src/domain/playbook/hearth_registry.rs` — the per-request hearth scan §4 describes
- `anvil-core/src/domain/persist_playbook.rs` — `persist_playbook` handling and its error codes
- `anvil-core/features/agent_hook_paths_resolve.feature` — the seam that keeps documented hook paths
  resolvable against staged kit content
