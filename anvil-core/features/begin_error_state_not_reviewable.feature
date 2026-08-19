Feature: Begin Error — State Not Reviewable
  begin(identifier) by a reviewer on an (artifact_kind, state) combo
  outside engine support returns StateNotReviewable naming forge:review.

  Scenario: Reviewer on proposal in draft returns StateNotReviewable
    Given an in-memory query adapter with proposal "20260411T2021_anvil_workflow_engine" in state "draft"
    When begin is called via query adapter with identifier "20260411T2021_anvil_workflow_engine" and session_role "reviewer"
    Then the begin outcome is a StateNotReviewable error naming "forge:review"
    And the begin outcome error message contains "forge:review"
    And the handler emitted no events
