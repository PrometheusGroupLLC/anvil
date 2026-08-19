Feature: AC1 — the rewritten builder machine loads and validates
  The playbook_generation builder machine (rewritten to the
  satisfaction-discriminated encoding) is physically held in a hearth under
  workflows/20260528T2321_workflow_generation/ with hooks/gathering.md present.
  When the HearthPlaybookRegistry loads it, the registry reports NO load errors
  and playbook_generation resolves to a machine — catching the silent-drop trap
  where a dangling hook reference would drop the machine from the registry map.

  Scenario: the builder machine resolves with no load errors
    Given a hearth seeded with the builder machine
    And the builder hearth registry is constructed
    Then the builder hearth registry has no load errors
    And the builder hearth registry resolves "playbook_generation" to a machine
