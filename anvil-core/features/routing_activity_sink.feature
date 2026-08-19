Feature: Routing activity durable sink is append-only and redacted
  The FileSystemRoutingActivityAdapter persists one redacted RoutingActivity
  record per route resolution to `<hearth>/routing-activity.jsonl`, one JSON
  object per line. A record carries ONLY the allowlisted fields — playbook
  kind, resolver outcome, an ISO-8601 timestamp, and optional correlation
  fields (`conversation_hash`, `project_label`). It NEVER carries the
  surface message text, paths, identities, or any other raw input. Appends
  extend the file; reads return every recorded record in append order. A
  missing sink reads as an empty record stream (no error).

  Scenario: appending a record creates the sink with exactly the redacted fields
    Given a routing activity hearth
    When a routing activity record is appended with kind "lore_query", outcome "single", at "2026-06-15T09:00:00Z"
    Then the routing activity sink file contains "\"kind\":\"lore_query\""
    And the routing activity sink file contains "\"outcome\":\"single\""
    And the routing activity sink file contains "\"at\":\"2026-06-15T09:00:00Z\""
    And the routing activity sink file does not contain "message"
    And the routing activity sink file does not contain "input"
    And the routing activity sink file does not contain "actor"

  Scenario: appending a record may include hashed correlation keys without raw inputs
    Given a routing activity hearth
    When a routing activity record is appended with kind "lore_query", outcome "single", at "2026-06-15T09:00:00Z", conversation_hash "0123456789abcdef", project_label "sample-project"
    Then the routing activity sink file contains "\"conversation_hash\":\"0123456789abcdef\""
    And the routing activity sink file contains "\"project_label\":\"sample-project\""
    And the routing activity sink file does not contain "raw-session-123"
    And the routing activity sink file does not contain "/home/user/Development/sample-project"

  Scenario: a second append extends the sink rather than replacing it
    Given a routing activity hearth
    When a routing activity record is appended with kind "lore_query", outcome "single", at "2026-06-15T09:00:00Z"
    And a routing activity record is appended with kind "lore_digest", outcome "single", at "2026-06-15T09:05:00Z"
    Then reading the routing activity sink returns 2 records
    And the routing activity records contain a record for kind "lore_query"
    And the routing activity records contain a record for kind "lore_digest"

  Scenario: reading a missing sink returns an empty record stream
    Given a routing activity hearth
    When the routing activity sink is read without any append
    Then reading the routing activity sink returns 0 records
