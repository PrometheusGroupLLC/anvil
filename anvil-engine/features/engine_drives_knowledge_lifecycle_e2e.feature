Feature: HEADLINE — the engine drives the knowledge_lifecycle end-to-end (AC4)
  A seeded knowledge_lifecycle artifact is driven begin → complete … → complete
  through the full happy path
  ingesting → ingest_review → organizing → compiling → compile_review →
  validating → validation_review → published, with each transition's to_state,
  required_role, and registry placement sourced from machine.yaml. ZERO "track"
  literals on the path. The driver step asserts each hop's machine-declared
  destination state internally.

  Scenario: drive a knowledge_lifecycle artifact ingesting → published
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the engine drives a knowledge_lifecycle artifact from ingesting to published
    Then the e2e final state is "published"
    And the e2e artifact resolved state is "published"
    And the e2e artifact status.yaml contains "kind: knowledge_lifecycle"
    And the hearth file "knowledge.md" contains "knowledge/"
