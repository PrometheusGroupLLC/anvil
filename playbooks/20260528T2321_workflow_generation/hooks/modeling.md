# Modeling: emit a measurable anchored playbook

You are turning the analyzed process into a playbook machine. The output is not
complete unless it is measurable and anchored.

## Required output

Write the candidate playbook/machine with:

- States, roles, review gates, revision paths, terminal state, route metadata,
  and projection targets.
- Per-step `measurement_by_role` entries with concrete intent and expected output.
- Per-step `success_criteria` where a step can be judged immediately.
- A whole-playbook `success_rubric` selecting from the shared quality dimensions,
  with weights, grader hint, lagging signals, and anchors.
- `anchors` that reference emitted exemplar ids and use only the allowed bands:
  `good`, `bad`, `trap`, `hidden_virtue`.
- Exemplar markdown files under `exemplars/<id>.md`, each with valid frontmatter
  and a distilled redacted body.
- `ledger_classification` and, when `none_yet`, the full
  `none_yet_justification`.

## Candidate submission contract (the engine expands your input)

When you submit the candidate to the engine, it GENERATES the full machine from a
compact input. Get these right or `generate` rejects the candidate:

- **`proposed_states` are DOER working states ONLY.** List just the doer steps
  (e.g. `spec`, `plan`, `implementing`, `reflecting`), all with `role: "doer"`.
  The engine AUTO-ADDS each state's `_review` gate (reserved `reviewer` role),
  its `_revision` doer state, the `outcome_reflection_review` gate, and the
  terminal `completed`. Do NOT list `*_review`, `*_revision`, reviewer-role, or
  `completed` states yourself — a `reviewer` role in `proposed_states` is rejected
  as `ReservedRole`, and duplicating the auto-added states corrupts the machine.
- **`intent` becomes the playbook `kind`** (slugified). Keep it a SHORT kind name
  (e.g. `intent: "track lifecycle"` → kind `track_lifecycle`), NOT a long sentence
  — a long intent yields an over-long kind and a filename-too-long persist error.
  Put the descriptive purpose in the states' `intent`/`expected_output`, not the
  top-level `intent`.
- **`projection_targets` must be NON-EMPTY.** Give at least one projection file
  (e.g. `["workflows.md"]` for a lifecycle, `["routing.md"]` for a router) — an
  empty list is rejected as `EmptyProjectionTargets`.
  (`workflows.md` is the LEGACY registry filename of an existing hearth file and is
  reproduced here byte-exact; it is a persisted key, not vocabulary. New playbooks
  should name their own registry file after the artifact kind they govern.)
- **`route_triggers` and `route_description` must be NON-EMPTY.** Give at least one
  natural-language trigger phrase (e.g. `["make a decision"]`) and a one-line
  route description — empty `route_triggers` is rejected as `EmptyRouteTriggers`.
- Every scored `success_rubric` dimension must be COVERED by at least one exemplar
  whose `dimensions` includes it, or the anchor-coverage validator fails.

## Anchoring rules

Use the analyzing output's 2x2 mining results. Prefer trap exemplars when they
exist; then add good, bad, and hidden_virtue anchors only where they clarify the
rubric. Each scored rubric dimension should join to at least one emitted
exemplar through that exemplar's `dimensions` list.

Exemplar frontmatter must carry:

- `id`, unique within this playbook kind.
- `band`: one of `good`, `bad`, `trap`, `hidden_virtue`.
- `dimensions`: shared quality dimensions this exemplar anchors.
- `evidence_class`.
- `outcome_link` when evidence is `artifact_of_consequence`.
- `provenance` with source and corpus.
- `playbook_version` and `refreshed_at`.

Do not include raw sensitive artifacts in frontmatter or body. The body is the
distilled pattern only. If no exemplars exist yet, do not invent them; emit the
none-yet justification and keep production-routing risk explicit.
