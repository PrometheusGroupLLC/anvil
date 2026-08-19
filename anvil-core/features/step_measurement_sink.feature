Feature: Step measurement durable sink is append-only and redacted
  The FileSystemStepMeasurementAdapter persists one redacted StepMeasurement
  record per completed step to `<hearth>/step-measurement.jsonl`, one JSON
  object per line. A record carries ONLY the allowlisted fields — record kind,
  the from_state/to_state transition labels, the role, the BOOLEANS
  intent_present / expected_output_present, and an ISO-8601 timestamp. It NEVER
  carries the intent/expected_output PROSE, the surface message text, paths,
  raw identities, or token counts. The booleans capture "was a measurement
  declared" without leaking its content. Two additive allowlisted fields ride
  along: `artifact_kind` (the same public kind the routing-activity sink records)
  and `actor_hash` (a salted, non-reversible, truncated SHA-256 of the actor —
  null when no salt is configured, so a raw or unsalted actor never leaks).
  Join-participant records may also carry optional `conversation_hash`,
  `project_label`, and `playbook_run_id`; these are hashed or public labels,
  never raw identities or paths.
  Appends extend the file; reads return
  every recorded record in append order. A missing sink reads as an empty
  record stream (no error).

  Scenario: appending a measured step writes exactly the redacted booleans, no prose
    Given a step measurement hearth
    When a step measurement record is appended with from "spec", to "spec_review", role "doer", intent_present "true", expected_output_present "true", at "2026-06-15T09:00:00Z"
    Then the step measurement sink file contains "\"kind\":\"step_measurement\""
    And the step measurement sink file contains "\"from_state\":\"spec\""
    And the step measurement sink file contains "\"to_state\":\"spec_review\""
    And the step measurement sink file contains "\"role\":\"doer\""
    And the step measurement sink file contains "\"intent_present\":true"
    And the step measurement sink file contains "\"expected_output_present\":true"
    And the step measurement sink file contains "\"at\":\"2026-06-15T09:00:00Z\""
    And the step measurement sink file does not contain "intent\":\""
    And the step measurement sink file does not contain "expected_output\":\""
    And the step measurement sink file does not contain "message"
    And the step measurement sink file does not contain "actor_name"
    And the step measurement sink file does not contain "path"
    And the step measurement sink file contains "\"actor_hash\":null"

  Scenario: appending a measured step may include hashed correlation keys
    Given a step measurement hearth
    When a step measurement record is appended with from "spec", to "spec_review", role "doer", intent_present "true", expected_output_present "true", at "2026-06-15T09:00:00Z", conversation_hash "0123456789abcdef", project_label "sample-project", playbook_run_id "20260615T0900_sample"
    Then the step measurement sink file contains "\"conversation_hash\":\"0123456789abcdef\""
    And the step measurement sink file contains "\"project_label\":\"sample-project\""
    And the step measurement sink file contains "\"playbook_run_id\":\"20260615T0900_sample\""
    And the step measurement sink file does not contain "raw-session-123"
    And the step measurement sink file does not contain "/home/user/Development/sample-project"

  Scenario: an unmeasured step records false booleans without any prose
    Given a step measurement hearth
    When a step measurement record is appended with from "plan", to "implementing", role "doer", intent_present "false", expected_output_present "false", at "2026-06-15T09:01:00Z"
    Then the step measurement sink file contains "\"intent_present\":false"
    And the step measurement sink file contains "\"expected_output_present\":false"

  Scenario: a second append extends the sink rather than replacing it
    Given a step measurement hearth
    When a step measurement record is appended with from "spec", to "spec_review", role "doer", intent_present "true", expected_output_present "true", at "2026-06-15T09:00:00Z"
    And a step measurement record is appended with from "spec_review", to "plan", role "reviewer", intent_present "true", expected_output_present "false", at "2026-06-15T09:05:00Z"
    Then reading the step measurement sink returns 2 records
    And the step measurement records contain a record for to_state "spec_review"
    And the step measurement records contain a record for to_state "plan"

  Scenario: reading a missing sink returns an empty record stream
    Given a step measurement hearth
    When the step measurement sink is read without any append
    Then reading the step measurement sink returns 0 records
