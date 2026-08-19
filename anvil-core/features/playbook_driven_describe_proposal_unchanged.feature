Feature: Proposal describe actions are driven by the proposal lifecycle machine
  The proposal arm of describe::available_actions is now interpreter-driven.

  Scenario Outline: Proposal in <state> returns machine-declared actions
    When available_actions is called with kind "proposal" and state "<state>"
    Then the available actions list has <count> entries
    And the available actions list includes action "<first_action>" with role "<first_role>"

    Examples:
      | state               | count | first_action        | first_role |
      | vision              | 1     | vision_review       | envision   |
      | vision_review       | 2     | vision_revision     | envision   |
      | vision_revision     | 1     | vision_review       | envision   |
      | draft               | 1     | draft_review        | propose    |
      | draft_review        | 2     | draft_revision      | propose    |
      | draft_revision      | 1     | draft_review        | propose    |
      | proposal            | 1     | proposal_review     | propose    |
      | proposal_review     | 2     | proposal_revision   | propose    |
      | proposal_revision   | 1     | proposal_review     | propose    |
      | active              | 5     | amend               | amend      |
      | amend               | 1     | amend_review        | amend      |
      | amend_review        | 2     | amend_revision      | amend      |
      | amend_revision      | 1     | amend_review        | amend      |
      | reflecting          | 1     | reflection_review   | reflect    |
      | reflection_review   | 2     | reflection_revision | reflect    |
      | reflection_revision | 1     | reflection_review   | reflect    |

  # Second actions for multi-action proposal states
  Scenario: vision_review second action is draft with role reviewer
    When available_actions is called with kind "proposal" and state "vision_review"
    Then the available actions list includes action "draft" with role "reviewer"

  Scenario: draft_review second action is proposal with role reviewer
    When available_actions is called with kind "proposal" and state "draft_review"
    Then the available actions list includes action "proposal" with role "reviewer"

  Scenario: proposal_review second action is active with role reviewer
    When available_actions is called with kind "proposal" and state "proposal_review"
    Then the available actions list includes action "active" with role "reviewer"

  Scenario: proposal active terminal actions include completed
    When available_actions is called with kind "proposal" and state "active"
    Then the available actions list includes action "completed" with role "doer"

  Scenario: proposal reflection_review second action returns active with role reviewer
    When available_actions is called with kind "proposal" and state "reflection_review"
    Then the available actions list includes action "active" with role "reviewer"
