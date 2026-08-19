Feature: Registry enumeration — listing every resolvable machine

  BP0 (playbook_routing_layer): the PlaybookRegistry port gains an enumeration
  API (all_machines / kinds) so a consumer can list every resolvable machine.
  This is the candidate-set data source the in-engine router consumes. The
  HearthPlaybookRegistry enumerates only successfully-loaded machines (a
  malformed machine.yaml never enters the map, so it is absent from the
  enumeration — AC9: active = registry-resolvable). The SeedPlaybookRegistry
  enumerates exactly its compiled-in kinds.

  Scenario: HearthPlaybookRegistry enumerates loaded kinds and omits malformed ones
    Given a hearth with a valid "track" machine on disk
    And the hearth also has a valid "knowledge_lifecycle" machine on disk
    And the hearth also has a malformed machine.yaml on disk
    When I enumerate the hearth registry kinds
    Then the enumerated kinds include "track"
    And the enumerated kinds include "knowledge_lifecycle"
    And the enumerated kinds do not include "broken"

  @registration
  Scenario: SeedPlaybookRegistry enumerates built-in machine kinds
    When I enumerate the seed registry kinds
    Then the seed enumerated kinds are exactly "backlog_item,decision,initiative,learning,milestone,playbook,proposal,spark,track"
