Feature: Checkin re-serves open-begin hook content end-to-end (T4, harness-agnostic)
  A compacted/resumed session that lost its context window can re-warm it by
  checking back in under its prior actor name: the MCP shim's `checkin` tool
  surfaces, in its `context` field, the hook content for any artifact that actor
  still has an OPEN begin on. This e2e exercises the full marker → checkin
  re-serve path through the shim/JSON-RPC seam, proving compaction-survival is
  the felt experience for the harness (Claude Code) — not just an engine detail.

  Background:
    Given a hearth directory with the following structure:
      | path                                                   | state        |
      | proposals/20260411T2021_anvil_workflow_engine/          | active       |
      | tracks/20260604T1200_reserve_e2e/                       | implementing |
    And the track "20260604T1200_reserve_e2e" has spec.md with content "# Reserve E2E Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## implementing

      - [Reserve E2E](tracks/20260604T1200_reserve_e2e/) — reserve e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-04T00:00:00Z
      last_updated: 2026-06-04T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Implementing (1)

      - [Reserve E2E](tracks/20260604T1200_reserve_e2e/)
      """
    And a playbook hook body for the track hook "implementing.md" with content "E2E-REWARM: RESERVE-IMPL-HOOK"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized

  # After a resumer opens a begin on the implementing track, a checkin under the
  # same actor name re-serves that state's doer hook into the response context.
  Scenario: checkin re-serves the open-begin hook to the resuming session
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-test     |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    When a begin tools/call is sent with:
      | field          | value                            |
      | identifier     | 20260604T1200_reserve_e2e        |
      | actor_name     | Resumer-E2E-RESERVE-001          |
      | actor_type     | agent                            |
      | actor_model    | claude-test                      |
      | actor_provider | anthropic                        |
    Then the begin response has state "implementing"
    When a checkin tools/call is sent with role "resumer" and:
      | field          | value                   |
      | actor_name     | Resumer-E2E-RESERVE-001 |
      | actor_type     | agent                   |
      | actor_model    | claude-test             |
      | actor_provider | anthropic               |
    Then the checkin response actor_name is "Resumer-E2E-RESERVE-001"
    And the checkin response context contains "E2E-REWARM: RESERVE-IMPL-HOOK"
