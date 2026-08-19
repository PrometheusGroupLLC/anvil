Feature: AC6 — the builder routes as a driven candidate
  The playbook_generation builder declares register: driven (the default), so the
  registry enumerates it as a driven candidate the surface LLM can select and
  begin. route(...) over a hearth holding the builder machine includes
  playbook_generation in the candidate set.

  Scenario: route includes playbook_generation as a driven candidate
    Given a route hearth seeded with the builder machine
    And the engine is started with that hearth
    When the route RPC is called with message "I want to author a new playbook"
    Then the route outcome is "candidates"
    And the route candidates include kind "playbook_generation"
    And the route candidate "playbook_generation" carries a description and required_fields metadata
