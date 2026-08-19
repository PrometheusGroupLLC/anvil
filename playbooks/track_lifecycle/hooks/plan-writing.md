# Plan Writing Context

HOOK-MARKER: TRACK-LIFECYCLE-PLAN-WRITING

Create the execution plan for a track: how the approved spec will be implemented.

## First Read Context

- `PHILOSOPHY.md`
- `CLAUDE.md`
- Relevant files under `forge/projections/`
- The track's `spec.md` and `spec.review.md`
- The source proposal's `proposal.md`

Verify that the spec phase has a closing commit by reading the last transition from
`status.yaml` and checking `git log`. Warn the human before proceeding if no matching
commit exists.

## Research The Codebase

The spec tells you what. Your job is to determine how.

Systematically research:

- Existing patterns: how similar features are implemented and which conventions apply
- Entry points: files, modules, interfaces, and seams where the change belongs
- Data flow: how related behavior moves through the system
- Test patterns: how similar behavior is covered with `.feature` files
- Dependencies: what must exist before this work starts and what this unblocks

Use fast code search and file reads aggressively. Reference specific files and line
numbers in the plan.

## Search Existing Decisions And Learnings

Before planning execution, search for relevant prior deliberation:

1. Read `forge/decisions.md` and `forge/learnings.md` for execution strategy
   decisions, operational patterns, technical choices, and prior observations,
   conclusions, and established learnings in the track's domain.
2. If `forge/projections/decisions.md` exists, scan it for summary data, and if
   `forge/projections/learnings.md` exists, scan it for state counts, recent
   observations, and nearing-establishment conclusions.
3. If matches are found, read the matching `forge/decisions/{name}/definition.md`
   or `forge/learnings/{name}/definition.md`.

Prior decisions inform task ordering, phase structure, and implementation approach;
established learnings are standing knowledge the plan should respect, conclusions are
provisional guidance, and observations are context that may ripen. If no relevant
decisions or learnings exist, proceed normally.

## Write `plan.md`

Add `plan.md` to the track directory. It should include:

- Phases based on actual code structure and dependency order
- Concrete tasks within each phase, referencing specific files and patterns found
  during research
- Pending tasks marked `[ ]`
- Phase checkpoints marked `[ ] checkpoint`

Tasks must be actionable: "implement X in Y using the pattern from Z", not
"add feature".

Notify the human when the artifact is ready for review with a short summary, the file
path, and a request to review it in their editor. Human approval gates the commit, not
the file write. If the human requests changes, revise the file in place.

## Revision Mode

When the reviewer returns findings in `plan.review.md`:

1. Read `plan.md` and `plan.review.md`.
2. Append a timestamped response to `plan.review.md` with an explicit disposition
   for every finding:
   - **Will address**: describe the change.
   - **Acknowledged, not addressing**: explain why.
3. Revise `plan.md` for all findings you will address.
4. Commit the revision with the plan review-fix convention.

No finding may be silently skipped.

## Critical Rules

1. Read context first, especially the approved spec and review.
2. Research the codebase before proposing an approach.
3. Every task must reference real files and patterns.
4. The plan implements the spec; do not add or remove scope.
5. Write the artifact, then get human approval before committing.
6. Do not write lifecycle bookkeeping files directly; the engine owns transitions.
7. Behavioral tests are `.feature` files.
