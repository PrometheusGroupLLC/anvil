# Completion Context

HOOK-MARKER: TRACK-LIFECYCLE-COMPLETE

Mark a track, proposal, or milestone complete. This is administrative closure; the
semantic work should already be done.

## First Read Context

- Relevant registries, such as `forge/tracks.md` and `forge/proposals.md`
- The target's `status.yaml`
- The target's `reflection.md` and `reflection.review.md`

Verify that the reflection review phase has a closing commit. If no matching commit
exists, warn the human before proceeding.

## Verify Completion

For a track:

1. All tasks in `plan.md` are marked `[x]`.
2. `reflection.md` exists.
3. `reflection.review.md` has sign-off.
4. `impl.review.md` has sign-off; phase reviews do not substitute for final review.
5. No work depends on unapproved exceptions.
6. Reflection addresses the knowledge delta lens when active initiatives exist.

For a proposal:

1. All spawned tracks are completed or explicitly out of scope.
2. `reflection.md` exists.
3. `reflection.review.md` has sign-off.

For a milestone:

1. `milestone.review.md` has sign-off.
2. `reflection.md` exists.
3. `reflection.review.md` has sign-off.
4. No unapproved exceptions remain.

If anything is missing or incomplete, warn the human and ask how to proceed.

## Record Terminal Transition

The completion action records the terminal transition to `completed` with role
`complete`. Pass explicit actor identity fields through the engine-owned transition
mechanism. Do not write lifecycle bookkeeping files directly.

## Rebuild Projections

After the terminal transition lands, regenerate projections from authoritative
sources:

- Status files under tracks, proposals, milestones, decisions, and learnings
- Registry files
- Last human-verified projection bases

`truth.md` is **rebuild-only**: it is regenerated from the structural state above
(artifact kinds, lifecycle shape, the projection/engine topology), NOT accumulated
from reflection deltas. Reflection deltas are never appended into `truth.md`.

When folding reflection deltas, route each delta to exactly one tagged destination:

- `decision` — what was CHOSEN between alternatives, with rationale
- `tension` — what we ASK (unresolved question)
- `learning` — what we NOTICED about system or agent behavior
- `initiative-evidence` — a pattern instance against an active initiative

The human triages tagged deltas into the appropriate artifacts; this step only
fixes the destination, it does not create the downstream artifact.

The human reviews regenerated projections and signs off by resetting projection
frontmatter as described by the project's documented process.

## Critical Rules

1. Verify before completing.
2. Do not complete with unsigned reviews or missing reflections.
3. Get human approval before marking complete.
4. Do not write lifecycle bookkeeping files directly; the engine owns the
   deterministic transition.
