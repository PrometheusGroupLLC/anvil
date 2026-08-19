Feature: Begin RPC — reviewer rejection on spec_revision
  A reviewer calling begin(identifier) on a track in spec_revision is rejected
  with FAILED_PRECONDITION and the spec_not_ready_for_review code, carrying the
  distinct revision-case message. Per spec R4.2.

  Scenario: begin(identifier, reviewer) on track in spec_revision returns spec_not_ready_for_review
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260419T1700_rpc_revision_reviewer/        | spec_revision |
    And the track "20260419T1700_rpc_revision_reviewer" has spec.md with content "# RPC Revision Reviewer\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [rpc revision reviewer](tracks/20260419T1700_rpc_revision_reviewer/) — rpc revision reviewer

      ## plan
      """
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260419T1700_rpc_revision_reviewer" and session_role "reviewer"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "spec_not_ready_for_review"
    And the begin RPC error message contains "revision"
