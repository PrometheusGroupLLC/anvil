Feature: Begin Error Taxonomy at the gRPC Seam
  Each BeginError variant maps to a distinct gRPC Status code, with the
  message preserved so the MCP shim can surface fallback skill names
  to the agent.

  Scenario: SessionRequired maps to UNAUTHENTICATED
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role ""
    Then the begin RPC returns gRPC status "UNAUTHENTICATED"

  Scenario: ModeNotImplemented maps to UNIMPLEMENTED and names the mode
    Given a hearth directory with the following structure:
      | path                                              | state         |
      | proposals/20260411T2021_anvil_workflow_engine/     | active        |
      | tracks/20260414T0405_review_spec_strand/           | plan_review   |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "resumer"
    Then the begin RPC returns gRPC status "UNIMPLEMENTED"
    And the begin RPC error message contains "resumer"

  Scenario: RoleStateMismatch maps to INVALID_ARGUMENT
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "creator"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"

  Scenario: SpecNotReadyForReview maps to FAILED_PRECONDITION
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "spec_not_ready_for_review"

  Scenario: StateNotReviewable maps to FAILED_PRECONDITION and names the kind and state
    Given a hearth directory with the following structure:
      | path                                              | state  | kind     |
      | glossaries/20260411T2021_delivery_glossary/        | active | glossary |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260411T2021_delivery_glossary" and session_role "reviewer"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "is not engine-supported"

  Scenario: NotFound maps to NOT_FOUND
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "nonexistent_track_id" and session_role "reviewer"
    Then the begin RPC returns gRPC status "NOT_FOUND"

  Scenario: Empty actor_name maps to INVALID_ARGUMENT (ActorNameRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand", session_role "reviewer", and empty "actor_name"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "actor_name"

  Scenario: Empty actor_type maps to INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand", session_role "reviewer", and empty "actor_type"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "actor_type"

  Scenario: Empty actor_model maps to INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand", session_role "reviewer", and empty "actor_model"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "actor_model"

  Scenario: Empty actor_provider maps to INVALID_ARGUMENT (ActorParamsRequired)
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260414T0405_review_spec_strand/           | spec    |
    And a context file "spec-review.md" in the hearth with content "review protocol"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand", session_role "reviewer", and empty "actor_provider"
    Then the begin RPC returns gRPC status "INVALID_ARGUMENT"
    And the begin RPC error message contains "actor_provider"
