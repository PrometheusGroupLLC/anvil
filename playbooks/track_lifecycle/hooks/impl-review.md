# Final Implementation Review Context

HOOK-MARKER: TRACK-LIFECYCLE-IMPL-REVIEW

Final whole-track implementation review happens at `impl_review`. This is the gate
before reflection. Create or append to `impl.review.md`.

## Initiative Regression Check

Read active initiative definitions relevant to the work. Check whether the final
implementation advances or regresses those initiatives.

- Regression against a promoted initiative is High severity.
- Regression against an active initiative is Medium severity.

## Criteria

- **Intent compliance**: Does the code implement what `spec.md` and `plan.md`
  asked for?
- **Style compliance**: Does it follow `CLAUDE.md` rules and `PHILOSOPHY.md`
  principles?
- **Correctness**: Look for bugs, race conditions, edge cases, and security issues.
- **Testing**: Are there new `.feature` files and do they cover the changes? Flag
  any raw unit tests for behavioral testing.
- **Cross-phase integration**: Do the phases work together coherently?
- **Acceptance-criteria closure**: Are all spec acceptance criteria met?
- **Unresolved findings**: Are phase-review findings resolved or explicitly
  accepted?

## Mechanical Verification

For code tracks, run the verification commands from `CLAUDE.md` that apply to the
change. For artifact-only tracks, review substitutes for tests: verify structure,
content completeness, and acceptance-criteria alignment.

## Track-Phase Amendment Check

Look for `*.amendments.md` files in the track directory. If present, read each one
and verify the amendment was addressed consistently with the original spec intent.
