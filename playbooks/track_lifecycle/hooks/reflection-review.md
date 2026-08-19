# Reflection Review Context

HOOK-MARKER: TRACK-LIFECYCLE-REFLECTION-REVIEW

Reflection review evaluates the semantic reflection for a track, proposal, or
milestone. Reflections feed projections, so inaccurate reflections can mislead
future work.

## Four Delta Lenses

Each lens must be substantively addressed, not merely named.

### Intent Delta

Check what the work set out to do versus what was understood by the end.
Cross-reference requirements against actual implementation and flag any requirement
that was implemented correctly but turned out to be the wrong thing.

### Reality Delta

Check what the plan predicted versus what actually happened. Compare plan tasks,
git history, review findings, and final artifacts. Identify work that took longer,
was reordered, or required approaches not in the plan.

### Exception Delta

Verify each claimed intentional divergence from spec or plan. Confirm it was
discussed in review or explicitly approved. Flag undocumented drift.

### Projection Delta

Verify future-facing claims against current codebase state. Projection claims must
be accurate, actionable, and not already stale.

## Cross-Cutting Checks

- Are claimed divergences truly intentional?
- Are projection claims accurate?
- Is anything missing that implementation review already observed?
- Does the reflection acknowledge failures and friction honestly?
