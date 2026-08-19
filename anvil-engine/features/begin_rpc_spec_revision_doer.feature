Feature: Begin RPC — doer revision context on spec_revision
  The engine's Begin RPC accepts identifier + session_role "creator" for a doer
  re-entering a track in spec_revision, returning revision context from the
  (spec_revision, doer) hook (spec-revision.md), a computed review_doc_path, and
  no transition (the track stays in spec_revision). Per spec R2.

  Scenario: begin(identifier, creator) on track in spec_revision returns revision context, no transition
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T1600_rpc_revision_doer/            | spec_revision |
    And the track "20260419T1600_rpc_revision_doer" has spec.md with content "# RPC Revision Doer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [rpc revision doer](tracks/20260419T1600_rpc_revision_doer/) — rpc revision doer

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

      - [rpc revision doer](tracks/20260419T1600_rpc_revision_doer/)
      """
    And a playbook hook body for the track hook "spec-revision.md" with content "# Spec Revision Context\n\nWill address / Acknowledged, not addressing. After revision, call complete(artifact_path)."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260419T1600_rpc_revision_doer" and session_role "creator"
    Then the begin RPC response state is "spec_revision"
    And the begin RPC response has non-empty "context_text"
    And the begin RPC response "context_text" contains "Will address"
    And the begin RPC response has non-empty "review_doc_path"
    And the begin RPC response "review_doc_path" contains "spec.review.md"
    And the begin RPC response "artifact_text" is exactly ""
    And the resolved state of "tracks/20260419T1600_rpc_revision_doer" in the hearth is "spec_revision"
