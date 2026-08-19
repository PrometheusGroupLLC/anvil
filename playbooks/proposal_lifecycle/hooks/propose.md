# Forge Propose

Develop the concrete approach for an approved vision — research the codebase, write `proposal.md`, and move the proposal from `vision` to `draft`.

This phase runs after the envision phase and its vision review. The vision defines the direction; this phase defines the approach.

**FIRST:** Read context.

- PHILOSOPHY.md
- CLAUDE.md
- forge/projections/ (read the files relevant to your work)
- The proposal's `vision.md` and `vision.review.md`

Verify the proposal has an approved vision (check `status.yaml` — the vision review must have human sign-off). If the vision has not been reviewed, stop — the vision-review phase must complete first (a reviewer calls `begin(identifier: <artifact_path>)` then `complete(... satisfaction)`).

**Verify previous phase committed:** Check that the vision review phase has a closing commit — read the last transition from status.yaml and check `git log` for a corresponding commit. If no matching commit exists, warn the human before proceeding. See AGENTS.md "Commit convention — Verification at session start."

---

## Phase 1: Research

Based on the approved vision, research the codebase and existing proposals:
- What exists today that relates to this direction?
- What proposals or tracks have touched adjacent areas?
- What patterns, constraints, or invariants apply?
- What prior reflections (if any) inform this direction?

### Search existing decisions and learnings

Before developing the approach, search for relevant prior deliberation:

1. Read `forge/decisions.md` and `forge/learnings.md` — scan for decided decisions in the area, alternatives already evaluated and rejected, validity conditions on existing decisions that the proposal might trigger, and prior observations, conclusions, and established learnings in the approach's domain
2. If `forge/projections/decisions.md` exists, scan it for summary data (tension counts, recently resolved decisions), and if `forge/projections/learnings.md` exists, scan it for state counts, recent observations, and nearing-establishment conclusions
3. If matches found, read the matching `forge/decisions/{name}/definition.md` or `forge/learnings/{name}/definition.md`
4. Use found decisions to avoid proposing approaches already evaluated and rejected. Reference existing decisions that constrain or inform the approach. For learnings, treat established learnings as standing knowledge the approach should respect, conclusions as provisional guidance, and observations as context that may ripen.

This search is guidance, not a gate. If no relevant decisions or learnings exist, proceed normally.

Present findings to the human. Get confirmation that the research is complete.

---

## Phase 2: Write the proposal

Add `proposal.md` to the existing proposal directory.

**proposal.md** — the concrete approach:
- Proposed approach (informed by research and the approved vision)
- Key decisions and trade-offs
- Relationship to existing work
- Open questions

Write proposal.md to the proposal directory.

Notify the human that the artifact is ready for review:
- Provide a 1-3 sentence summary of what the document covers
- State the file path for editor review
- Prompt: "Review in your editor and let me know when you're ready to proceed."

Human approval gates the commit, not the file write. If the human requests changes, revise the file in place.

---

## Phase 3: Commit

The engine has already updated the registry. Create a conventional commit: `propose(forge): {proposal description}`

---

## After completion

The proposal is now in `draft` state. Next step options:

- **Agent review** — call `complete(artifact_path, actor_*)` to advance; a reviewer then calls `begin(identifier: <artifact_path>)` to receive the engine-served review guidance, which challenges the proposal's approach, feasibility, and completeness. The human mediates. Once approved (reviewer's `complete(... satisfaction: "satisfied")`), the proposal moves to `active`.
- **Human-as-reviewer** — the human may approve the proposal directly. Record a minimal sign-off entry in `proposal.review.md`; the reviewer records the transition via `complete`. The transition note records that the human acted as reviewer.

---

## Revision mode (responding to proposal review)

When the reviewer returns findings in `proposal.review.md`:

1. Read `proposal.md` and `proposal.review.md`
2. Append a response timestamped via `date -u +%Y-%m-%dT%H:%M:%SZ` to `proposal.review.md` with explicit dispositions for every finding:
   - **"Will address"** — describe the change you'll make
   - **"Acknowledged, not addressing"** — explain why (design choice, out of scope, etc.)
   No finding may be silently skipped, regardless of severity.
3. Revise `proposal.md` as needed
4. Commit the revision: `propose(forge): {proposal description} — address review findings`. See AGENTS.md "Commit convention."
5. The human or reviewer advances state back to `proposal_review` when ready for re-review.

---

## Critical rules

1. Read context files FIRST — especially the approved vision and its review
2. Verify the vision has been reviewed and approved before starting
3. Research is mandatory — understand what exists before proposing how
4. Write artifact to disk, then get human approval before committing
5. Do not write to status.yaml, registry files, or projection files directly — the engine handles deterministic bookkeeping
