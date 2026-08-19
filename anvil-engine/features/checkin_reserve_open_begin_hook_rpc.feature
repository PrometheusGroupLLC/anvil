Feature: Checkin RPC re-serves the open-begin state's hook content (T4)
  When a resumed/compacted session checks back in under its prior actor name,
  the engine re-serves the hook content for any artifact that actor still has an
  OPEN begin on (in its current state), through CheckinResponse.context. This
  re-warms standing context that compaction may have stripped, reusing the same
  budget-capped serve path as `begin`. Re-serve is purely additive: an actor
  with no open begin gets an empty context.

  Background:
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260614T0700_reserve_impl/                 | implementing |
    And the track "20260614T0700_reserve_impl" has spec.md with content "# Reserve Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## implementing

      - [Reserve Track](tracks/20260614T0700_reserve_impl/) — reserve track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-14T00:00:00Z
      last_updated: 2026-06-14T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Implementing (1)

      - [Reserve Track](tracks/20260614T0700_reserve_impl/)
      """
    And a playbook hook body for the track hook "implementing.md" with content "RPC-REWARM: RESERVE-IMPL-HOOK"
    And the engine is started with that hearth

  # After an actor opens a begin on the implementing track, a checkin under the
  # same actor name re-serves that state's doer hook into context.
  Scenario: checkin re-serves the open-begin hook for the resuming actor
    When the begin RPC is called with identifier "20260614T0700_reserve_impl" and session_role "resumer" and actor_name "Resumer-700101"
    Then the begin RPC response state is "implementing"
    When the checkin RPC is called with role "resumer" and actor_name "Resumer-700101"
    Then the checkin RPC response actor_name is "Resumer-700101"
    And the checkin RPC response context contains "RPC-REWARM: RESERVE-IMPL-HOOK"

  # A different actor (no open begin on the track) gets an empty context.
  Scenario: checkin returns empty context for an actor with no open begin
    When the begin RPC is called with identifier "20260614T0700_reserve_impl" and session_role "resumer" and actor_name "Resumer-700101"
    Then the begin RPC response state is "implementing"
    When the checkin RPC is called with role "resumer" and actor_name "OtherActor-700199"
    Then the checkin RPC response actor_name is "OtherActor-700199"
    And the checkin RPC response context is empty
