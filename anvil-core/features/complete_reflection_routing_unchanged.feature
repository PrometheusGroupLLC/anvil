Feature: Routing table unchanged by reflection_notes addition
  Per spec R8.1: adding reflection_notes is an input-shape widening, not a
  new playbook. No compute_execution_route entries change.
  These scenarios are regression guards — they assert current Slice A
  routing values are preserved and no new routing entries were added.

  Scenario: filtered_artifact_playbook (track, spec, reviewer) is none
    Given a hearth with artifacts for checkin query:
      | id                        | type  | state | summary             |
      | 20260420T0210_spec_track  | track | spec  | Routing guard spec  |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "reviewer"
    Then the checkin query filtered artifacts include "20260420T0210_spec_track" with execution_route "none"

  Scenario: filtered_artifact_playbook (track, spec_review, reviewer) is engine
    Given a hearth with artifacts for checkin query:
      | id                             | type  | state       | summary                    |
      | 20260420T0210_spec_review_track | track | spec_review | Routing guard spec_review  |
    And the checkin query word list is "Cibola"
    When checkin query is executed with role "reviewer"
    Then the checkin query filtered artifacts include "20260420T0210_spec_review_track" with execution_route "engine"

  Scenario: available_action_playbook (track, spec, spec doer) is engine
    Given a describe handler with instances:
      | id                            | kind  | state | transitions |
      | 20260420T0210_routing_spec    | track | spec  | 1           |
    When describe is called with identifier "20260420T0210_routing_spec"
    Then the describe result is instance info
    And the describe instance available actions include "spec_review" with execution_route "engine"

  Scenario: available_action_playbook (track, spec_review, reviewer) is engine
    Given a describe handler with instances:
      | id                                  | kind  | state       | transitions |
      | 20260420T0210_routing_spec_review   | track | spec_review | 2           |
    When describe is called with identifier "20260420T0210_routing_spec_review"
    Then the describe result is instance info
    And the describe instance available actions include "plan" with execution_route "engine"
