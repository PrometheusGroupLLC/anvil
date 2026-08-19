Feature: Fallback kinds return empty available_actions unchanged
  # Phase 5 scope-boundary regression (R5.5 fallback-to-skill AC):
  # The _ => Vec::new() fallback arm of describe::available_actions covers
  # all non-track, non-proposal artifact kinds. The Phase 5 migration must
  # NOT change behavior for the remaining fallback kinds.
  #
  # These kinds' available actions are served by dev-playbook skills (via the
  # execution_route fallback routing table), not by the engine directly.
  # Decision, initiative, milestone, and learning are no longer part of this
  # fallback set; they are engine-driven free artifacts.

  Scenario: Milestone in draft state returns engine-driven available_actions
    When available_actions is called with kind "milestone" and state "draft"
    Then the available actions list has 1 entries
    And the available actions list includes action "draft_review" with role "doer"

  Scenario: Initiative in active state returns engine-driven available_actions
    When available_actions is called with kind "initiative" and state "active"
    Then the available actions list has 4 entries
    And the available actions list includes action "promoted" with role "promote"
    And the available actions list includes action "retired" with role "retire"
    And the available actions list includes action "active" with role "log"
    And the available actions list includes action "active" with role "reflect"

  Scenario: Learning in observation state returns engine-driven available_actions
    When available_actions is called with kind "learning" and state "observation"
    Then the available actions list has 2 entries
    And the available actions list includes action "observation_review" with role "learn"
    And the available actions list includes action "conclusion" with role "learn"

  # The subject here is the LEGACY kind string still sitting on artifacts that
  # C-p.1 has not migrated: `status.yaml.kind: workflow` resolves no machine and
  # therefore surfaces no actions. That is unchanged by the rename, and it is
  # not the canonical `playbook` kind — which DOES resolve its lifecycle machine
  # and surfaces its outgoing actions, so asserting "empty" of it would be false.
  Scenario: The legacy definition-artifact kind in any state returns empty available_actions
    When available_actions is called with kind "workflow" and state "draft"
    Then the available actions list is empty

  Scenario: Unknown kind returns empty available_actions
    When available_actions is called with kind "unknown_kind" and state "some_state"
    Then the available actions list is empty
