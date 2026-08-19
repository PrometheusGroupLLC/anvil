Feature: Snapshot filesystem actor-name handling
  Per spec R3 of the checkin_backfill_spec_context track, the snapshot
  handler requires a non-empty `actor_name`. Empty values return
  `ActorNameRequired` and do not mutate filesystem state. Supplied names
  are echoed to the transition and the actor record.

  Scenario: Supplied actor_name is echoed to status.yaml and response
    Given a snapshot fs hearth with:
      | path                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                         |
      | tracks/20260417T0300_echo_track/status.yaml               | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Existing-999999:\n    type: agent\n    configurations:\n      - at: "2026-04-17T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions: []\n                          |
      | tracks/20260417T0300_echo_track/spec.md                   | # Echo Track                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks.md                                                 | # Tracks\n\n## spec\n\n- [Echo Track](tracks/20260417T0300_echo_track/) — echo track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n                                                                                                                                                                                                                               |
      | projections/execution.md                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n                                                                                                                                                                                                                 |
    When snapshot fs is executed with:
      | artifact_path         | tracks/20260417T0300_echo_track |
      | to_state              | spec_review                     |
      | actor_name            | Gliridae-785891                 |
      | actor_role            | review                          |
      | actor_type            | agent                           |
      | actor_model           | claude-opus-4-7                 |
      | actor_provider        | anthropic                       |
      | actor_context_window  | 1000000                         |
      | actor_sdk_version     | 0.2.111                         |
      | actor_entrypoint      | claude-desktop                  |
      | at                    | 2026-04-17T03:00:00Z            |
    Then the snapshot result is successful
    And the snapshot result actor_name is "Gliridae-785891"
    And a transition event for "tracks/20260417T0300_echo_track" contains "actor: Gliridae-785891"
    And the file "tracks/20260417T0300_echo_track/status.yaml" contains "Gliridae-785891:"

  Scenario: Empty actor_name returns ActorNameRequired and writes nothing
    Given a snapshot fs hearth with:
      | path                                                      | content                                                                                                                                                                                                                                                                                                                                                                                                         |
      | tracks/20260417T0200_gen_track/status.yaml                | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Existing-999999:\n    type: agent\n    configurations:\n      - at: "2026-04-17T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions: []\n                          |
      | tracks/20260417T0200_gen_track/spec.md                    | # Gen Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks.md                                                 | # Tracks\n\n## spec\n\n- [Gen Track](tracks/20260417T0200_gen_track/) — gen track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n                                                                                                                                                                                                                                  |
      | projections/execution.md                                  | ---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n                                                                                                                                                                                                                 |
    When snapshot fs is executed with:
      | artifact_path         | tracks/20260417T0200_gen_track |
      | to_state              | spec_review                    |
      | actor_name            |                                |
      | actor_role            | review                         |
      | actor_type            | agent                          |
      | actor_model           | claude-opus-4-7                |
      | actor_provider        | anthropic                      |
      | actor_context_window  | 1000000                        |
      | actor_sdk_version     | 0.2.111                        |
      | actor_entrypoint      | claude-desktop                 |
      | at                    | 2026-04-17T02:00:00Z           |
    Then the snapshot result is an ActorNameRequired error
    And the file "tracks/20260417T0200_gen_track/status.yaml" still starts with "version: 1"
    And the file "tracks/20260417T0200_gen_track/status.yaml" does not contain "spec_review"
