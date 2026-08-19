# Spec Writing Context

HOOK-MARKER: TRACK-LIFECYCLE-SPEC-WRITING

Instructions for writing a track specification. Delivered by the engine as part of the checkin/begin response.

## Actor identity discipline

**Carry `actor_name` forward.** On your first `checkin` of this conversation, the engine returns a canonical `actor_name` (or echoes the one you supplied). Hold that name in conversation memory and pass it explicitly on every subsequent `begin` or `snapshot` MCP call — those tools require a non-empty `actor_name` on every call and reject missing values with `actor_name_required`.

**Re-detect runtime `actor_*` attributes per call.** The runtime fields (`actor_type`, `actor_model`, `actor_provider`, plus optional `actor_context_window` / `actor_sdk_version` / `actor_entrypoint`) come from your current environment — not from any prior-call state. The `forge:id` skill's env-detection pattern is the normative reference: `DEFAULT_LLM_MODEL`, `CLAUDE_AGENT_SDK_VERSION`, `CLAUDE_CODE_ENTRYPOINT`, and the provider-appropriate context-window value derived from your model.

Do not rely on any prior-call state to populate identity. Every `begin`/`snapshot` call carries its own explicit `actor_*` set; the engine does not infer them from a prior `checkin`.

## Pre-read list

Before writing the spec, read these in order:

1. **`PHILOSOPHY.md`** at the workspace root — the project's design doctrine.
2. **`CLAUDE.md`** at the workspace root — build commands, rules, and repo structure.
3. **Relevant files under `forge/projections/`** — at minimum `truth.md` for active initiatives; read `intent.md`, `execution.md`, `forward.md`, and `decisions.md` where they bear on the track's domain.
4. **The source proposal** — `forge/proposals/{parent-id}/proposal.md`, and `proposal.review.md` when present. The proposal is the strategic direction the track narrows; the spec is subordinate to it.

## Decisions-search step

Before committing to an approach, search for prior deliberation:

1. Scan `forge/decisions.md` (the registry) for decisions in the track's domain — both `decided` and `tension`/`investigating`.
2. If `forge/projections/decisions.md` exists, scan it for summary counts and recently resolved items.
3. For any match, read the corresponding `forge/decisions/{name}/definition.md` for rationale, alternatives evaluated, and validity conditions.

The goal is to avoid re-proposing patterns that prior tracks already tried and rejected, and to discover validity conditions your spec may shift. This is guidance, not a gate — proceed normally if no relevant decisions exist.

## Understanding the track scope

The proposal defines the strategic direction. The spec narrows it to a concrete, implementable unit.

Ask the human:
- **Which slice of the proposal is this track?** A proposal may spawn multiple tracks. What's this one?
- **What are the acceptance criteria?** How do we know this track is done?
- **What's in scope and out of scope?** Be explicit about boundaries.
- **What should the user experience look like when this is done?**

If the human gives a vague answer, ask follow-ups. The spec is only useful if it's specific enough to be verified.

## Writing the spec

Add `spec.md` to the track directory.

**spec.md structure:**
- Overview (the WHY — what problem, who benefits, what happens if we don't)
- Requirements (the WHAT — numbered list of concrete, verifiable requirements)
- Acceptance criteria (how we know it's done — checkable items)
- Out of scope (explicitly excluded)

The spec does NOT include technical approach or implementation details. That's the plan's job.

Write `spec.md` to the track directory. Notify the human that the artifact is ready for review:
- Provide a 1-3 sentence summary of what the document covers
- State the file path for editor review
- Prompt: "Review in your editor and let me know when you're ready to proceed."

Human approval gates the commit, not the file write. If the human requests changes, revise the file in place.

## After the spec is accepted

When the spec is ready, call `complete(artifact_path, actor_*)` (no `satisfaction`) to advance the track to `spec_review`. The engine drives the lifecycle from here — do NOT invoke a skill.

**Review:**
- **Agent review** — a reviewer (you in a reviewer role, or another actor) calls `begin(identifier: <artifact_path>)` to receive the review guidance the engine serves, writes `spec.review.md`, then calls `complete(artifact_path, satisfaction: "satisfied", actor_*)` to advance to plan.
- **Human-as-reviewer** — the human reviews directly and records a minimal sign-off entry in `spec.review.md` before the reviewer's `complete`.

**Next phase:**
- **Plan** — default for non-trivial tracks. On the reviewer's `complete(... satisfaction: "satisfied")` the engine advances to the plan phase; the planner calls `begin(identifier: <artifact_path>)` to receive plan-writing guidance, writes `plan.md`, then `complete`.
- **Skip plan, go straight to implement** — appropriate for small, well-understood tracks where the spec provides sufficient implementation guidance. The engine-native path is to advance through to `implementing` via the normal `complete` calls; the implementer then calls `begin(identifier: <artifact_path>)` for implementation guidance.

## Commit convention

Per AGENTS.md "Commit convention," commit the spec once after human approval. The commit message format:

```
spec(forge): {track description}
```

The commit bundles `spec.md` plus any snapshot bookkeeping (status.yaml, registry, projection files) into one commit. This is the closing bracket for the spec phase — the transition to `spec` opened it; this commit closes it.

For revision-mode commits use the same prefix with an "address review findings" suffix when appropriate:

```
spec(forge): {track description} — address review findings
```
