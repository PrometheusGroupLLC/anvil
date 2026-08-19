# Plan Review Context

HOOK-MARKER: TRACK-LIFECYCLE-PLAN-REVIEW

Plan review evaluates coverage against the spec, feasibility of approach, task
ordering, and completeness.

## Criteria

- **Coverage**: For each requirement in `spec.md`, is there a matching task?
  Flag gaps.
- **Feasibility**: Does the approach match actual codebase patterns? Are file
  references correct?
- **Ordering**: Do phase dependencies make sense? Can anything be parallelized?
- **Completeness**: Are there implied tasks the plan does not list, including
  test infrastructure, manifest updates, and documentation?
- **Architecture**: Is the implementation approach specified clearly enough, or
  is it hand-wavy?

Write findings to `plan.review.md` using the standard review format. Critical and
High findings block sign-off. Medium findings should be resolved or explicitly
accepted by the human. Low findings may be acknowledged.
