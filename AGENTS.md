# Forge Agent Workflow

This document describes the development workflow for agents working on this project. Read PHILOSOPHY.md for the testing philosophy. Read CLAUDE.md for the build commands and rules. Read `forge/projections/` for current project state (intent.md, execution.md, truth.md, forward.md — read the files relevant to your work).

## Mental model

The forge artifacts are an **event-sourced log** of development decisions. The project's current semantic state is derived by replaying them in order. There is no manually maintained "current state" document — the snapshot is a materialized view, regenerated after any terminal state transition (`completed`, `superseded`, `abandoned`).

The development loop follows **build-measure-learn**:

```
propose → spec → plan → implement → reflect → (next propose)
```

With **review** as a quality gate at every transition and **complete** as administrative closure.

## Two lifecycles

### Proposal lifecycle

Proposals are strategic — they describe a direction that spawns one or more tracks.

```
vision → vision_review ⇄ vision_revision → draft → proposal_review ⇄ proposal_revision → active → [tracks realize it] → reflecting → reflection_review ⇄ reflection_revision → completed
                ↓                                        ↓                                  ↑ ↓                                ↓
             abandoned                                abandoned                    amend loop  superseded / abandoned
                                                                                     ↓
                                                                          amend → amend_review ⇄ amend_revision → active
```

A proposal begins as a **vision** — the problem and desired future state, without committing to an approach. Vision review is lighter than proposal review: "is this worth pursuing? is the problem real?" not "is the approach sound?" Once the direction is approved, the proposal moves to `draft` where the concrete approach is developed.

A proposal becomes `active` when its first track is planned. It can be `abandoned` from `vision` or `draft`, or `superseded`/`abandoned` from `active`. The human triggers these transitions. The `active → reflecting` transition is triggered by the human when they judge the proposal's intent has been sufficiently realized.

**Proposal amendments:** When track execution teaches something that changes proposal-level intent, the proposal enters an amend loop: `active → amend → amend_review ⇄ amend_revision → active`. The amendment is recorded in `proposal.amendments.md` (append-only). The review is appended to `proposal.review.md` — there is no separate amendments review file. Only reviewed and human-approved amendments are authoritative.

**Documents:**
- `vision.md` — the direction: problem, desired future state, why it matters (frozen after vision review)
- `vision.review.md` — vision review conversation (lighter — "is this worth pursuing?")
- `proposal.md` — the approach: how to realize the vision (frozen after approval)
- `proposal.review.md` — proposal review conversation
- `*.amendments.md` — append-only amendment log for changes to frozen artifacts (e.g., `vision.amendments.md`, `proposal.amendments.md`, `reflection.amendments.md`). Frozen artifacts are never edited; amendments are recorded here. Amendment reviews are appended to the phase's existing review document.
- `reflection.md` — semantic delta after tracks realize the proposal
- `reflection.review.md` — reflection review conversation
- `status.yaml` — state machine position + transition history

### Track lifecycle

Tracks are tactical — they implement a concrete slice of a proposal.

```
spec → spec_review ⇄ spec_revision → plan → plan_review ⇄ plan_revision → implementing ⇄ impl_phase_review → impl_review ⇄ impl_revision → reflecting → reflection_review ⇄ reflection_revision → completed
                                       ↓                                        ↑
                                       └─── (skip plan, human decides) ─────────┘

Amendments to frozen phases (spec, plan, reflection) are possible at any time via *.amendments.md — these are document events, not state transitions.
```

**Optional plan phase:** After spec review sign-off, the human may choose to skip the plan phase and proceed directly to `implementing`. This is an alternative edge chosen by the human — not a default. Use it for small, well-understood tracks where the spec provides sufficient implementation guidance.

During `implementing`, bounded review loops (`implementing → impl_phase_review → implementing`) can occur at phase boundaries. These are incremental reviews — a reviewer checks completed phases while implementation continues. The `impl_phase_review` state is optional; tracks may proceed directly to `impl_review` if phase-level review is unnecessary.

After all tasks complete, the track transitions to `impl_review` — the final whole-track implementation review. This gate verifies cross-phase integration, spec/plan conformance, acceptance-criteria closure, and unresolved findings before reflection. Implementation-phase reviews do not substitute for the final review.

**Documents:**
- `spec.md` — what and why (frozen after approval)
- `spec.review.md` — spec review conversation
- `plan.md` — phased tasks (progress updated during implementation)
- `plan.review.md` — plan review conversation
- `*.amendments.md` — append-only amendment log for changes to frozen artifacts (e.g., `spec.amendments.md`, `plan.amendments.md`, `reflection.amendments.md`). Amendment reviews are appended to the phase's existing review document.
- `impl.phase.review.md` — implementation-phase review conversation (incremental, during `implementing`)
- `impl.review.md` — final whole-track implementation review conversation
- `reflection.md` — semantic delta after implementation
- `reflection.review.md` — reflection review conversation
- `status.yaml` — state machine position + transition history

### Milestone lifecycle

Milestones are delivery-scoped — they represent a human-meaningful threshold at which the project is usable for a named consumer or rollout target. Unlike proposals (intent) and tracks (execution), milestones describe outcomes that typically span multiple proposals and tracks.

```
draft → draft_review ⇄ draft_revision → active → reflecting → reflection_review ⇄ reflection_revision → completed
  ↓           ↓                           ↓ ↑
abandoned  abandoned       superseded / abandoned + amend loop
                                        ↓
                             amend → amend_review ⇄ amend_revision → active
```

Milestones follow the same universal lifecycle patterns as proposals and tracks: frozen document after review, append-only amendments, reflection before completion. The distinction between artifact kinds is their *scope* (delivery vs. intent vs. execution), not their lifecycle machinery.

- A milestone starts as `draft` — the agent drafts content in the engine-served draft phase, then the review gate evaluates it.
- Human approval of draft review constitutes activation — the milestone transitions to `active`. The human exercises timing judgment by choosing *when* to approve, not by a separate post-review gate.
- The human moves an active milestone to `reflecting` when they judge the success criteria are met (or are no longer relevant). Completion is human judgment informed by linked work and stated criteria, not a mechanical rollup of track/proposal states.
- Active milestones can receive amendments via `milestone.amendments.md` (state-machine mode: `active → amend → amend_review ⇄ amend_revision → active`).
- A milestone can be `superseded` from `active` (linked to a successor via transition note) or `abandoned` from `draft`, `draft_review`, or `active`.
- The human-as-reviewer fast path applies at every review gate, same as proposals and tracks.

**Documents:**
- `milestone.md` — outcome statement, why it matters, contributing work (required vs. helpful), success criteria, current delivery judgment and open risks (frozen after draft review)
- `milestone.review.md` — append-only review conversation
- `milestone.amendments.md` — append-only amendment log for post-freeze changes (optional)
- `reflection.md` — semantic delta before completion
- `reflection.review.md` — reflection review conversation
- `status.yaml` — lifecycle position + transition history (`kind: milestone`, no `proposal:` field)

### Decision lifecycle

Decisions record institutional knowledge — what was decided, what was rejected, and why. They start as tensions (unresolved questions) and mature into decided artifacts with rationale and validity conditions.

```
tension → tension_review ⇄ tension_revision → investigating → decided → retired
  ↓                                              ↓
  └──────────────────────────────────────────────┘  (direct resolution)
                                                  ↓
                                    decided → decision_review ⇄ decision_revision → decided
                                      ↓
                                    amend → amend_review ⇄ amend_revision → decided
```

| State | Meaning | Transitions from |
|---|---|---|
| `tension` | Question identified, unresolved | — (initial state) |
| `investigating` | Active work to resolve (linked assessment track) | `tension` |
| `decided` | Resolved with rationale and validity conditions | `tension`, `investigating` |
| `retired` | No longer relevant | `decided` |

**Freeze-point exception:** `definition.md` is not frozen after tension review. The tension review validates the question, not the answer. The freeze point is after the *resolution* is reviewed and accepted (`decision_review` sign-off). After resolution review, the standard freeze-and-amend pattern applies.

**Direct transition:** `tension` → `decided` is valid for questions resolved through direct human judgment without a formal investigation.

**Documents:**
- `definition.md` — the question (as tension), answer + rationale (as decided)
- `evidence.md` — append-only support/contradiction over time
- `review.md` — append-only review conversation
- `amendments.md` — post-resolution-freeze changes (optional)
- `reflection.md` and `reflection.review.md` — created later in the reflecting phase, not at directory creation time
- `status.yaml` — lifecycle position + transition history (`kind: decision`)

**Evidence accumulation:** Decision evidence is append-only and event-sourced. Event types: `tested`, `tension-surfaced`, `validity-shifted`. Evidence enters the system through two paths:

- **In-band:** Track reflections (knowledge delta lens) → harvested during terminal transitions
- **Out-of-band:** Direct appends to `evidence.md` for observations outside normal reflection

### Learning lifecycle

Learnings record observations about reality that mature through interpretation and validation. They start as raw observations (what was seen), advance through conclusion (what it means), and mature into established knowledge (confirmed across contexts). Terminal paths are `retired` (no longer relevant) and `graduated` (absorbed into a higher-order initiative or decision).

```
observation → observation_review ⇄ observation_revision → conclusion → conclusion_review ⇄ conclusion_revision → established → graduated / retired
                                                                            │                                        ↓
                                                                            └──→ established                       amend → amend_review ⇄ amend_revision → established
                                                                              (conditional edge on
                                                                               "direct-experience establishment"
                                                                               transition note)
```

| State | Meaning | Transitions from |
|---|---|---|
| `observation` | Raw signal recorded, not yet interpreted | — (initial state) |
| `conclusion` | Interpretation drawn, awaiting cross-context validation | `observation`, — (initial state via `--conclude`) |
| `established` | Validated across contexts, load-bearing knowledge | `conclusion`, `conclusion_review` (via `--establish` path) |
| `graduated` | Absorbed into an initiative or decision | `established` |
| `retired` | No longer relevant | `observation`, `conclusion`, `established` |

**Entry points.** The `capture` skill creates a learning at one of three initial states depending on the maturity flag; these variants are not transitions from a prior state — they select the learning's starting state at creation time:
- `capture` (no flag) → `observation`
- `capture --conclude` → `conclusion` (observation and interpretation authored together, skipping observation review)
- `capture --establish` → `conclusion_review` (carries the transition note `"direct-experience establishment"`)

**Freeze-point exception:** `definition.md` is not frozen after observation review. The observation review validates the signal, not the interpretation. The freeze point is after the *conclusion* is reviewed and accepted (`conclusion_review` sign-off). After conclusion review, the standard freeze-and-amend pattern applies.

**Direct transition:** `conclusion_review` → `established` is valid via the conditional edge shown in the diagram, triggered when the learning's initial transition carries the `"direct-experience establishment"` note (the `capture --establish` path). The transition note string is load-bearing: the reviewing agent inspects the initial transition to decide whether review sign-off advances to `conclusion` (default) or `established` (this path).

**Documents:**
- `definition.md` — the observation (as observation), observation + interpretation (as conclusion or established)
- `evidence.md` — append-only progression log across contexts
- `review.md` — append-only review conversation (single file, all review phases)
- `amendments.md` — post-conclusion-freeze changes (optional)
- `reflection.md` and `reflection.review.md` — created later in the reflecting phase, not at directory creation time
- `status.yaml` — lifecycle position + transition history (`kind: learning`)

**Evidence accumulation:** Learning evidence is append-only and event-sourced. Event types: `observed`, `confirmed`, `contradicted`, `graduated`. Evidence enters the system through two paths:

- **In-band:** Track reflections (knowledge delta lens, `confirmed`/`contradicted` entries naming an existing learning) → harvested by the engine's terminal-transition processing (the `snapshot` RPC) into `forge/learnings/{name}/evidence.md`
- **Out-of-band:** Direct appends to `evidence.md` — this path produces `observed` events (additional instances of the pattern noticed in a different context) as well as any other observations outside normal reflection

## The review loop

Every phase follows the same sub-loop:

```
doing agent produces artifact
  → reviewer creates/appends to review doc
  → doing agent responds (timestamped entry in review doc)
  → reviewer amends
  → ... (repeat until consensus)
  → human signs off
  → next phase
```

Review documents are **append-only**. Each round has a timestamped review entry and a timestamped response. Prior rounds are never edited. Sign-off is recorded in the document.

Different agents handle different roles. The human mediates handoffs between them. A planning agent should not implement. An implementing agent should not review their own work. A reviewing agent should not implement fixes (unless the human asks).

## Exceptions

No exception to an active workflow expectation becomes authoritative without explicit human approval.

- An agent may **propose** an exception (in a review response, plan note, or amendment)
- A reviewer may **flag or challenge** an exception
- Only a human may **approve** an exception
- Work that depends on an unapproved exception must be treated as **non-conformant** during review and closure

This rule applies to deviations from the lifecycle, from spec/plan requirements, from review findings, and from any other active workflow expectation. Agents do not grant themselves exceptions.

## Proposal amendments

A frozen `proposal.md` is never edited after approval. When track execution teaches something that changes proposal-level intent:

1. The implementing or reflecting agent records the learning in `proposal.amendments.md` (append-only)
2. A reviewer reviews the amendment — the review is appended to `proposal.review.md` (the phase's existing review document, not a separate file)
3. A human approves the amendment before it becomes authoritative

**When to amend vs. reflect locally:**
- If the learning changes the proposal's strategic direction, scope, or approach: **amend the proposal**
- If the learning is specific to one track's execution: **keep it in track reflection**
- If in doubt: keep it local. Amendments are for proposal-level intent, not implementation details.

Only reviewed and human-approved amendments are authoritative. An unapproved amendment is a proposal, not a decision.

## How work is driven

There are no lifecycle skills. There is no skill to invoke for any phase of any
artifact's lifecycle, and the engine will never hand you the name of one.

The anvil engine is the front door. `begin` / `complete` / `snapshot` / `amend`
drive every lifecycle phase, and each phase's guidance is served to you as the
machine's state hook (see `playbooks/*/hooks/`) — delivered inline in the
response's `context_text`, not addressed by name for you to go fetch.

To act on any artifact: call the engine and follow the returned `context_text`.
Each action carries an `execution_route` discriminator, which has exactly two
values:

- `engine` — call the engine to execute it.
- `none` — no action exists for your role on that subject.

Deterministic bookkeeping (status.yaml mutation, registry sync, incremental
projection updates) is engine-owned and compiled, recorded as part of the
`complete` / `snapshot` RPCs. The non-deterministic remainder — narrative
truth.md synthesis, evidence harvesting from reflections, forward.md edits, and
full projection regeneration on terminal transitions — is served as the
machine's terminal-state hooks, again as content in `context_text`.

Actor identity is supplied per call: every `begin` / `complete` / `snapshot`
carries an explicit `actor_name` plus the runtime `actor_*` fields detected from
your current environment. The engine rejects empty values with
`actor_name_required` / `actor_params_required`.

## Registries

### forge/milestones.md

Ordered list of milestones grouped by status (active, draft, completed, superseded, abandoned). Active-first ordering because milestones are scanned for "what's the current delivery target?" Each entry links to the milestone directory. Positional order within each state group is the priority.

### forge/initiatives.md

Ordered list of initiatives grouped by status (draft, active, promoted, retired). Descriptive kebab-case names, not date-prefixed. Positional order within each state group is the priority.

### forge/decisions.md

Ordered list of decisions grouped by status (tension, investigating, decided, retired). Descriptive kebab-case names, not date-prefixed. Positional order within each state group is the priority.

### forge/proposals.md

Ordered list of proposals grouped by status (completed, active, draft). Each entry links to the proposal directory. This registry **is** the roadmap — there is no separate ROADMAP.md. Positional order within each state group is the priority.

### forge/tracks.md

Ordered list of tracks with status and proposal link. Positional order within each state group is the priority.

## Priority

Priority is **stack rank — positional order within each state group** in the registry files (`milestones.md`, `proposals.md`, `tracks.md`). The registries are the source of truth for priority.

- Position 1 in a state group is highest priority within that group.
- Ranking is per-state-group. Cross-group comparison is not meaningful — rank 1 among `draft` proposals and rank 1 among `active` proposals are independent orderings.
- No named priority levels, no numeric scores. Position alone determines priority.
- Priority is separate from lifecycle state. State says what phase work is in; position within a state group says how important it is relative to peers.

**Git history on registry files is the authoritative append-only audit trail for prioritization.** No separate priority-change log is maintained. When items are reordered, the commit message must include the rationale for the reordering. An agent may propose a reprioritization, but it does not become authoritative until a human approves the registry change.

## Blocking

Tracks and proposals may have dependencies that prevent work from starting regardless of priority. Blocking relationships are owned by `status.yaml` via an optional `blocked_by` field:

```yaml
version: 1
kind: track
state: plan
proposal: 20260402T0000_contract_features
blocked_by:
  - adapter_automation_20260331
transitions:
  # ...
```

- `blocked_by` lists artifact directory names (tracks, proposals, or milestones) that must reach a terminal state (`completed`, `superseded`, `abandoned`) before this item can proceed.
- The agent starting the blocked track is responsible for verifying blockers are cleared and removing them from `blocked_by`. Git history preserves the record of past blocking.
- Registries may project blocking status when listing items, but `status.yaml` is the source of truth — not the registry.
- The implementing phase warns (does not hard-gate) when starting a track whose blockers have not been cleared.

Blocking complements priority. Priority says "work on this first by choice"; blocking says "this cannot proceed yet regardless of priority." Both are needed for an agent to answer "what should I do next?"

## Projections

The project's current state is projected into five layer-specific files under `forge/projections/`:

| File | Layer | Contents |
|------|-------|----------|
| `intent.md` | State of intent | Milestones and proposals by status, preserving registry ordering as the priority projection |
| `execution.md` | State of execution | In-flight tracks with state machine positions, preserving registry ordering |
| `truth.md` | Architectural truth | Active invariants, intentional exceptions, deprecated assumptions (from reflection projection deltas) |
| `forward.md` | Forward projection | What's next and why; notes significant registry reorderings since last generation |
| `decisions.md` | Decisions | State counts, tensions by priority, recently resolved — not read at conversation start; read by decision triage agents and discovery habit searches (which also cover `learnings.md` alongside decisions) |
| `sparks.md` | Sparks | Untriaged spark counts, annotation counts, last reflection date — count line updated by the `snapshot` MCP tool on spark/annotation events; "Last reflection:" narrative line updated by the terminal-transition hook |

Deterministic projection layers (execution.md, intent.md, decisions.md, sparks.md count line) are updated incrementally by the `snapshot` MCP tool on every state transition. Narrative layers (truth.md, forward.md) and terminal-transition evidence harvesting are updated by the terminal-transition hook. Each file carries its own frontmatter (`incremental_count`, `base_snapshot`, `last_updated`, `after_event`). Full rebuilds from the last human-verified base are regenerated inline as part of the terminal transition itself, for track/proposal terminals and for milestone supersede/abandon. Agents read the projection files relevant to their work at conversation start.

## Spark capture

Any agent may capture a spark as a non-blocking subagent when encountering an idea that doesn't belong in the current artifact. This is the single authoritative location for this guidance.

**When to spark:** You notice a cross-cutting concern, a potential improvement, a question that doesn't belong in your current spec/plan/review/reflection — but you don't want to lose it. Capture the spark with the idea and continue your primary work.

**How:** Invoke as a non-blocking subagent passing `body` (the idea), `actor` (your name), and `origin` (your current artifact and phase). The spark subagent reads undispositioned sparks, determines whether to capture a new spark or annotate an existing one, and updates the sparks projection. You never load the sparks log.

**Coordination model:** The sparks directory has no `status.yaml`. There is no state machine. Capture and annotate are fire-and-forget. Spark reflection is always explicitly requested by the human — never auto-detected by agents. No agent tasked with specific work is responsible for deciding when to reflect on sparks.

**Dispositions:** Reflection triages each spark to one type — `vision`, `track`, `initiative`, `tension`, `observation`, `noted`, `dismissed`, or `merged`. The `observation` type bridges a spark to a learning capture (a noticing about system or agent behavior), exactly as `tension` bridges to a decision.

## Directory structure

```
forge/
├── projections/
│   ├── intent.md                            # Layer 1: milestones + proposals by status
│   ├── execution.md                         # Layer 2: tracks by state
│   ├── truth.md                             # Layer 3: architectural invariants
│   ├── forward.md                           # Layer 4: what's next, priorities
│   ├── decisions.md                         # Decisions: state counts, tensions, recently resolved
│   └── sparks.md                            # Sparks: untriaged counts, annotation counts
├── sparks/
│   ├── sparks.md                            # append-only event log (spark, annotation, disposition)
│   ├── reflection.md                        # append-only synthesis entries (optional)
│   └── reflection.review.md                 # reflection review (optional)
├── initiatives.md                           # initiative registry (convergence pressures)
├── decisions.md                             # decision registry (institutional knowledge)
├── milestones.md                            # milestone registry (delivery targets)
├── proposals.md                             # proposal registry (the roadmap)
├── tracks.md                                # track registry
├── initiatives/
│   └── {descriptive-kebab-case}/
│       ├── status.yaml
│       ├── definition.md                  # projected current truth
│       ├── review.md                      # append-only review
│       ├── amendments.md                  # definition changes
│       ├── evidence.md                    # progression log
│       ├── reflection.md                  # synthesis entries (optional)
│       └── reflection.review.md           # reflection review (optional)
├── decisions/
│   └── {descriptive-kebab-case}/
│       ├── status.yaml
│       ├── definition.md                  # question (tension) + answer (decided)
│       ├── evidence.md                    # append-only support/contradiction
│       ├── review.md                      # append-only review
│       └── amendments.md                  # post-resolution-freeze changes
├── milestones/
│   └── {YYYYMMDD}T{HHMM}_{name}/
│       ├── status.yaml
│       ├── milestone.md                    # frozen after draft review
│       ├── milestone.review.md             # append-only review
│       ├── milestone.amendments.md         # post-freeze changes (optional)
│       ├── reflection.md
│       └── reflection.review.md
├── proposals/
│   └── {YYYYMMDD}T{HHMM}_{name}/
│       ├── status.yaml
│       ├── vision.md
│       ├── vision.review.md
│       ├── proposal.md
│       ├── proposal.review.md
│       ├── *.amendments.md                  # append-only amendment logs for frozen phases (optional)
│       ├── reflection.md
│       └── reflection.review.md
└── tracks/
    └── {YYYYMMDD}T{HHMM}_{name}/
        ├── status.yaml
        ├── spec.md
        ├── spec.review.md
        ├── plan.md
        ├── plan.review.md
        ├── *.amendments.md                  # append-only amendment logs for frozen phases (optional)
        ├── impl.phase.review.md             # incremental implementation-phase reviews (optional)
        ├── impl.review.md                   # final whole-track implementation review
        ├── reflection.md
        └── reflection.review.md
```

Date-first naming ensures chronological ordering when listing directories.

## status.yaml

Every track, proposal, milestone, and initiative has a `status.yaml` with the full transition history and an actors table identifying participants.

### Schema

```yaml
version: 1
kind: track  # or 'proposal', 'milestone', 'decision'
state: impl_review
proposal: 20260403T0900_contract_features  # tracks link to their source proposal
actors:
  mark:
    type: human
  carol-8821:
    type: agent
    configurations:
      - at: 2026-04-03T09:00:00
        model: claude-opus-4-6
        provider: anthropic
        details:
          context_window: 1000000
          sdk_version: "0.2.87"
          entrypoint: claude-desktop
      - at: 2026-04-03T16:00:00
        model: claude-sonnet-4-6
        provider: anthropic
        details:
          context_window: 200000
          sdk_version: "0.2.87"
          entrypoint: claude-desktop
  dave-3317:
    type: agent
    configurations:
      - at: 2026-04-03T10:00:00
        model: claude-sonnet-4-6
        provider: anthropic
        details:
          context_window: 200000
          sdk_version: "0.2.87"
          entrypoint: claude-desktop
transitions:
  - to: spec
    at: 2026-04-03T09:00:00
    actor: carol-8821
    role: spec
    approver: mark
  - to: spec_review
    at: 2026-04-03T10:00:00
    actor: dave-3317
    role: review
    note: "sign-off achieved"
  - to: plan
    at: 2026-04-03T12:00:00
    actor: carol-8821
    role: plan
    approver: mark
  - to: implementing
    at: 2026-04-03T15:00:00
    actor: carol-8821
    role: implement
    approver: mark
  - to: impl_review
    at: 2026-04-04T12:00:00
    actor: dave-3317
    role: review
    note: "all tasks complete, final review"
```

### Transition fields

| Field | Required | Description |
|-------|----------|-------------|
| `to` | yes | Destination state |
| `at` | yes | ISO 8601 timestamp |
| `actor` | yes | Identifier from the `actors` table (or a bare string for legacy transitions) |
| `role` | yes | Freeform string — what role the actor played (e.g., `spec`, `review`, `implement`, `reflect`, `complete`, `amend`, `envision`, `propose`, `approve`) |
| `approver` | no | Identifier of the actor who authorized the transition — most commonly a human approving an agent's work |
| `note` | no | Human-readable context |

### Legacy compatibility

Existing `status.yaml` files use bare `agent:` strings (e.g., `agent: spec-agent`) without an actors table. These remain valid. When a skill writes a new transition to an existing file with bare `agent:` strings, it adds the current actor to the actors table and writes new transitions using the `actor:`/`role:` format. Old transitions are left unchanged. Bare strings without actors table entries are valid — they simply have no structured identity record.

## Actor identity

### What is an actor?

An **actor** is any participant in the forge lifecycle. Actors have a `type`:

- **`human`** — a person. Identity is self-evident. Multiple humans may participate in a single track.
- **`agent`** — a threaded LLM conversation. One conversation = one agent. The agent is defined by its underlying record of inputs, outputs, and tool calls. It is born when the conversation starts and dies when it ends. A different conversation picking up Phase 2 of the same track is a different agent with a different name.

The type system is open to future modalities (single API calls, CI bots, etc.) without schema changes. Agent identity is conversation-scoped by definition — there is no centralized actor registry across conversations. Each `status.yaml` records the actors that touched it independently.

### Actor record fields

**Common fields (all actor types):**

| Field | Required | Description |
|-------|----------|-------------|
| `type` | yes | Actor type: `human`, `agent` |

**Agent-type fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `configurations` | yes | Append-only list of timestamped configuration entries |

**Configuration entry fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `at` | yes | ISO 8601 timestamp — when this configuration became active |
| `model` | yes | Model identifier, cross-provider (e.g., `claude-opus-4-6`, `gpt-4o`, `gemini-2.5-pro`) |
| `provider` | yes | Provider name (e.g., `anthropic`, `openai`, `google`) |
| `details` | no | Provider-specific configuration block |

**Anthropic/Claude Code details:**

| Field | Description | Source env var |
|-------|-------------|---------------|
| `context_window` | Context window size in tokens | (derived from model) |
| `sdk_version` | Claude Agent SDK version | `CLAUDE_AGENT_SDK_VERSION` |
| `entrypoint` | How Claude Code was launched | `CLAUDE_CODE_ENTRYPOINT` |

Human actors have `type: human` with no configurations list.

### Naming convention

Actor names are **project-declared**. Each project defines its naming strategy. For example:

- **Format:** `{Word}-{6 digits}` — a proper noun from the system dictionary + a 6-digit random suffix. Examples: `Tacoma-384921`, `Delhi-904507`, `Elinor-573835`.
- **Generation command:** agents run this to produce their name (do not invent or reuse names from existing `status.yaml` files):
  ```bash
  W=$({ grep '^[A-Z][a-z]\{3,7\}$' /usr/share/dict/words || grep '^[a-z]\{4,8\}$' /usr/share/dict/words | awk '{print toupper(substr($0,1,1)) substr($0,2)}'; } | awk -v s=$(od -An -tu4 -N4 /dev/urandom | tr -d ' ') 'BEGIN{srand(s)}{a[NR]=$0}END{print a[int(rand()*NR)+1]}') && printf '%s-%06d\n' "$W" $(($(od -An -tu4 -N4 /dev/urandom | tr -d ' ') % 1000000))
  ```
- Names identify the entity, not the task — not role-derived, not model-derived.
- Human actors use their actual name (e.g., `mark`, `nick`).
- Names are **unique within a single `status.yaml`**. A collision (two different actors generating the same name) is an error — run the command again.
- Names are **consistent across artifacts within a single conversation**. If an actor specs a track, reviews a proposal amendment, and updates a milestone in the same conversation, it uses the same name in all three `status.yaml` files. Each file records the actor's details independently so artifacts are self-contained.

### Mid-conversation parameter changes

If the human switches the model or thinking level mid-conversation, the actor's identity parameters change but the conversation (and therefore the actor) continues. The `configurations` list records this: a new timestamped entry is appended with the updated model/provider/details. The previous configuration is preserved — to determine what model was running at any transition, find the most recent configuration entry that predates the transition's `at` timestamp. No configuration data is ever overwritten or removed.

## Reflection

The reflection captures the **semantic delta between plan and outcome** through four lenses:

1. **Intent delta** — how did our understanding of the goal change?
2. **Reality delta** — what did reality force us to learn?
3. **Exception delta** — what mismatches remain, and are they intentional?
4. **Projection delta** — what should future agents now treat as true?

Reflections are the primary input to the snapshot's "architectural truth" layer. The projection delta is especially important — it's how new invariants, deprecated assumptions, and unresolved tensions propagate to future work.

For tracks, reflection includes a fifth lens:

5. **Knowledge delta** — which active initiatives did this track advance or regress? Which decisions were tested by this track's experience? Which tensions surfaced during this track's work? Which validity conditions shifted based on what we learned? Which existing learnings did this track's experience confirm or contradict? Which new patterns surfaced during this work that should be captured as learnings?

This lens tracks against all standing knowledge systems (initiatives, decisions, and learnings). `confirmed`/`contradicted` entries against existing learnings are harvested from this section by the engine's terminal-transition processing (the `snapshot` RPC's terminal-state hook) into initiative `evidence.md`, decision `evidence.md`, and learning `evidence.md` files. New-pattern entries are not harvested — they point the reflecting agent (or a later agent) to capture a new learning via `begin(artifact_type: "learning")` either inline during reflection or as follow-up.

## Initiatives

Initiatives are **cross-cutting implementation expectations** the codebase is converging toward. They describe *how* things should be built, not *what* the system should do. Proposals are behavioral (what to build); initiatives are convergence pressure (how to build it).

### Directory structure

Each initiative is a directory under `forge/initiatives/` with descriptive kebab-case naming (not date-prefixed — initiatives are standing patterns, not time-ordered work):

```
forge/initiatives/{name}/
├── status.yaml          # lifecycle state + transition history
├── definition.md        # projected current truth (pattern, guidance, status)
├── review.md            # append-only review conversation
├── amendments.md        # append-only changes to the definition
├── evidence.md          # append-only progression log
├── reflection.md        # append-only synthesis entries (optional, after first reflection)
└── reflection.review.md # reflection review conversation (optional)
```

### Initiative lifecycle

```
draft → review ⇄ revision → active → promoted / retired
                                ↕
                           reflecting
```

- **Draft:** Created in the initiative draft phase. Goes through the review gate before activation.
- **Draft → Active:** Human approval of draft review constitutes activation — same convention as milestones. The reviewer or human records the `active` transition after review sign-off.
- **Active:** Enforced during the forge lifecycle. Evidence accumulates. Definition can be amended.
- **Promoted:** Hardened into a CLAUDE.md rule. Evidence tracking continues. The rule is the enforcement mechanism; the initiative is the tracking mechanism.
- **Retired:** No longer enforced. Kept for history.

All status transitions are human-approved.

### Relationship to CLAUDE.md rules

Initiatives and CLAUDE.md rules are the same system at different points on a continuum. CLAUDE.md rules are binary constraints with no evidence trail. Initiatives add four properties: gradation, cumulative evidence, exception tracking, and lifecycle.

Existing CLAUDE.md rules become promoted initiatives — they keep their CLAUDE.md presence but gain evidence tracking. New convergence pressures start as active initiatives and may be promoted when evidence supports it. A rule generating persistent exceptions may be demoted back to active.

### Evidence accumulation

Evidence is append-only and event-sourced. Event types: `advance`, `regress`, `exception-proposed`, `exception-approved`, `promoted`, `demoted`, `retired`. Evidence enters the system through two paths:

- **In-band:** Track reflections (knowledge delta lens) → harvested during terminal transitions
- **Out-of-band:** the initiative log phase, for observations outside normal reflection

### Initiative reflection

Initiative reflection is a seventh mode. Unlike single-shot track/proposal/milestone reflection, initiative reflection is **append-only and repeatable** — each invocation appends a timestamped synthesis entry. The synthesis covers convergence assessment, definition effectiveness, exception assessment, and lifecycle readiness. Every initiative reflection must be followed by a review gate.

### Review gate

- **For CLAUDE.md-derived (promoted) initiatives:** review validates **definition accuracy** — does the initiative correctly capture the rule?
- **For reflection-derived (active) initiatives:** review validates **pattern validity** — is the pattern real, well-scoped, enforceable?

## Commit convention

The forge lifecycle uses **transitions and commits as paired signals**. A status.yaml transition opens the bracket — it records "this phase started." A git commit closes the bracket — it records "this phase's work is done and persisted." Together they make each lifecycle step complete. A transition without a closing commit is an open bracket; a commit without a transition is orphaned work.

### When to commit

**Artifact phases** (envision, propose, spec, plan, reflect, amend, milestone, complete): commit once after human approval. The commit bundles the phase artifact + any snapshot bookkeeping into one commit. This is the closing bracket for the phase.

**Implementation** (implement): commit per-task after green. Each commit includes code changes + plan.md status update (`[x] <commit-sha>`). Checkpoint commits at phase boundaries include plan.md `[checkpoint: <sha>]` update.

**Review** (review): the reviewer commits the review document standalone after producing it. The reviewer's transition opens the bracket; the reviewer's commit closes it. This is defensive — the review document survives if the author's session fails.

**Revision modes** (spec_revision, plan_revision, impl_revision, reflection_revision): commit after making revisions and appending dispositions to the review doc. Same commit pattern as the primary mode — the revision + review response + snapshot bookkeeping in one commit.

### Who commits

The **performing agent** — the agent who produced the artifact — commits it. The speccing agent commits the spec. The reviewer commits the review. The implementing agent commits each task. The reflecting agent commits the reflection. If a session ends before the commit, the next agent detects the gap (see verification below) and flags it.

### Verification at session start

Skills that continue work on an existing artifact after a prior phase verify that the previous phase's bracket is closed before proceeding. The check: read the last transition from status.yaml, check `git log` for a closing commit that corresponds to that transition. If no matching commit exists, warn the human — do not hard-stop, but make the open bracket visible before building on potentially uncommitted state.

**Phases that verify the prior phase:** plan (after spec), implementing (after plan), each review gate (after the producing agent's phase), reflecting (after impl_review), completion (after reflection_review), proposal (after vision_review).

**Phases that don't:** vision (starts a new proposal — no prior phase), spec (starts a new track), milestone draft (starts a new milestone), and amend (amendments happen at any time during `active` — no sequential prior phase).

## Authoring and registering playbooks

A **playbook** is the reusable lifecycle definition artifact (`machine.yaml` + hooks). One execution driven by a playbook is a **playbook run**. To author a new playbook, ship it, register it, and verify it live, read:

**`docs/authoring-playbooks.md`** — covers the machine model, the full `machine.yaml` schema, measurement enforcement, the real package → install → registrar path, the `persist_playbook` and `candidate_playbook_intake` authoring surfaces, the build-distribute-verify-retire-skill discipline, and verification steps.

Vocabulary is canonical: `docs/vocabulary.md`.

## For agents starting a conversation

1. Read PHILOSOPHY.md, CLAUDE.md, and the projection files under `forge/projections/` relevant to your work (including `truth.md` for active initiatives)
2. Check whether your task relates to an existing track or proposal
3. Use the appropriate dev-workflow skill for your role in the lifecycle
