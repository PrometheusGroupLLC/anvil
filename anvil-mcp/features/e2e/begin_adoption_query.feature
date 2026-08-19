Feature: Begin-adoption status query end-to-end (AC-5, harness-agnostic)
  The MCP shim exposes `begin_adoption_status` as a pure-read MCP tool. This
  e2e feature exercises the full marker → query → close lifecycle through the
  shim/JSON-RPC seam, proving the path the harness hard-gate will use.

  Background:
    Given a hearth directory with the following structure:
      | path                                                   | state       |
      | proposals/20260411T2021_anvil_workflow_engine/          | active      |
      | tracks/20260604T1000_begin_adoption_query_e2e/          | spec_review |
    And the track "20260604T1000_begin_adoption_query_e2e" has spec.md with content "# Query E2E Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Begin Adoption Query E2E](tracks/20260604T1000_begin_adoption_query_e2e/) — begin adoption query e2e — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

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

      ## Spec (0)

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | Begin Adoption Query E2E | anvil-playbook-engine |

      ## Planned (0)

      ## Implementing (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Review criteria."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized

  # (a) No begin → has_open_begin false
  Scenario: begin_adoption_status returns false when actor has never begun
    When a begin_adoption_status tools/call is sent with:
      | actor_name    | Reviewer-E2E-BA-001001                            |
      | artifact_path | tracks/20260604T1000_begin_adoption_query_e2e      |
      | state         | spec_review                                        |
    Then the begin_adoption_status response has_open_begin is false

  # (b) After reviewer begin, has_open_begin true
  Scenario: begin_adoption_status returns true after reviewer calls begin
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-test     |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    When a begin tools/call is sent with:
      | field          | value                                              |
      | identifier     | 20260604T1000_begin_adoption_query_e2e             |
      | actor_name     | Reviewer-E2E-BA-001002                             |
      | actor_type     | agent                                              |
      | actor_model    | claude-test                                        |
      | actor_provider | anthropic                                          |
    Then the begin response has state "spec_review"
    When a begin_adoption_status tools/call is sent with:
      | actor_name    | Reviewer-E2E-BA-001002                            |
      | artifact_path | tracks/20260604T1000_begin_adoption_query_e2e      |
      | state         | spec_review                                        |
    Then the begin_adoption_status response has_open_begin is true

  # (c) After begin then complete, has_open_begin false
  Scenario: begin_adoption_status returns false after reviewer completes (marker closed)
    When a checkin tools/call is sent with role "reviewer" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-test     |
      | actor_provider | anthropic       |
    Then the checkin response has a generated actor name
    When a begin tools/call is sent with:
      | field          | value                                              |
      | identifier     | 20260604T1000_begin_adoption_query_e2e             |
      | actor_name     | Reviewer-E2E-BA-001003                             |
      | actor_type     | agent                                              |
      | actor_model    | claude-test                                        |
      | actor_provider | anthropic                                          |
    Then the begin response has state "spec_review"
    When a complete tools/call is sent with:
      | field          | value                                              |
      | artifact_path  | tracks/20260604T1000_begin_adoption_query_e2e      |
      | actor_name     | Reviewer-E2E-BA-001003                             |
      | actor_type     | agent                                              |
      | actor_model    | claude-test                                        |
      | actor_provider | anthropic                                          |
      | satisfaction   | satisfied                                          |
    Then the complete response new_state is "plan"
    When a begin_adoption_status tools/call is sent with:
      | actor_name    | Reviewer-E2E-BA-001003                            |
      | artifact_path | tracks/20260604T1000_begin_adoption_query_e2e      |
      | state         | spec_review                                        |
    Then the begin_adoption_status response has_open_begin is false
