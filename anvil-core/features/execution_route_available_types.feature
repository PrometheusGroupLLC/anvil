Feature: execution_route discriminator on available types
  Each AvailableArtifactType in CheckinResponse.available_types carries a
  execution_route value. `track`, `proposal`, `milestone`, `initiative`,
  and `decision` are engine-supported create flows today.

  Scenario: Creator sees execution_route on every type
    Given a hearth with artifacts for checkin query:
      | id                                  | type     | state  | summary         |
      | 20260411T2021_anvil_workflow_engine | proposal | active | Playbook engine |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "creator"
    Then the checkin query available type "track" has execution_route "engine"
    And the checkin query available type "proposal" has execution_route "engine"
    And the checkin query available type "milestone" has execution_route "engine"
    And the checkin query available type "initiative" has execution_route "engine"
    And the checkin query available type "decision" has execution_route "engine"
