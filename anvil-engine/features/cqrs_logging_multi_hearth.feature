Feature: Multi-hearth logging attribution and no-deadlock capture
  Two catalog calls to two hearths log two different resolved hearths
  (ties logging to multi-hearth attribution). A multi-command scenario
  proves the harness captures every record without deadlock.

  Scenario: Two hearths log two different resolved hearths (AC3)
    Given two hearth directories X and Y each with the standard structure
    When the catalog RPC is called for hearth "X"
    And the catalog RPC is called for hearth "Y"
    Then the engine stderr contains two catalog log records with different hearth fields

  Scenario: Concurrent command RPCs are all captured without deadlock (AC6)
    Given two hearth directories X and Y each with the standard structure
    When concurrent snapshot and complete are issued to the same hearth
    Then the engine stderr contains a JSON log record with fields:
      | command | snapshot |
      | outcome | ok       |
    And the engine stderr contains a JSON log record with fields:
      | command | complete |
      | outcome | ok       |
