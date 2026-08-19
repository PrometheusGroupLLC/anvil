Feature: Every command turn is recorded in the universal activity log
  The engine appends ONE redacted record to the durable activity-log sink
  (`activity-log.jsonl`) for EVERY command turn it serves — not just the routed
  slice the routing-activity sink records. A route turn records its resolution
  outcome (single | candidates | no_match), so routed-vs-abstained is measurable;
  a begin records the resolved playbook kind; a catalog records the command with
  no kind. The read/measurement queries are NOT logged — folding the dashboard's
  own reads would pollute the measurement. Each record carries ONLY the Part-3
  allowlisted fields (command, outcome label, artifact_kind, salted actor_hash,
  timestamp) — never the message text, raw identities, or paths.

  Scenario: a no_match route records command route outcome no_match with no kind
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a free "spark" machine
    And the engine is started with that hearth
    When the route RPC is called with message "capture this idea"
    Then the route outcome is "no_match"
    And the activity log sink has 1 records
    And the activity log sink has a record command "route" outcome "no_match" artifact_kind ""

  Scenario: a matching route records command route with a route outcome label
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the route RPC is called with message "run the knowledge lifecycle for this transcript"
    Then the route outcome is "candidates"
    And the activity log sink has 1 records
    And the activity log sink has no record command "begin"

  Scenario: a begin records command begin outcome ok with the resolved playbook kind
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "alpha topic" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the activity log sink has a record command "begin" outcome "ok" artifact_kind "knowledge_lifecycle"

  Scenario: a begin carrying conversation_id records a hashed conversation on activity
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent for conversation "surface-session-begin-activity-001" and project root "/tmp/anvil-correlation-project"
    Then the begin RPC response has non-empty "track_path"
    And the activity log command "begin" carries correlation keys for project root "/tmp/anvil-correlation-project"

  Scenario: a complete records command complete outcome ok with the resolved playbook kind
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the activity log sink has a record command "begin" outcome "ok" artifact_kind "lore_query"
    And the activity log sink has a record command "complete" outcome "ok" artifact_kind "lore_query"

  Scenario: a snapshot records command snapshot with the resolved playbook kind
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the snapshot RPC is called on the begin RPC response artifact to state "completed" with role "doer"
    Then the snapshot RPC response success is "true"
    And the activity log sink has a record command "snapshot" outcome "ok" artifact_kind "lore_query"

  Scenario: snapshot tagging does not add or remove route/begin/complete turns
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the snapshot RPC is called on the begin RPC response artifact to state "completed" with role "doer"
    Then the snapshot RPC response success is "true"
    And the activity log sink has exactly 1 records with command "begin"
    And the activity log sink has exactly 1 records with command "snapshot"
    And the activity log sink has exactly 0 records with command "complete"
    And the activity log sink has exactly 0 records with command "route"

  # MUTATION: append the complete activity record twice. The exact complete count
  # must become 2 and turn this scenario red.
  Scenario: a complete turn is recorded exactly once
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the activity log sink has exactly 1 records with command "begin"
    And the activity log sink has exactly 1 records with command "complete"
    And the activity log sink has exactly 0 records with command "snapshot"
    And the activity log sink has exactly 0 records with command "route"

  # MUTATION: append the route activity record twice for one route RPC. The exact
  # route count must become 2, catching the duplicate production row shape.
  Scenario: a claude-code route turn is recorded exactly once
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the route RPC is called with message "run the knowledge lifecycle for this transcript" and source "claude-code"
    Then the route outcome is "candidates"
    And the activity log sink has exactly 1 records with command "route"
    And the activity log sink has exactly 0 records with command "begin"
    And the activity log sink has exactly 0 records with command "snapshot"
    And the activity log sink has exactly 0 records with command "complete"

  Scenario: a begin records the entered step as to_state with an empty from_state
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the activity log sink has a record command "begin" from_state "" to_state "answering"

  Scenario: a complete records the exact transition from_state and to_state
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    And the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the activity log sink has a record command "complete" from_state "answering" to_state "completed"

  Scenario: a catalog records command catalog with no kind
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the catalog RPC is called
    Then the activity log sink has a record command "catalog" outcome "ok" artifact_kind ""
    And the activity log sink has a record command "catalog" from_state "" to_state ""

  Scenario: read/measurement queries are NOT logged
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the PlaybookActivity RPC is called
    Then the activity log sink has 0 records
