Feature: DescribeQueryHandler exercises PlaybookRegistry as a production consumer
  # R9/AC10: production-consumer-perspective feature binding PlaybookRegistry to
  # describe::available_actions through DescribeQueryHandler.
  #
  # These scenarios drive through the handler's consumer boundary (the seam) so
  # the PlaybookRegistry port is exercised by a production consumer, not just
  # by adapter-level direct calls. This closes Track #11's documented weakness on
  # the `tests-from-user-perspective` and `feature-files-bind-intent` initiatives.

  Scenario: DescribeQueryHandler returns available_actions from HearthPlaybookRegistry for a track in spec state
    Given a scratch hearth with the track machine.yaml loaded
    And a HearthPlaybookRegistry pointed at that scratch hearth
    And a track artifact in state "spec" in the scratch hearth
    When DescribeQueryHandler execute is called with that registry and artifact
    Then the describe result contains available_actions with action "spec_review" and role "spec"

  Scenario: DescribeQueryHandler returns available_actions for a track in implementing state
    Given a scratch hearth with the track machine.yaml loaded
    And a HearthPlaybookRegistry pointed at that scratch hearth
    And a track artifact in state "implementing" in the scratch hearth
    When DescribeQueryHandler execute is called with that registry and artifact
    Then the describe result contains available_actions with action "impl_phase_review" and role "reviewer"
    And the describe result contains available_actions with action "impl_review" and role "implement"

  Scenario: DescribeQueryHandler returns empty available_actions when registry misses the kind
    Given an empty HearthPlaybookRegistry with no playbook artifacts
    And a track artifact in state "spec" in a scratch hearth
    When DescribeQueryHandler execute is called with that registry and artifact
    Then the describe result available_actions list is empty
