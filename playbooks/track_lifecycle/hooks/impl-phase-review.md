# Implementation-Phase Review Context

HOOK-MARKER: TRACK-LIFECYCLE-IMPL-PHASE-REVIEW

This is a checkpoint review during `implementing` / `impl_phase_review`, not the
final gate. Create or append to `impl.phase.review.md`, not `impl.review.md`.

## Criteria

- Focus on the completed phase or phases, not the entire track.
- Check phase output against the corresponding plan tasks.
- Flag concerns that may affect later phases.
- Confirm that the track can safely return to `implementing` after findings are
  addressed.

## Initiative Regression Check

Read relevant active initiative definitions under `forge/initiatives/`. Check
whether the completed phase advances or regresses those initiatives.

- Regression against a promoted initiative is High severity.
- Regression against an active initiative is Medium severity.

## Rule 11 Compliance Checklist

Rule 11 is enforced per phase. Every completed phase should deliver a vertical
slice a real consumer can use end to end.

Check:

- Consumer-coverage checks pass where the repo has such tooling.
- Banned-pattern checks pass on touched files where the repo has such tooling.
- Every touched page has `.feature` coverage.
- User-seam scenarios assert real data roundtrip, not DOM shape alone.
- The phase is not UI-only or backend-only unless explicitly scoped and approved.
- Production data absence is represented with explicit empty, loading, or error
  states, not fabricated data.

Surface failures as ordinary review findings with calibrated severity.
