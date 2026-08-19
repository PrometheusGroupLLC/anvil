Feature: Begin RPC resumer serves doer hooks
  A resumer begin on track doer states is engine-handled: it returns the
  doer hook body, doer measurement fields, and writes a begin-marker.

  Scenario: resumer begin on plan returns the doer hook and measurement
    Given a hearth directory with the following structure:
      | path                                              | state  |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260614T0620_resumer_plan/                 | plan   |
    And the track "20260614T0620_resumer_plan" has spec.md with content "# Resumer Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Resumer Track](tracks/20260614T0620_resumer_plan/) — resumer track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## implementing

      ## reflecting
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

      ## Planned (1)

      - [Resumer Track](tracks/20260614T0620_resumer_plan/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "RPC-HOOK: RESUMER-PLAN"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0620_resumer_plan" and session_role "resumer" and actor_name "Resumer-210001"
    Then the begin RPC response state is "plan"
    And the begin RPC response "context_text" is exactly "RPC-HOOK: RESUMER-PLAN"
    And the begin RPC response has non-empty "intent"
    And the begin RPC response has non-empty "expected_output"
    And the hearth file "tracks/20260614T0620_resumer_plan/status.yaml" contains "kind: begin"
    And the hearth file "tracks/20260614T0620_resumer_plan/status.yaml" contains "actor: Resumer-210001"

  Scenario: resumer begin on implementing returns the doer hook and measurement
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260614T0621_resumer_implementing/         | implementing |
    And the track "20260614T0621_resumer_implementing" has spec.md with content "# Resumer Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## implementing

      - [Resumer Track](tracks/20260614T0621_resumer_implementing/) — resumer track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
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

      - [Resumer Track](tracks/20260614T0621_resumer_implementing/)
      """
    And a playbook hook body for the track hook "implementing.md" with content "RPC-HOOK: RESUMER-IMPL"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0621_resumer_implementing" and session_role "resumer" and actor_name "Resumer-210001"
    Then the begin RPC response state is "implementing"
    And the begin RPC response "context_text" is exactly "RPC-HOOK: RESUMER-IMPL"
    And the begin RPC response has non-empty "intent"
    And the begin RPC response has non-empty "expected_output"
    And the hearth file "tracks/20260614T0621_resumer_implementing/status.yaml" contains "kind: begin"
    And the hearth file "tracks/20260614T0621_resumer_implementing/status.yaml" contains "actor: Resumer-210001"

  Scenario: resumer begin on reflecting returns the doer hook and measurement
    Given a hearth directory with the following structure:
      | path                                              | state      |
      | proposals/20260411T2021_anvil_workflow_engine/     | active     |
      | tracks/20260614T0622_resumer_reflecting/           | reflecting |
    And the track "20260614T0622_resumer_reflecting" has spec.md with content "# Resumer Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## reflecting

      - [Resumer Track](tracks/20260614T0622_resumer_reflecting/) — resumer track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)
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

      ## Reflecting (1)

      - [Resumer Track](tracks/20260614T0622_resumer_reflecting/)
      """
    And a playbook hook body for the track hook "reflecting.md" with content "RPC-HOOK: RESUMER-REFLECT"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260614T0622_resumer_reflecting" and session_role "resumer" and actor_name "Resumer-210001"
    Then the begin RPC response state is "reflecting"
    And the begin RPC response "context_text" is exactly "RPC-HOOK: RESUMER-REFLECT"
    And the begin RPC response has non-empty "intent"
    And the begin RPC response has non-empty "expected_output"
    And the hearth file "tracks/20260614T0622_resumer_reflecting/status.yaml" contains "kind: begin"
    And the hearth file "tracks/20260614T0622_resumer_reflecting/status.yaml" contains "actor: Resumer-210001"
