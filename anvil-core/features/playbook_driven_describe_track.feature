Feature: Playbook-driven describe available_actions for track kind
  # Phase 5: describe::available_actions for the track kind is driven by the
  # compiled-in track seed via the interpreter (R12.2–12.5). This feature
  # verifies:
  #   (g1) Per-state: each of the 13 non-terminal track states returns the
  #        actions the seed prescribes.
  #   (g2) Mutation propagates: a test-only seed override removes a transition;
  #        describe() returns the shorter list without any edit to describe.rs.
  #   (g3) No-exit state: a synthetic state with zero outgoing transitions
  #        returns an empty available_actions list.
  #
  # The mutation-propagates scenario is the architectural proof of the cutover:
  # it FAILS when describe.rs still uses the hardcoded match, PASSES when it
  # consults the seed.
  #
  # Background teardown note: the Background step runs before every scenario.
  # This guarantees that any thread-local seed override leaked by a prior
  # failing scenario is cleared before the next scenario begins — preventing
  # cross-scenario contamination when brine schedules multiple scenarios on the
  # same thread. The (g2)/(g3) scenarios also carry an explicit trailing clear
  # for documentation clarity; the Background clear makes that trailing step
  # redundant but harmless (double-clear is a no-op).

  Background:
    Given the track seed override is cleared

  # (g1) Per-state Scenario Outline
  # Each row is one of the 13 non-terminal track states. The expected actions
  # are the to_state values from the seed's outgoing transitions for that state,
  # concatenated as CSV (order matches seed order).
  Scenario Outline: Track in <state> returns seed-prescribed available actions
    When available_actions is called with kind "track" and state "<state>"
    Then the available actions list has <count> entries
    And the available actions list includes action "<first_action>" with role "<first_role>"

    # resume-signal context-awareness: every open lifecycle state gained a
    # machine-declared `→ abandoned` (park) action, appended AFTER its forward
    # edge(s) — so `first_action`/`first_role` are unchanged but each `count` is
    # one higher than before park.
    Examples:
      | state               | count | first_action        | first_role |
      | spec                | 2     | spec_review         | spec       |
      | spec_review         | 3     | spec_revision       | reviewer   |
      | spec_revision       | 2     | spec_review         | spec       |
      | plan                | 2     | plan_review         | plan       |
      | plan_review         | 3     | plan_revision       | plan       |
      | plan_revision       | 2     | plan_review         | reviewer   |
      | implementing        | 3     | impl_phase_review   | reviewer   |
      | impl_phase_review   | 2     | implementing        | implement  |
      | impl_review         | 3     | impl_revision       | implement  |
      | impl_revision       | 2     | impl_review         | reviewer   |
      | reflecting          | 2     | reflection_review   | reflect    |
      | reflection_review   | 3     | reflection_revision | reflect    |
      | reflection_revision | 2     | reflection_review   | reviewer   |

  # resume-signal context-awareness: park is a first-class, discoverable action
  # from every open lifecycle state (interpreter-driven from the seed's new
  # `→ abandoned` edges), with role "doer".
  Scenario Outline: Track in <state> exposes the abandon (park) action
    When available_actions is called with kind "track" and state "<state>"
    Then the available actions list includes action "abandoned" with role "doer"

    Examples:
      | state               |
      | spec                |
      | spec_review         |
      | plan                |
      | plan_review         |
      | implementing        |
      | impl_review         |
      | reflecting          |
      | reflection_review   |

  # Additional assertion for multi-action states (second action)
  Scenario: spec_review second action is plan with role reviewer
    When available_actions is called with kind "track" and state "spec_review"
    Then the available actions list includes action "plan" with role "reviewer"

  Scenario: plan_review second action is implementing with role implement
    When available_actions is called with kind "track" and state "plan_review"
    Then the available actions list includes action "implementing" with role "implement"

  Scenario: implementing second action is impl_review with role implement
    When available_actions is called with kind "track" and state "implementing"
    Then the available actions list includes action "impl_review" with role "implement"

  Scenario: impl_review second action is reflecting with role reflect
    When available_actions is called with kind "track" and state "impl_review"
    Then the available actions list includes action "reflecting" with role "reflect"

  Scenario: reflection_review second action is completed with role complete
    When available_actions is called with kind "track" and state "reflection_review"
    Then the available actions list includes action "completed" with role "complete"

  # B5b BP4: `completed` is now non-terminal — it surfaces the amend action
  # (completed → amend, role doer), interpreter-driven from the track machine.
  Scenario: completed surfaces the amend action with role doer
    When available_actions is called with kind "track" and state "completed"
    Then the available actions list includes action "amend" with role "doer"

  # `abandoned` and `superseded` remain terminal (no-exit states).
  Scenario: Terminal state abandoned returns empty available_actions
    When available_actions is called with kind "track" and state "abandoned"
    Then the available actions list is empty

  # (g2) Mutation propagates
  # A hearth with a modified machine.yaml (spec_review→spec_revision removed) is
  # loaded via HearthPlaybookRegistry. available_actions is called with that
  # registry. The result is shortened without any edit to describe.rs — the
  # architectural proof of the cutover: hearth artifact change propagates through
  # the registry into describe output.
  # spec_review now has three actions (spec_revision, plan, abandoned); omitting
  # the spec_review→spec_revision edge shortens the list to two (plan + the park
  # action), still proving the hearth-artifact change propagates through describe.
  Scenario: Mutation propagates — removing a transition from machine.yaml shortens describe output
    Given a hearth with a modified track machine.yaml that omits the spec_review to spec_revision transition
    When available_actions is called with registry from that hearth, kind "track", and state "spec_review"
    Then the available actions list has 2 entries
    And the available actions list includes action "plan" with role "reviewer"
    And the available actions list includes action "abandoned" with role "doer"

  # (g3) No-exit state via synthetic state override
  # The override injects a synthetic state with zero outgoing transitions.
  Scenario: No-exit state via seed override returns empty available_actions
    Given the track seed override adds a synthetic state with no outgoing transitions
    When available_actions is called with kind "track" and state "test_holding_state"
    Then the available actions list is empty
    And the track seed override is cleared
