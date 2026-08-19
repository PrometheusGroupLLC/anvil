# Amendment Review Context

HOOK-MARKER: TRACK-LIFECYCLE-AMEND-REVIEW

No dedicated amend-review criteria file exists in the source skill tree. This hook
is derived from the amendment review guidance in `amend.md`.

Amendment reviews compose mechanism checks with the base criteria for the amended
artifact type.

## Criteria

1. Is the amendment necessary?
2. Is it factually accurate?
3. Is it properly scoped, without smuggling unrelated changes?
4. Does it follow append-only rules and avoid editing the frozen source artifact?
5. Does it affect downstream work?
6. Does it require human approval before becoming authoritative?

For proposal amendments, check whether active tracks depend on changed assumptions.
For milestone amendments, check whether success criteria or contributing-work changes
invalidate in-progress tracks. For decision amendments, check whether validity
conditions have shifted enough to justify revising institutional knowledge.

Write the review to the phase's existing review document, not to a separate
`*.amendments.review.md` file.
