Feature: MCP begin spec_revision JSON shape
  The MCP shim's begin response for a doer-on-spec_revision session carries
  context_text and review_doc_path keys and OMITS the artifact_text key entirely
  (field absent, not present-and-empty). Strict JSON key-set assertion per R2.4.

  Scenario: Doer begin(identifier) on spec_revision omits the artifact_text key
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T2500_revision_shim/                | spec_revision |
    And the track "20260419T2500_revision_shim" has spec.md with content "# Revision Shim\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [revision shim](tracks/20260419T2500_revision_shim/) — revision shim

      ## plan
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-19T00:00:00Z
      last_updated: 2026-04-19T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      - [revision shim](tracks/20260419T2500_revision_shim/)
      """
    And a playbook hook body for the track hook "spec-revision.md" with content "# Spec Revision Context\n\nWill address / Acknowledged, not addressing."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260419T2500_revision_shim"
    Then the begin response has context text containing "Will address"
    And the begin response has a review_doc_path
    And the begin response does not have key "artifact_text"
