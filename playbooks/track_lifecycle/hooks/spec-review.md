# Forge Review

HOOK-MARKER: TRACK-LIFECYCLE-SPEC-REVIEW

Review a forge artifact. You are a **Principal Software Engineer** — meticulous, first-principles, firm but helpful.

**FIRST:** Read context: PHILOSOPHY.md, CLAUDE.md, forge/projections/ (files relevant to this review).

## Identify what to review

Check `{{args}}` for explicit scope (e.g., a track/proposal name, or an explicit stage like `impl_phase_review` or `amendment`). If empty, auto-detect:

1. Read forge/tracks.md and forge/proposals.md
2. Look for a track or proposal in a reviewable state:
   - Proposals: `vision`, `draft`, `amend`, `reflecting`
   - Tracks: `spec`, `plan`, `implementing` (for phase review), `impl_review`, `reflecting`
   - Milestones: `draft`, `amend`, `reflecting`
   - Initiatives: `draft`, `reflecting`
   - Decisions: `tension`, `decided`, `amend`
3. Ask the human to confirm

AGENTS.md is authoritative for state machine definitions — this protocol consumes lifecycle definitions, it does not define them. For ambiguous cases (e.g., reviewing a proposal amendment while the proposal is `active`), require an explicit stage override. Read the target's status.yaml to determine which phase needs review.

## Load context

Read ALL relevant artifacts: the artifact being reviewed, all prior review documents, the source proposal (for tracks), PHILOSOPHY.md and CLAUDE.md.

**Verify previous phase committed:** Check that the producing agent's phase has a closing commit — read the last transition from status.yaml and check `git log`. If no matching commit exists, warn the human. See AGENTS.md "Commit convention — Verification at session start."

## Actor identity and transition

Before your first status transition, register yourself via `forge:id`. If already registered in the target `status.yaml`'s actors table, skip. The review transition is engine-driven: you entered this state by calling `begin(identifier: <artifact_path>)` with a reviewer role, which delivered this guidance. After writing your review, record the transition by calling `complete` with the appropriate `satisfaction`:

- `complete(artifact_path, satisfaction: "satisfied", actor_*)` — accept and advance to the next phase.
- `complete(artifact_path, satisfaction: "full_revision", actor_*)` — send the artifact back for revision.

Do NOT write to status.yaml, registry, or projection files directly, and do NOT invoke a skill — the engine's `complete` RPC handles all bookkeeping for the transition.

## Dispatch to criteria

Read the criteria file for your review type. For amendment reviews, also read the base criteria file for the amended artifact type.

| Review type | Criteria file |
|---|---|
| Vision | `.claude/commands/forge/review/vision.md` |
| Proposal draft | `.claude/commands/forge/review/proposal.md` |
| Spec | `.claude/commands/forge/review/spec.md` |
| Plan | `.claude/commands/forge/review/plan.md` |
| Implementation phase | `.claude/commands/forge/review/impl-phase.md` |
| Final implementation | `.claude/commands/forge/review/impl-final.md` |
| Milestone draft | `.claude/commands/forge/review/milestone-draft.md` |
| Reflection (proposal/track/milestone) | `.claude/commands/forge/review/reflection.md` |
| Initiative definition | `.claude/commands/forge/review/initiative-definition.md` |
| Initiative reflection | `.claude/commands/forge/review/initiative-reflection.md` |
| Decision tension | `.claude/commands/forge/review/decision-tension.md` |
| Decision resolved | `.claude/commands/forge/review/decision-resolved.md` |
| Spark reflection | `.claude/commands/forge/review/spark-reflection.md` |

## Amendment reviews

Amendment reviews compose: apply these mechanism checks AND Read the base criteria file for the amended artifact type to evaluate substance.

1. Is the amendment necessary?
2. Is it factually accurate?
3. Is it properly scoped — not smuggling unrelated changes?
4. Does it follow append-only rules — no edits to the frozen source artifact?
5. Does it affect downstream work? For proposal amendments: check whether active tracks depend on assumptions the amendment changes. For milestone amendments: check whether contributing work analysis or success criteria changes invalidate in-progress tracks.
6. Does it require human approval before becoming authoritative?

Write to the phase's existing review document (e.g., `proposal.review.md`, `milestone.review.md`) — not a separate `*.amendments.review.md` file.

## Review artifact routing

| State | Review artifact |
|-------|----------------|
| `vision` | `vision.review.md` |
| `draft` (proposal) | `proposal.review.md` |
| `spec` | `spec.review.md` |
| `plan` | `plan.review.md` |
| `implementing` / `impl_phase_review` | `impl.phase.review.md` |
| `impl_review` | `impl.review.md` |
| `reflecting` | `reflection.review.md` |
| `draft` (milestone) | `milestone.review.md` |
| `amend` (milestone) | `milestone.review.md` |
| `reflecting` (milestone) | `reflection.review.md` |
| `amendment` (explicit stage) | phase's existing review doc |
| `draft` (initiative) | `review.md` (in initiative directory) |
| `reflecting` (initiative) | `reflection.review.md` (in initiative directory) |
| `tension` (decision) | `review.md` (in decision directory) |
| `decided` / `decision_review` (decision) | `review.md` (in decision directory) |
| `amend` (decision) | `review.md` (in decision directory) |
| Spark reflection (explicit invocation) | `forge/sparks/reflection.review.md` |

## Produce the review document

Create or append to the appropriate review artifact:

```markdown
# Review: {artifact name}

## Round N

### Review — {timestamp from `date -u +%Y-%m-%dT%H:%M:%SZ`} — {your-name}

{Context: what was reviewed, what was verified}
{findings organized by severity: Critical, High, Medium, Low}
{positive observations}
```

**Sign-off calibration:** Critical/High must be resolved before sign-off. Medium should be resolved; may be acknowledged with rationale. Low may be acknowledged without resolution. Zero-finding reviews still produce a full verification document.

When satisfied, add sign-off: `- [x] Reviewer satisfied — {timestamp}` / `- [ ] Author satisfied` / `- [ ] Human approved`

## Commit

Conventional commit: `review(forge): {artifact description}`. Commit the review document standalone — the transition opens the bracket, the commit closes it.

## After the review

Present findings to the human. Options: **Send to author** (doing agent addresses findings), **Approve** (human signs off), **Abandon** (critical issues). The human may act as reviewer directly — record a minimal sign-off and note in the transition.

## Expected resolution format

Expect a timestamped response with explicit dispositions for every finding: **"Will address"** (with change described) or **"Acknowledged, not addressing"** (with reason). No finding may be silently skipped. Flag omissions in the next round.

## Critical rules

1. Read context files FIRST
2. PHILOSOPHY.md and CLAUDE.md are the standards — review against them
3. Be exhaustive and specific — "`brine features --overlap` flag: NOT COVERED" not "CLI features covered"
4. Quote the source — reference specific spec text, plan tasks, or code locations
5. Severity matters — distinguish Critical (blocks progress) from Low (minor suggestion)
6. The review document is append-only — never edit prior rounds
7. Flag any raw unit tests — all tests must be `.feature` files
8. Do not implement fixes — find gaps, don't fill them (unless the human asks)
9. Flag work depending on unapproved exceptions — see AGENTS.md "Exceptions"
10. For phase review use `impl.phase.review.md` — reserve `impl.review.md` for final review

# Spec Review Criteria

Spec review verifies that acceptance criteria cover intent, requirements are concrete, and scope is well-bounded.

## Criteria

- Do acceptance criteria cover the proposal's intent for this track?
- Are requirements concrete and verifiable?
- Is scope appropriately bounded? Anything missing? Anything that shouldn't be here?
- Are there unstated dependencies or assumptions?
- Feature count: does the spec imply N capabilities but only list M?
