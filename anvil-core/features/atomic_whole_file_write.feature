Feature: Atomic temp+rename whole-file write (crash-safety mechanism)
  Whole-file replacements (status.yaml, registry, projections, actor block)
  MUST be written via temp-file + atomic rename — never an in-place
  std::fs::write on the live path — so a crash mid-write cannot leave a torn
  file (spec Req 6, AC "Crash-safety").

  This feature exercises the mechanism at the anvil-core library seam: the
  shared `atomic_write` helper, plus the real FileSystem adapters that route
  their whole-file replacements through it.

  Scenario: atomic_write routes through a temp sibling then renames it
    Given an empty atomic-write scratch directory
    When atomic_write writes "status.yaml" with content "state: spec_review\n"
    Then a temp sibling existed during the write
    And the file "status.yaml" has content "state: spec_review\n"
    And no ".tmp" files remain in the scratch directory

  Scenario: a snapshot status.yaml + registry transition leaves no torn .tmp files
    Given a snapshot fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
      | tracks/20260417T0100_sample_track/status.yaml                     | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Aprilis-379075:\n    type: agent\n    configurations:\n      - at: "2026-04-17T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-17T00:00:00Z\n    actor: Aprilis-379075\n    role: spec\n    approver: mark\n              |
      | tracks/20260417T0100_sample_track/spec.md                         | # Sample Track\n\nExample body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
      | tracks.md                                                         | # Tracks\n\n## spec\n\n- [Sample Track](tracks/20260417T0100_sample_track/) — sample track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                         |
      | projections/execution.md                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Plan (0)\n                                                                                                                                                                                                                                                                                                 |
    When snapshot fs is executed with:
      | artifact_path         | tracks/20260417T0100_sample_track |
      | to_state              | spec_review                       |
      | actor_name            | Reviewer-111111                   |
      | actor_role            | review                            |
      | actor_type            | agent                             |
      | actor_model           | claude-opus-4-7                   |
      | actor_provider        | anthropic                         |
      | actor_context_window  | 1000000                           |
      | actor_sdk_version     | 0.2.111                           |
      | actor_entrypoint      | claude-desktop                    |
      | at                    | 2026-04-17T01:00:00Z              |
    Then the snapshot result is successful
    And the resolved state of "tracks/20260417T0100_sample_track" is "spec_review"
    And no ".tmp" files remain under "tracks/20260417T0100_sample_track"
    And no ".tmp" files remain under "."
    And no ".tmp" files remain under "projections"

  Scenario: the actor-block status.yaml write leaves no torn .tmp file (N4)
    Given a snapshot fs hearth with:
      | path                                          | content                                                            |
      | tracks/20260417T0300_actor_track/status.yaml   | version: 1\nkind: track\nstate: spec\nactors:\ntransitions:\n  - to: spec\n    at: 2026-04-17T00:00:00Z\n    actor: Seed-000000\n    role: spec |
    When the actor block for "Seed-111111" is upserted into "tracks/20260417T0300_actor_track"
    Then the file "tracks/20260417T0300_actor_track/status.yaml" contains "Seed-111111:"
    And no ".tmp" files remain under "tracks/20260417T0300_actor_track"
