# Implementation Context

HOOK-MARKER: TRACK-LIFECYCLE-IMPLEMENTING

Execute tasks from the current track's plan through test-driven development.

## First Read Context

- `PHILOSOPHY.md`
- `CLAUDE.md`
- Relevant files under `forge/projections/`, including `truth.md`
- The track's `status.yaml`, `spec.md`, and `plan.md`
- The track's `plan.review.md`

Active initiatives in `forge/projections/truth.md` are standing implementation
pressures. Treat them as guidance alongside the spec and plan.

Verify the track is in `implementing` state. If it is in `plan_review` or earlier,
stop because the plan has not been approved. If it is already in `implementing`,
check `status.yaml` and `plan.md` for where work left off.

Verify that the plan phase has a closing commit. If no matching commit exists, warn
the human before proceeding.

If `status.yaml` has `blocked_by`, inspect each referenced artifact's `status.yaml`.
Warn the human if any blocker has not reached a terminal state.

## Search Existing Decisions And Learnings

Before writing code, search for relevant prior deliberation:

1. Read `forge/decisions.md` and `forge/learnings.md`.
2. If `forge/projections/decisions.md` exists, scan it for summary data, and if
   `forge/projections/learnings.md` exists, scan it for state counts, recent
   observations, and nearing-establishment conclusions.
3. If matches are found, read the matching `forge/decisions/{name}/definition.md`
   or `forge/learnings/{name}/definition.md`.

Avoid patterns that prior work rejected and note validity conditions that this
implementation may shift. Treat established learnings as standing knowledge,
conclusions as provisional guidance, and observations as context that may ripen.

## Capture Observations

When you notice something about system or agent behavior mid-implementation,
capture it without derailing flow:

- Tangential noticing (a surprising convention, a confusing error) — fire a
  `forge:spark` with the `observation` disposition in mind; triage happens later.
- The observation is the point of the current task — capture it directly with
  `forge:learn capture`.

Keep the primary task moving; capture is low-ceremony, not a context switch.

## Task Loop

For each task:

1. Find an in-progress task `[~]` in `plan.md`; if none exists, take the first
   pending task `[ ]`.
2. Mark it in progress: `[ ]` to `[~]`.
3. Red: write failing `.feature` coverage from the user's perspective and run it.
   Confirm it fails for the right reason.
4. Green: implement the smallest change that makes the feature pass.
5. Refactor while keeping tests green.
6. Commit code changes and the `plan.md` status update together. Mark the task
   `[x] <commit-sha>` as part of that commit.

## Phase Completion

When every task in a phase is complete:

1. Run the full relevant test suite.
2. Present manual verification steps to the human.
3. Wait for explicit human approval.
4. Create the checkpoint commit and update `plan.md` with `[checkpoint: <sha>]`.

## Implementation-Phase Review

At phase boundaries, the human may request an incremental review. The review writes
to `impl.phase.review.md`; it does not substitute for the final whole-track review.

When phase-review findings return, respond with a timestamped entry and explicit
dispositions for every finding, then make the required changes before resuming.

## Final Implementation Review

After all tasks complete, the track is ready for final implementation review. The
final review covers the entire implementation against the spec and plan and writes
to `impl.review.md`.

## Critical Rules

1. Read context first, especially the approved plan and review.
2. Never skip Red: failing feature first.
3. Wait for human approval at phase checkpoints.
4. All behavioral tests are `.feature` files run through Brine.
5. Tests are written from the user's perspective at each seam.
6. Keep `plan.md` updated as work progresses.
7. Do not write lifecycle bookkeeping files directly; the engine owns transitions.
