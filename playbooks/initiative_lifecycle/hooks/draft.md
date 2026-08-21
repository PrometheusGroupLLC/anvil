# Draft Mode — Create a New Initiative

Create a new initiative — a cross-cutting implementation expectation the codebase is converging toward. Initiatives are standing pressures with tracked evidence, not time-bounded work units.

## Steps

1. **Read existing initiatives.** Read `forge/initiatives.md` and scan `forge/initiatives/` directories to understand what already exists. Ensure the new initiative is distinct — not a duplicate or subset of an existing one.

2. **Discuss with the human.** Understand:
   - What pattern is the codebase converging toward?
   - Why does it matter?
   - What should implementers keep in mind?
   - What should reviewers check for?
   - Where did this initiative originate? (reflection, human observation, CLAUDE.md rule)

3. **Create the initiative directory.** Name uses descriptive kebab-case (e.g., `all-tests-are-feature-files`, `status-transition-authority`) — not date-prefixed.

   ```
   forge/initiatives/{name}/
   ├── status.yaml
   ├── definition.md
   ├── review.md        (empty, created for first review)
   ├── amendments.md    (empty)
   └── evidence.md      (empty or with bootstrap entries)
   ```

   **definition.md:**
   ```markdown
   # Initiative: {Title}

   ## Pattern

   {What implementation pattern the codebase is converging toward}

   ## Why

   {Why this matters}

   ## Implementer guidance

   {Concise bullets for agents doing implementation work}

   ## Reviewer guidance

   {What to check, what counts as a regression, severity level}

   ## Status

   {active | promoted | retired}

   ## Source

   {Which reflection, human observation, or CLAUDE.md rule originated this}

   ## Related CLAUDE.md rule

   {Present only if promoted — reference to the corresponding rule}
   ```

4. **Record transition.** Now that the directory exists, call the `snapshot` MCP tool:
   - `artifact_path`: `forge/initiatives/{name}`
   - `to_state`: `draft`
   - `actor_role`: `draft`

   Pass `actor_name` + runtime `actor_*` explicitly. Do NOT write to status.yaml, registry files, or projection files directly — the MCP tool handles all deterministic bookkeeping.

5. **Notify the human** that the initiative is ready for review. Provide a summary and file path. Human approval gates the commit.

6. **Commit:** `initiative(forge): draft {name} — {one-line description}`

## After draft

The initiative is ready for review. A reviewer calls `begin(identifier: <artifact_path>)` to receive the engine-served review guidance, then `complete(... satisfaction)` to record the verdict. Review validates:
- For initiatives sourced from CLAUDE.md: **definition accuracy** — does the initiative correctly capture the rule?
- For initiatives sourced from reflections/observation: **pattern validity** — is the pattern real, well-scoped, enforceable?

## Activation

Human approval of draft review constitutes activation — same convention as milestones. The reviewer or human records the `draft → active` transition via the `snapshot` MCP tool after review sign-off. There is no separate "activate" mode — review approval IS the activation gate. This is a review-gate convention handled by the engine's `complete` transition, not a separate skill.
