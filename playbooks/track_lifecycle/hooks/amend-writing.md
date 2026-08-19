# Amendment Writing Context

HOOK-MARKER: TRACK-LIFECYCLE-AMEND-WRITING

Amend a frozen phase artifact. Frozen artifacts are never edited directly;
amendments are append-only entries in `*.amendments.md`.

## First Read Context

Read `PHILOSOPHY.md`, `CLAUDE.md`, relevant projections, the target artifact, and
its review document. Amendments can happen outside a sequential phase, so there is
no prior phase commit to verify.

## Identify Target And Mode

Determine:

1. Artifact kind: proposal, milestone, track, initiative, or decision.
2. Mode:
   - State-machine mode for proposals, milestones, and decisions.
   - Document-only mode for tracks and initiatives.
3. Frozen artifact being amended.
4. What changed during execution and why it belongs in an amendment.

Ask the human if any of these are unclear.

## Amendment Routing

- Proposal `vision.md`: `vision.amendments.md`
- Proposal `proposal.md`: `proposal.amendments.md`
- Proposal `reflection.md`: `reflection.amendments.md`
- Milestone `milestone.md`: `milestone.amendments.md`
- Milestone `reflection.md`: `reflection.amendments.md`
- Track `spec.md`: `spec.amendments.md`
- Track `plan.md`: `plan.amendments.md`
- Track `reflection.md`: `reflection.amendments.md`
- Initiative `definition.md`: `amendments.md`
- Decision `definition.md`: `amendments.md`

## Amendment Entry

Read the existing amendments file if present, count existing `## Amendment N`
entries, and append the next number.

Use this structure:

```markdown
## Amendment N

**Date:** {timestamp}
**Author:** {your-name}
**Target:** {artifact being amended}

### Context

{What happened during execution that motivates this amendment}

### Amendment

{The specific change to intent, scope, or approach}

### Rationale

{Why this change is necessary at this level}
```

## State-Machine Amendments

For proposals, milestones, and decisions, the lifecycle enters `amend`, the
amendment is written, then review follows. Proposal and milestone amendments append
to the relevant `*.amendments.md`; decision amendments append to `amendments.md`.

## Document-Only Amendments

For tracks and initiatives, append the amendment without pausing the lifecycle.
The amendment is reviewed as part of the artifact's normal lifecycle.

## Revision Mode

For state-machine amendments only, when review findings return:

1. Read the amendments file and review document.
2. Append a timestamped response with explicit dispositions for every finding.
3. Revise by appending a revision note; do not delete the original entry.
4. Commit with the amendment review-fix convention.

## Critical Rules

1. Frozen artifacts are never edited.
2. Identify state-machine versus document-only mode early.
3. Only reviewed and human-approved amendments are authoritative.
4. Determine amendment number by reading the existing file.
5. If the learning is local to one track's execution, keep it in reflection rather
   than amending higher-level intent.
