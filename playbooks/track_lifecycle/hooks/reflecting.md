# Reflection Writing Context

HOOK-MARKER: TRACK-LIFECYCLE-REFLECTING

Write the semantic reflection: the delta between what was planned and what actually
happened. This is not a changelog or implementation diary. It records what was
learned, how understanding changed, what reality forced, what remains unresolved,
and what future agents should now treat as true.

## First Read Context

- `PHILOSOPHY.md`
- `CLAUDE.md`
- Relevant files under `forge/projections/`
- The artifact's implementation or delivery review
- The artifact's spec, plan, and review history where applicable

Verify that the implementation review phase has a closing commit. If no matching
commit exists, warn the human before proceeding.

## Inventory Per-Phase Reflection Notes

List `*_reflection/` subdirectories under the artifact directory. Each directory
corresponds to a phase whose actors supplied reflection notes through completion.
If none exist, proceed without error.

Use the inventory as input only; the criteria below still govern the reflection.

## Dispatch To Criteria

Determine the artifact kind from `status.yaml`, then use the corresponding
reflection criteria:

- Track: track reflection criteria
- Proposal: proposal reflection criteria
- Milestone: milestone reflection criteria

Read exactly one criteria set for the artifact kind.

## Write `reflection.md`

Create `reflection.md` in the target directory. Address every required lens, even
when the answer is "nothing changed." Keep it concise; a small track can have a
short reflection.

## Revision Mode

When review findings return in `reflection.review.md`:

1. Read `reflection.md` and `reflection.review.md`.
2. Append a timestamped response with explicit dispositions for every finding.
3. Revise `reflection.md` for findings you will address.
4. Commit the revision with the reflection review-fix convention.

No finding may be silently skipped.

## Critical Rules

1. Read all context required by the criteria.
2. Capture changed understanding, not just files edited.
3. Address every lens.
4. Projection and knowledge deltas matter most.
5. Be honest about exceptions and mismatches.
6. Do not conflate reflection with review.
