# Spec Revision Context

HOOK-MARKER: TRACK-LIFECYCLE-SPEC-REVISION

Instructions for revising a track specification in response to review findings. Delivered by the engine when a doer re-enters a `spec_revision` track via `begin(identifier)`. This is the revision counterpart to `spec-writing.md` (spec creation) and `spec-review.md` (reviewer judgment).

## Actor identity discipline

**Carry `actor_name` forward.** On your first `checkin` of this conversation, the engine returns a canonical `actor_name` (or echoes the one you supplied). Hold that name in conversation memory and pass it explicitly on every subsequent `begin` or `complete` MCP call — those tools require a non-empty `actor_name` on every call and reject missing values with `actor_name_required`.

**Re-detect runtime `actor_*` attributes per call.** The runtime fields (`actor_type`, `actor_model`, `actor_provider`, plus optional `actor_context_window` / `actor_sdk_version` / `actor_entrypoint`) come from your current environment — not from any prior-call state. Detect them from the environment: `DEFAULT_LLM_MODEL`, `CLAUDE_AGENT_SDK_VERSION`, `CLAUDE_CODE_ENTRYPOINT`, and the provider-appropriate context-window value derived from your model.

Do not rely on any prior-call state to populate identity. Every `begin`/`complete` call carries its own explicit `actor_*` set; the engine does not infer them from a prior `checkin`.

## Revision-mode procedure

The reviewer required a full revision and the track is now in `spec_revision`. Your job is to address the findings the reviewer recorded.

1. **Read `spec.md` and `spec.review.md`** from the track's artifact directory using standard file tools. The `review_doc_path` in the `begin` response points at `spec.review.md` so you can locate the findings without scanning the directory. The `begin` response does not inline the artifact text — read the files directly.
2. **Respond to every finding with an explicit disposition.** Append a timestamped response to `spec.review.md` (append-only). No finding may be silently skipped regardless of severity.
3. **Revise `spec.md` in place** to reflect the findings you will address.
4. **Call `complete`** to signal the revision pass is done (see "After revision" below).

## Finding-disposition vocabulary

For every finding the reviewer raised, record exactly one disposition:

- **"Will address"** — describe the change you will make.
- **"Acknowledged, not addressing"** — explain why (design choice, out of scope, reviewer misread, etc.).

No finding may be silently skipped.

## Commit convention

Per AGENTS.md "Commit convention," commit the revision once after the revision pass is complete. Use the same `spec(forge)` prefix as spec creation, with an "address review findings" suffix:

```
spec(forge): {track description} — address review findings
```

The commit bundles the revised `spec.md`, the appended `spec.review.md` dispositions, and any `complete` bookkeeping (status.yaml, registry, projection files) into one commit. This is the closing bracket for the revision pass — the transition into `spec_revision` opened it; this commit closes it.

## After revision

Call `complete(artifact_path)` with **no `satisfaction`** to transition the track back to `spec_review` for re-review. This is the engine-driven path: the doer's `complete` call records the `spec_revision → spec_review` transition. Do NOT invoke a skill — the engine's `complete` RPC handles this transition.

The `spec_revision → spec_review → spec_revision` cycle may repeat as many rounds as the reviewer requires; each `complete` records a distinct transition.
