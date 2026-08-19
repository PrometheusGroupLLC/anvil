Feature: Begin spec_revision (doer revision context) — end-to-end
  A doer re-entering a `spec_revision` track calls `checkin(creator)` then
  `begin(identifier)` via MCP and receives revision context from
  `spec-revision.md`, a computed review_doc_path, and a response whose JSON body
  OMITS the artifact_text key. No state transition is recorded. Per spec R2.

  Scenario: Doer checkin(creator) then begin(identifier) on a spec_revision track delivers revision context, no transition
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T2200_revision_doer_e2e/            | spec_revision |
    And the track "20260419T2200_revision_doer_e2e" has spec.md with content "# Revision Doer E2E\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [revision doer e2e](tracks/20260419T2200_revision_doer_e2e/) — revision doer e2e

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

      - [revision doer e2e](tracks/20260419T2200_revision_doer_e2e/)
      """
    And a playbook hook body for the track hook "spec-revision.md" with content "# Spec Revision Context\n\nWill address / Acknowledged, not addressing. After revision, call complete(artifact_path)."
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a checkin tools/call is sent with role "creator" and:
      | field          | value           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-6 |
      | actor_provider | anthropic       |
    When a begin tools/call is sent with identifier "20260419T2200_revision_doer_e2e"
    Then the begin response has state "spec_revision"
    And the begin response has context text containing "Will address"
    And the begin response review_doc_path ends with "spec.review.md"
    And the begin response does not have key "artifact_text"
    And the resolved state of "tracks/20260419T2200_revision_doer_e2e" in the hearth is "spec_revision"
