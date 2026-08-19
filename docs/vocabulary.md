# Anvil canonical vocabulary

**Status:** normative for all current, human-facing and agent-served Anvil surfaces.

This file is the checked-in referent taxonomy for the `workflow` → `playbook` migration
(track `20260725T2309_retire_the_workflow_term:_migrate_every_surface_to_playbook`). It is the
**referent** table: it answers *what a token denotes*.

It is **not** the residual-token category enum. `scripts/residual-workflow-tokens.py` takes its
five-value `category` enum (why a residual token may stay) from the track spec's completion
boundary, and joins it against the eight `referent` rows below. The two taxonomies are joined,
never merged.

## Canonical semantic vocabulary

| Referent | Current examples | Canonical target |
|---|---|---|
| Reusable lifecycle definition artifact | `workflow` kind/status, “workflow package,” `WorkflowMachine` | **playbook**, playbook definition, `PlaybookMachine` |
| One execution driven by a playbook | “workflow instance,” `workflow_instance_id` | **playbook run** — the same words for people and for machines: **`playbook_run_id`** (RATIFIED, see below) |
| Governed artifact kind selected by routing | `workflow_kind`, `available_workflow_kinds` | **artifact kind** or **driven kind**, chosen field-by-field |
| Engine vs fallback discriminator | `supported_workflow` | **execution route** / `execution_route` |
| Router hint | `workflow_hint` | **routing hint** / `routing_hint` |
| Playbook authoring kind | `workflow_generation` | **playbook generation** / `playbook_generation` — the only spelling anything writes; a PERSISTED `kind:` carrying the retired one still resolves (read-side, bidirectional, `playbook_generation_aliases`) |
| Foreign package/evaluation/dispatch schema | a downstream consumer's `workflow_id`, paths, or column | Owner-defined versioned migration; never blind Anvil substitution |
| Opaque historical identity | artifact IDs, old paths in transition records | Preserve byte-for-byte; label as historical where displayed |

### The runtime noun is ratified: **playbook run** / `playbook_run_id`

Nick, 2026-07-30, verbatim: *“let's call it 'playbook run' everywhere for both machines and
people.”* That settles the row above and **supersedes the provisional `run_instance_id`** this
document previously carried pending that ratification. One noun, one spelling, no register
split: prose says “playbook run,” identifiers and wire fields say `playbook_run_id`.

`run_instance_id` is now a placeholder that outlived its purpose, and no surface may keep it: a
name that was only ever provisional, left standing in a durable schema, is the second version of
the same thing. Where an interim build already wrote it, the owner's migration takes the extra
hop rather than stranding it (see a downstream store's two-hop `task_runs` rename:
`workflow_instance_id` → `playbook_run_id` and `run_instance_id` → `playbook_run_id`, both
idempotent, only one applicable to any given database). It survives here only as the
`run_instance` referent KEY below, which names the *referent* (“one execution driven by a
playbook”), never a field.

`playbook_instance_id` **remains forbidden** as a replacement for `workflow_instance_id`:
“instance” blurs the reusable definition with one execution of it. `playbook_run_id` does not
carry that defect, because *run* names the execution and nothing else — which is exactly why
ratifying it costs nothing the forbidden name would have cost.

With the noun ratified, the **live wire moves now, whole**: the proto messages and fields, the
engine's gRPC/JSON responses, the MCP tool surface and the kit dashboard that reads them all say
`playbook_run_id`, and the retired spellings are *refused*, not aliased. Readers and writers move
in the same commit. **Nothing anywhere emits a retired spelling**, and no client-supplied name is
aliased on any surface a caller can reach.

That claim is about what is WRITTEN and what a CALLER may send. It is not a claim that no reader
ever recognises a retired spelling, and it must not be read as one — two classes of already-written
bytes are recognised on the read side, each enumerated below with the gate that keeps it honest:

1. the **six durable JSONL sinks** (next paragraph), and
2. **persisted definitions** — a hearth's `machine.yaml`. Anvil reads those files; it does not own
   them. A machine may declare `kind: workflow_generation`
   (`registry::playbook_generation_aliases`, bidirectional, asserted by
   `semantic_identifier_families.feature`) or a required field named `workflow_name`
   (`begin::BUILTIN_REQUIRED_FIELDS`, `anvil_orchestrate_legacy_name_field.feature`), and the live
   builder machine in anvil-hearth declares both. Each resolves the ONE canonical value; neither
   adds a second field, a second code path, or a second thing a caller may send. Dropping either
   makes a live definition unsatisfiable — which is exactly what happened, and what
   `anvil_orchestrate_legacy_name_field.feature` now measures.

The **six durable JSONL sinks** take the extra hop, which is the same principle applied to bytes
already written. Their writers emit only `artifact_kind` / `playbook_run_id` (and the sink file is
`playbook-measurement.jsonl`); their READERS additionally recognise the pre-ratification key,
value and file name, because ~316,000 rows carry them, the sinks have no schema-version field, and
the schema-migration contract forbids rewriting or renaming a sink
in place. That is a migration read, not an alias: nothing writes the retired spelling, and a
canonical-only reader would resolve `""` for every historical row — a silent loss the
append-immutability gate cannot see, which is why
the canonical-keys contract asserts BOTH halves against the real
adapters over genuinely sampled rows. Every legacy-read site is enumerated in
`residual-tokens.allowlist.yaml` under category `legacy_adapter`, owned by C-d.2 — the row
migration that deletes them.

## Three kind fields that MUST remain distinct

For a directory such as `playbooks/track_lifecycle/`:

- manifest `anvil_kind: track` identifies the governed artifact kind and drives registrar dedup;
- `machine.yaml.kind: track` identifies the governed state machine and keys the engine registry;
- `status.yaml.kind: playbook` identifies the definition directory as a playbook artifact governed
  by the playbook lifecycle.

This migration changes the third. It MUST NOT rewrite the first two to `playbook`.

## Referent keys

Every entry in `residual-tokens.allowlist.yaml` carries a `referent` field whose value is one of
the eight keys below. A `referent` naming a key absent from this file is a hard failure of
`scripts/residual-workflow-tokens.py`. An ordinary-process noun (English "development workflow",
unrelated to Anvil) carries the literal `none`.

| `referent` key | Table row |
|---|---|
| `definition_artifact` | Reusable lifecycle definition artifact |
| `run_instance` | One execution driven by a playbook — human noun **playbook run**, wire name `playbook_run_id` |
| `governed_artifact_kind` | Governed artifact kind selected by routing |
| `execution_route` | Engine vs fallback discriminator |
| `routing_hint` | Router hint |
| `playbook_generation` | Playbook authoring kind |
| `foreign_owned` | Foreign package/evaluation/dispatch schema |
| `opaque_identity` | Opaque historical identity |
