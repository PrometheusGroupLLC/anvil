# Forge Envision

Frame a new strategic direction — the problem and desired future state, without committing to an approach.

This is the first phase of the proposal lifecycle. The vision is reviewed with a lighter touch ("is this worth pursuing? is the problem real?") before investing in the concrete approach (handled by the propose phase).

**FIRST:** Read context.

- PHILOSOPHY.md
- CLAUDE.md
- forge/projections/ (read the files relevant to your work)

---

## Phase 1: Identify the direction

Name the direction in one sentence and pick a directory name. The deeper Q&A (problem, outcome, scope, constraints) happens in Phase 2 after the proposal exists in the event log.

Ask the human:
- **What's this direction about?** A single-sentence framing — the detailed problem/outcome elicitation happens after the proposal is recorded.
- **What should we call the directory?** `{snake_case_name}` — stable once created.

---

## Create proposal directory

The engine has already created the proposal directory and recorded the initial `vision` state. Do not write to `status.yaml`, registry files, or projection files directly.

---

## Search existing decisions and learnings

Before deepening the Q&A, search for relevant prior deliberation:

1. Read `forge/decisions.md` and `forge/learnings.md` — scan for existing tensions that describe the same or overlapping problem, resolved decisions that constrain or inform the direction, related decisions the vision should reference, and prior observations, conclusions, and established learnings in the direction's domain
2. If `forge/projections/decisions.md` exists, scan it for summary data (tension counts, recently resolved decisions), and if `forge/projections/learnings.md` exists, scan it for state counts, recent observations, and nearing-establishment conclusions
3. Read `forge/sparks/sparks.md` — scan for undispositioned sparks pointing at the same problem from different angles
4. If matches found, read the matching `forge/decisions/{name}/definition.md` or `forge/learnings/{name}/definition.md`
5. Reference found decisions in the vision, build on existing tensions, or note where the direction diverges from prior deliberation. For learnings, treat established learnings as standing knowledge the direction should respect, conclusions as provisional guidance, and observations as context that may ripen. Search results inform the questions you ask the human.

This search is guidance, not a gate. If no relevant decisions, learnings, or sparks exist, proceed normally.

---

## Phase 2: Understand the direction

**Before writing the vision, deeply understand what the human wants and why.**

Ask about:
- **The problem.** What's broken, missing, or misaligned? What happens if we don't address it?
- **The desired outcome.** What should be true when this work is done? Who benefits?
- **The scope.** Is this one track or many? What's explicitly not in scope?
- **Constraints.** Deadlines, dependencies, existing decisions this must respect.

Do NOT rush this. A vague one-liner like "add resources" requires follow-up questions.

---

## Phase 3: Write the vision

Add vision.md to the proposal directory.

**vision.md** — the direction, not the approach:
- Problem (what's broken or missing)
- Desired future state (what the world looks like when this is done)
- Why this matters now
- Relationship to existing work

The vision does NOT include a concrete approach — that's the propose phase's job.

Write vision.md to the proposal directory.

Notify the human that the artifact is ready for review:
- Provide a 1-3 sentence summary of what the document covers
- State the file path for editor review
- Prompt: "Review in your editor and let me know when you're ready to proceed."

Human approval gates the commit, not the file write. If the human requests changes, revise the file in place.

Create a conventional commit: `vision(forge): {proposal description}`

---

## After completion

The vision is now ready for review. Next step options:

- **Agent review** — call `complete(artifact_path, actor_*)` to advance; a reviewer then calls `begin(identifier: <artifact_path>)` to receive the engine-served vision-review guidance. Vision review is lighter than proposal review: "is the problem real? is the desired future state compelling?" not "is the approach sound?"
- **Human-as-reviewer** — the human may approve the vision directly. Record a minimal sign-off entry in `vision.review.md`; the reviewer records the transition via `complete`. The transition note records that the human acted as reviewer.

Once the vision is reviewed and approved, the engine advances to the propose phase, which develops the concrete approach.

---

## Revision mode (responding to vision review)

When the reviewer returns findings in `vision.review.md`:

1. Read `vision.md` and `vision.review.md`
2. Append a response timestamped via `date -u +%Y-%m-%dT%H:%M:%SZ` to `vision.review.md` with explicit dispositions for every finding:
   - **"Will address"** — describe the change you'll make
   - **"Acknowledged, not addressing"** — explain why (design choice, out of scope, etc.)
   No finding may be silently skipped, regardless of severity.
3. Revise `vision.md` as needed
4. Commit the revision: `vision(forge): {proposal description} — address review findings`. See AGENTS.md "Commit convention."
5. The human or reviewer advances state back to `vision_review` when ready for re-review.

---

## Critical rules

1. Read context files FIRST
2. Phase 1 (understanding) is mandatory — never skip the conversation
3. The vision defines DIRECTION, not APPROACH — no implementation strategy, no technical design
4. Write artifact to disk, then get human approval before committing
5. Do not write to status.yaml, registry files, or projection files directly — the engine handles deterministic bookkeeping
