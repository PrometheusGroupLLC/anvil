# Reflect Mode — Initiative Convergence Synthesis

Assess an initiative's convergence trajectory, definition effectiveness, and lifecycle readiness. Initiative reflection is **fundamentally different** from track/proposal/milestone reflection:

- It is **repeatable** — each invocation appends a timestamped synthesis entry to `reflection.md` (append-only)
- It uses convergence lenses, not delta lenses
- It reads evidence and definition, not spec/plan/implementation

Every initiative reflection must be followed by review: call `complete(artifact_path, actor_*)` to advance, then a reviewer calls `begin(identifier: <artifact_path>)` to receive the engine-served review guidance.

The `snapshot` MCP tool transition to `reflecting` was already fired by the protocol.

## Context to read

- `definition.md` in the initiative directory
- `evidence.md` in the initiative directory
- Previous `reflection.md` entries (if any exist — this is append-only)

## Append a timestamped synthesis entry

Obtain timestamp via shell command:
```bash
date -u +%Y-%m-%dT%H:%M:%SZ
```
**Never fabricate a timestamp.** Always use the shell command.

Each invocation appends a new entry to `reflection.md` with the timestamp. Do not modify previous entries.

## Write through four lenses

### Convergence assessment

Is the codebase actually moving toward this pattern? What does the evidence trajectory show?

- Are recent evidence entries showing advancement or regression?
- Is the rate of advancement increasing, steady, or declining?
- Are there areas where convergence stalled?

### Definition effectiveness

Is the implementer/reviewer guidance working?

- Are regressions being caught during review?
- Are the same regressions recurring despite the guidance?
- Is the definition clear enough that agents follow it without confusion?
- Are there patterns of misinterpretation?

### Exception assessment

Are approved exceptions accumulating in a way that suggests the initiative needs refinement?

- How many exceptions exist relative to conforming evidence?
- Are exceptions concentrated in a specific area (suggesting the definition is too rigid there)?
- Do any exceptions suggest the initiative's scope should be narrowed or broadened?

### Lifecycle readiness

Does the evidence support a lifecycle transition?

- **Promotion to CLAUDE.md rule:** Is there enough conforming evidence? Is the pattern stable across multiple tracks? Are regressions rare?
- **Retirement:** Has the initiative lost relevance? Is the pattern no longer useful?
- **Definition amendment:** Does the evidence suggest the definition needs refinement rather than the codebase needing to change?

## Commit

`initiative(forge): reflect {name} — {synthesis summary}`

## After completion

The reflection is ready for review. The reviewer's engine-served review guidance (delivered on `begin(identifier: <artifact_path>)`) evaluates the synthesis against the evidence — this is especially important because initiative reflection informs lifecycle decisions (promotion, demotion, retirement).

---

## Revision mode (responding to reflection review)

When the reviewer returns findings in `reflection.review.md`:

1. Read `reflection.md` and `reflection.review.md`
2. Append a response timestamped via `date -u +%Y-%m-%dT%H:%M:%SZ` to `reflection.review.md` with explicit dispositions for every finding:
   - **"Will address"** — describe the change
   - **"Acknowledged, not addressing"** — explain why
   No finding may be silently skipped, regardless of severity.
3. Revise the latest `reflection.md` entry as needed (do not modify previous entries)
4. Commit: `initiative(forge): reflect {name} — address review findings`
