Feature: Checkin Role Filtering
  The reshaped checkin validates the declared role and returns
  a filtered view of the hearth based on intent.

  Scenario: Creator role returns active proposals and available types
    Given a hearth with artifacts for checkin query:
      | id                                      | type     | state         | summary         |
      | 20260411T2021_anvil_workflow_engine      | proposal | active        | Playbook engine |
      | 20260413T1349_checkin_decomposition      | track    | implementing  | Decompose tools |
      | 20260412T0000_old_proposal               | proposal | completed     | Old proposal    |
    And the checkin query word list is "Cibola,Delta,Echo"
    When checkin query is executed with role "creator"
    Then the checkin query result has a generated actor name
    And the checkin query filtered artifacts include "20260411T2021_anvil_workflow_engine" with state "active"
    And the checkin query filtered artifacts do not include "20260413T1349_checkin_decomposition"
    And the checkin query filtered artifacts do not include "20260412T0000_old_proposal"
    And the checkin query result includes available type "track"

  Scenario: Resumer role returns doer-actionable artifacts
    Given a hearth with artifacts for checkin query:
      | id                                      | type     | state         | summary         |
      | 20260411T2021_anvil_workflow_engine      | proposal | active        | Playbook engine |
      | 20260413T1349_checkin_decomposition      | track    | implementing  | Decompose tools |
      | 20260412T0000_some_track                 | track    | spec_review   | Under review    |
      | 20260412T0001_done_track                 | track    | completed     | Done            |
    And the checkin query word list is "Cibola,Delta,Echo"
    When checkin query is executed with role "resumer"
    Then the checkin query result has a generated actor name
    And the checkin query filtered artifacts include "20260413T1349_checkin_decomposition" with state "implementing"
    And the checkin query filtered artifacts do not include "20260411T2021_anvil_workflow_engine"
    And the checkin query filtered artifacts do not include "20260412T0000_some_track"
    And the checkin query filtered artifacts do not include "20260412T0001_done_track"
    And the checkin query result has no available types

  Scenario: Reviewer role returns review-pending artifacts
    Given a hearth with artifacts for checkin query:
      | id                                      | type     | state           | summary         |
      | 20260413T1349_checkin_decomposition      | track    | spec_review     | Under review    |
      | 20260412T0000_other_track                | track    | implementing    | Being built     |
      | 20260411T2021_proposal                   | proposal | proposal_review | Reviewing       |
    And the checkin query word list is "Cibola,Delta,Echo"
    When checkin query is executed with role "reviewer"
    Then the checkin query result has a generated actor name
    And the checkin query filtered artifacts include "20260413T1349_checkin_decomposition" with state "spec_review"
    And the checkin query filtered artifacts include "20260411T2021_proposal" with state "proposal_review"
    And the checkin query filtered artifacts do not include "20260412T0000_other_track"
    And the checkin query result has no available types

  Scenario: Reviewer role also returns tracks awaiting spec review
    Given a hearth with artifacts for checkin query:
      | id                                      | type     | state           | summary          |
      | 20260414T0405_spec_strand                | track    | spec            | Awaiting review  |
      | 20260413T1349_checkin_decomposition      | track    | spec_review     | Under review     |
      | 20260412T0000_old_track                  | track    | spec_revision   | Round-2 pending  |
      | 20260412T0001_plan_track                 | track    | plan            | Planning         |
    And the checkin query word list is "Cibola,Delta,Echo"
    When checkin query is executed with role "reviewer"
    Then the checkin query filtered artifacts include "20260414T0405_spec_strand" with state "spec"
    And the checkin query filtered artifacts include "20260413T1349_checkin_decomposition" with state "spec_review"
    And the checkin query filtered artifacts do not include "20260412T0000_old_track"
    And the checkin query filtered artifacts do not include "20260412T0001_plan_track"

  Scenario: Invalid role returns structured error
    Given a hearth with artifacts for checkin query:
      | id                                      | type     | state  | summary         |
      | 20260411T2021_anvil_workflow_engine      | proposal | active | Playbook engine |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "admin"
    Then the checkin query returns an UnsupportedRole error for "admin"
