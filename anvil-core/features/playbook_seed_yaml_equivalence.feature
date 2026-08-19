Feature: Seed ↔ YAML equivalence — track kind machine.yaml mirrors seeds::track_seed()

  Scenario: track machine.yaml is structurally equal to seeds::track_seed()
    Given the track lifecycle machine.yaml is loaded from the fixture
    And the track seed is loaded from the compiled-in seed
    Then the two PlaybookMachine structs are structurally equal
