Feature: execution_route discriminator on filtered artifacts
  Each artifact returned in CheckinResponse.filtered_artifacts carries a
  execution_route value describing the action the session role would
  take on it — "engine" if the engine handles it, or "none" if no action
  exists for that role on that subject.

  Scenario: Reviewer on track in spec_review gets engine (context delivery)
    Given a hearth with artifacts for checkin query:
      | id                       | type     | state           | summary         |
      | 20260414T0405_spec_track  | track    | spec_review     | Ready for context|
      | 20260413T1349_plan_review | track    | plan_review     | In review        |
      | 20260412T2021_proposal    | proposal | proposal_review | In review        |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "reviewer"
    Then the checkin query filtered artifacts include "20260414T0405_spec_track" with execution_route "engine"
    And the checkin query filtered artifacts include "20260413T1349_plan_review" with execution_route "engine"
    And the checkin query filtered artifacts include "20260412T2021_proposal" with execution_route "engine"

  Scenario: Reviewer on track in spec has no action (post-cutover: doer drives spec -> spec_review)
    Given a hearth with artifacts for checkin query:
      | id                       | type     | state | summary         |
      | 20260414T0405_spec_track  | track    | spec  | Doer in progress|
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "reviewer"
    Then the checkin query filtered artifacts include "20260414T0405_spec_track" with execution_route "none"

  Scenario: Creator on proposal in active gets engine
    Given a hearth with artifacts for checkin query:
      | id                                  | type     | state  | summary         |
      | 20260411T2021_anvil_workflow_engine | proposal | active | Workflow engine |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "creator"
    Then the checkin query filtered artifacts include "20260411T2021_anvil_workflow_engine" with execution_route "engine"

  Scenario: Resumer gets engine support for migrated doer-actionable states
    Given a hearth with artifacts for checkin query:
      | id                        | type  | state               | summary   |
      | 20260401T0000_spec        | track | spec                | spec      |
      | 20260401T0001_plan        | track | plan                | plan      |
      | 20260401T0002_impl        | track | implementing        | impl      |
      | 20260401T0003_spec_rev    | track | spec_revision       | spec rev  |
      | 20260401T0004_plan_rev    | track | plan_revision       | plan rev  |
      | 20260401T0005_impl_rev    | track | impl_revision       | impl rev  |
      | 20260401T0006_reflecting  | track | reflecting          | reflect   |
      | 20260401T0007_reflect_rev | track | reflection_revision | refl rev  |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "resumer"
    Then the checkin query filtered artifacts include "20260401T0000_spec" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0001_plan" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0002_impl" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0003_spec_rev" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0004_plan_rev" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0005_impl_rev" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0006_reflecting" with execution_route "engine"
    And the checkin query filtered artifacts include "20260401T0007_reflect_rev" with execution_route "engine"
