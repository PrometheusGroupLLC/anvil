Feature: Begin-adoption warning in standalone mode (AC-8)
  Standalone mode (no FOUNDRY_SESSION_TOKEN): when an actor completes a driven
  artifact (track) with no prior begin-marker, the JSON-RPC complete response
  carries the begin_adoption warning AND the transition still succeeds.
  Proves the seam keys on actor_name + artifact (present in every mode) and
  is orthogonal to auth — modeled on standalone_unchanged.feature.

  Background:
    Given a hearth directory with the following structure:
      | path                                               | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260604T0900_standalone_begin_adoption/    | spec   |
    And the track "20260604T0900_standalone_begin_adoption" has spec.md with content "# Standalone Begin Adoption Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Standalone Begin Adoption](tracks/20260604T0900_standalone_begin_adoption/) — standalone begin adoption — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
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

      ## Spec (1)

      | Track | Proposal |
      |-------|----------|
      | Standalone Begin Adoption | anvil-playbook-engine |

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active

  Scenario: Standalone complete with no begin-marker warns and still transitions
    When a complete tools/call is sent with:
      | field          | value                                               |
      | artifact_path  | tracks/20260604T0900_standalone_begin_adoption      |
      | actor_name     | StandaloneActor-BA-999001                           |
      | actor_type     | agent                                               |
      | actor_model    | claude-test                                         |
      | actor_provider | anthropic                                           |
    Then the complete response new_state is "spec_review"
    And the complete response transition_at is non-empty
    And the complete response warnings contains "begin_adoption"
    And the resolved state of "tracks/20260604T0900_standalone_begin_adoption" in the hearth is "spec_review"
