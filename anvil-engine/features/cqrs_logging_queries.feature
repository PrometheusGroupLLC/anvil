Feature: Query RPCs emit a lighter structured log record
  Query RPCs (catalog, describe, checkin) log the resolved hearth, command
  name, and outcome. catalog/describe carry no actor and emit no events, so
  those fields are omitted (not fabricated). checkin carries an actor field.

  Scenario: Catalog query logs hearth + command + outcome, no actor, no events
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260403T1500_forge_lifecycle/           | active       |
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the catalog response contains 1 active artifacts
    And the engine stderr contains a JSON log record with fields:
      | command | catalog     |
      | hearth  | <non-empty> |
      | outcome | ok          |
    And the engine stderr "catalog" log record has no "actor" field
    And the engine stderr "catalog" log record has no "events" field

  Scenario: Describe query logs hearth + command + outcome, no actor, no events
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the describe RPC is called with identifier "track"
    Then the describe RPC returns type info with name "track"
    And the engine stderr contains a JSON log record with fields:
      | command | describe    |
      | hearth  | <non-empty> |
      | outcome | ok          |
    And the engine stderr "describe" log record has no "actor" field
    And the engine stderr "describe" log record has no "events" field

  Scenario: Checkin query logs hearth + command + outcome + actor, no events
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And the engine is started with that hearth
    When the checkin RPC is called with role "creator" and actor_name "TestActor-000042"
    Then the checkin RPC response actor_name is "TestActor-000042"
    And the engine stderr contains a JSON log record with fields:
      | command | checkin          |
      | hearth  | <non-empty>      |
      | actor   | TestActor-000042 |
      | outcome | ok               |
    And the engine stderr "checkin" log record has no "events" field
