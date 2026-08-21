Feature: Review gates are engine-executed, and dead ends say so
  Forge does not exist. The engine serves review-gate context directly from
  the machine's hooks_by_role rather than handing back the address of a skill
  that was deleted.

  Every review-gate state of the track machine already declares its reviewer
  hook — plan_review has hooks_by_role.reviewer: plan-review.md, and that file
  ships. The content was always there; only the discriminator claimed
  otherwise. These scenarios pin the discriminator to the truth.

  Note on roles: a review gate's outgoing edges carry the DESTINATION doer
  role, not "reviewer". plan_review's three edges require plan, implement and
  doer respectively. That is why all three of a plan_review track's available
  actions previously collapsed to a single dead fallback, leaving the track
  with no executable path at all.

  Scenario Outline: every available action out of plan_review is engine-executed
    When compute_execution_route is called for available_action kind "track" state "plan_review" role "<role>"
    Then the supported playbook result is "engine"

    Examples:
      | role      |
      | plan      |
      | implement |
      | doer      |

  Scenario Outline: every review gate is engine-executed for its declared roles
    When compute_execution_route is called for available_action kind "track" state "<state>" role "<role>"
    Then the supported playbook result is "engine"

    Examples:
      | state             | role      |
      | spec_review       | reviewer  |
      | spec_review       | doer      |
      | plan_review       | plan      |
      | impl_phase_review | implement |
      | impl_phase_review | doer      |
      | impl_review       | implement |
      | impl_review       | reflect   |
      | impl_review       | doer      |
      | reflection_review | reflect   |
      | reflection_review | complete  |
      | reflection_review | doer      |
      | amend_review      | reviewer  |
      | amend_review      | doer      |

  # A resumer (doer) at a review gate has no pending action — the ball is in
  # the reviewer's court, and `begin` refuses that combination outright. So the
  # honest answer is "none", not "engine". Claiming "engine" here would recommit
  # the original defect in a new direction: a route promising execution that the
  # executing call then refuses. The doer's executable path at a gate is the
  # available_actions above, every one of which is engine-executed.
  Scenario Outline: a resumer holding a track at a review gate is told there is no doer action
    When compute_execution_route is called for filtered_artifact kind "track" state "<state>" role "resumer"
    Then the supported playbook result is "none"

    Examples:
      | state             |
      | spec_review       |
      | plan_review       |
      | impl_phase_review |
      | impl_review       |
      | reflection_review |
      | amend_review      |

  Scenario Outline: a reviewer looking at a review gate gets an executable route
    When compute_execution_route is called for filtered_artifact kind "track" state "<state>" role "reviewer"
    Then the supported playbook result is "engine"

    Examples:
      | state             |
      | spec_review       |
      | plan_review       |
      | impl_phase_review |
      | impl_review       |
      | reflection_review |
      | amend_review      |

  # The mutation-check on the residual decision. A terminal track has no
  # action, and saying "engine" there is the same lie as the old dead skill
  # address, pointed the other way. These scenarios fail if the default arm is
  # flipped to engine wholesale.
  Scenario Outline: a terminal track reports no action, not an engine action
    When compute_execution_route is called for filtered_artifact kind "track" state "<state>" role "resumer"
    Then the supported playbook result is "none"

    Examples:
      | state      |
      | completed  |
      | abandoned  |
      | superseded |

  Scenario: an unknown subject kind reports no action
    When compute_execution_route is called for available_action kind "track" state "not_a_state" role "not_a_role"
    Then the supported playbook result is "none"
