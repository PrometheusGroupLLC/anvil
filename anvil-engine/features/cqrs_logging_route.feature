Feature: Route RPC emits a structured CQRS log record
  When a route query runs through the engine, the engine emits one JSON log
  record to stderr in the CQRS command-log shape: the command name, the legacy
  and resolution outcomes, the selected kind (when any), and the granted/relevant
  candidate counts — matched by named fields, not position. The raw user message
  never appears in the command record (it belongs only to the separate
  routing_decision measurement record, which carries no `command` field).

  Scenario: a resolving route logs command, outcome, selected kind, and counts
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "single"
    And the engine stderr contains a JSON log record with fields:
      | command            | route       |
      | outcome            | candidates  |
      | resolution_outcome | single      |
      | selected_kind      | daily_recap |
      | granted_count      | 4           |
      | relevant_count     | 1           |
      | hearth             | <non-empty> |

  Scenario: an abstaining route logs a no_match command record with zero relevant
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "something totally unrelated" and ctx org "Consulting" role "read" clearance "internal"
    Then the route resolution outcome is "no_match"
    And the engine stderr contains a JSON log record with fields:
      | command            | route    |
      | outcome            | no_match |
      | resolution_outcome | no_match |
      | granted_count      | 4        |
      | relevant_count     | 0        |
