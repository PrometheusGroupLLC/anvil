Feature: Scope boundary — untouched consumers unchanged for track kind
  # Phase 5 scope-boundary regression (R12.5 / R5.5):
  # The three functions NOT touched by the track-kind describe migration are
  # asserted byte-identical to pre-Phase-5 HEAD for every track state.
  #
  # Functions under test (all pure, no I/O):
  #   snapshot::registry_section_for(kind, state) -> Option<&'static str>
  #   snapshot::projection_targets_for(kind, state, projection_only=false, event_type="")
  #   routing::compute_execution_route(SUBJECT_AVAILABLE_ACTION, kind, state, role)
  #
  # Expected values captured from HEAD at Phase 5 start and preserved here.

  Scenario Outline: registry_section_for returns unchanged section for track/<state>
    When registry_section_for is called with kind "track" and state "<state>"
    Then the registry section is "<expected_section>"

    Examples:
      | state               | expected_section |
      | spec                | spec             |
      | spec_review         | spec             |
      | spec_revision       | spec             |
      | plan                | plan             |
      | plan_review         | plan             |
      | plan_revision       | plan             |
      | implementing        | implementing     |
      | impl_phase_review   | implementing     |
      | impl_review         | implementing     |
      | impl_revision       | implementing     |
      | reflecting          | reflecting       |
      | reflection_review   | reflecting       |
      | reflection_revision | reflecting       |
      | completed           | completed        |
      | abandoned           | abandoned        |
      | superseded          | superseded       |

  Scenario Outline: projection_targets_for returns Execution for track/<state>
    When projection_targets_for is called with kind "track" and state "<state>"
    Then the projection targets is "Execution"

    Examples:
      | state               |
      | spec                |
      | spec_review         |
      | plan                |
      | plan_review         |
      | implementing        |
      | impl_phase_review   |
      | impl_review         |
      | reflecting          |
      | reflection_review   |
      | completed           |
      | abandoned           |
      | superseded          |

  Scenario Outline: projection_targets_for returns empty for track revision states
    When projection_targets_for is called with kind "track" and state "<state>"
    Then the projection targets is ""

    Examples:
      | state               |
      | spec_revision       |
      | plan_revision       |
      | impl_revision       |
      | reflection_revision |

  Scenario Outline: compute_execution_route for available_action on track
    When compute_execution_route is called for available_action kind "track" state "<state>" role "<role>"
    Then the supported playbook result is "<expected>"

    Examples:
      | state    | role     | expected                |
      | spec     | spec     | engine                  |
      | spec_review | reviewer | engine               |
      | plan     | plan      | engine                  |
      | plan_review | plan  | fallback:forge:review   |
      | implementing | implement | engine               |
