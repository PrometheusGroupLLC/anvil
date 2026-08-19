Feature: CompositePlaybookRegistry — hearth-first with seed fallback

  The CompositePlaybookRegistry attempts hearth resolution first and falls
  back to the seed registry on miss. Test doubles (FakeWorkflowRegistry
  structs with a fixed HashMap) are used for both hearth and seed slots so
  this feature isolates composite logic from adapter implementation details.

  Scenario: Composite registry returns hearth result when hearth has the kind
    Given a fake hearth registry with kind "track"
    And a fake seed registry with no kinds
    When the composite registry resolves kind "track"
    Then the composite-resolved machine has kind "track"

  Scenario: Composite registry falls back to seed when hearth misses
    Given a fake hearth registry with no kinds
    And a fake seed registry with kind "workflow"
    When the composite registry resolves kind "workflow"
    Then the composite-resolved machine has kind "workflow"

  Scenario: Composite registry returns None when both miss
    Given a fake hearth registry with no kinds
    And a fake seed registry with no kinds
    When the composite registry resolves kind "proposal"
    Then the composite-resolved machine is absent

  # M3 (hook_content_serving P5): workflow_id_for delegates with the same
  # hearth-first-then-seed order as machine_for. The id is slot-labeled so the
  # delegation order is observable: "hearth:{kind}" vs "seed:{kind}".
  Scenario: Composite workflow_id_for returns the hearth id when hearth has the kind
    Given an id hearth registry with kind "track"
    And an id seed registry with kind "track"
    When the composite registry resolves playbook id for kind "track"
    Then the composite-resolved playbook id is "hearth:track"

  Scenario: Composite workflow_id_for falls back to the seed id when hearth misses
    Given an id hearth registry with no kinds
    And an id seed registry with kind "track"
    When the composite registry resolves playbook id for kind "track"
    Then the composite-resolved playbook id is "seed:track"

  Scenario: Composite workflow_id_for returns None when both miss
    Given an id hearth registry with no kinds
    And an id seed registry with no kinds
    When the composite registry resolves playbook id for kind "proposal"
    Then the composite-resolved playbook id is absent

  # BP0 (workflow_routing_layer): enumeration API. The composite lists every
  # resolvable machine across hearth + seed, de-duped by kind with hearth
  # precedence — this is the candidate-set data source the router consumes.
  Scenario: Composite enumerates hearth and seed kinds with hearth-first de-dup
    Given a fake hearth registry with kinds "track,alpha"
    And a fake seed registry with kinds "track,workflow"
    When the composite registry enumerates all kinds
    Then the enumerated kinds are "track,alpha,workflow"
    And the enumerated machine for kind "track" came from the hearth
