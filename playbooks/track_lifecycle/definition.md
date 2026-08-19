# Track Lifecycle Workflow

**Artifact id:** `20260422T0000_track_lifecycle`
**Kind:** `workflow`
**State:** `active`

## Purpose

This is the track-kind lifecycle, bootstrapped from `seeds::track_seed()` in `anvil-core/src/domain/workflow/seeds/track.rs`. It defines the full state machine governing all track artifacts: 16 states (13 non-terminal + 3 terminal) and 18 transitions mirroring the `describe::available_actions("track", _)` call.

## Provenance

- **Track #11** (workflow machine types) established the `WorkflowMachine` type and seed discipline.
- **Track #14** (`hearth_loaded_workflow_registry`, `20260422T0206_hearth_loaded_workflow_registry`) produces this first on-disk workflow artifact as Phase 1. The `machine.yaml` mirrors `seeds::track_seed()` byte-identically; Phase 3's seed-YAML equivalence check enforces that no drift occurs.

## Approved exception

This artifact was bootstrapped directly to `active` state without the usual review-gate ceremony, per spec R1.2 and `truth.md` Exception governance. The rationale: machine.yaml is mechanically transcribed from a compiled-in seed, so review would add no editorial value. Approver: Mark Ducommun.
