Feature: Complete drives a satisfaction-discriminated review gate (AC3)
  CompleteCommandHandler, machine-driven, selects the outgoing edge whose
  required_satisfaction set contains the supplied satisfaction. The
  satisfaction vocabulary is the MACHINE's (approved / revision_needed /
  rejected), not forge's. An unrecognized value is a typed error. Guard 1
  keeps the track's "satisfied"-compat fallback gated to the track encoding.

  Scenario: reviewer approves ingest_review — advances to organizing
    Given a complete fs hearth with a knowledge artifact "20260601T0000_rev" in state "ingest_review"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_rev |
      | actor_name     | Reviewer-100001             |
      | actor_type     | agent                       |
      | actor_model    | claude-opus-4-8             |
      | actor_provider | anthropic                   |
      | satisfaction   | approved                    |
      | at             | 2026-06-01T03:00:00Z        |
    Then the complete result is successful
    And the complete result new_state is "organizing"
    And the resolved state of "knowledge/20260601T0000_rev" is "organizing"

  Scenario: reviewer requests revision on ingest_review — returns to ingesting
    Given a complete fs hearth with a knowledge artifact "20260601T0000_revback" in state "ingest_review"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_revback |
      | actor_name     | Reviewer-100002                 |
      | actor_type     | agent                           |
      | actor_model    | claude-opus-4-8                 |
      | actor_provider | anthropic                       |
      | satisfaction   | revision_needed                 |
      | at             | 2026-06-01T03:30:00Z            |
    Then the complete result is successful
    And the complete result new_state is "ingesting"

  Scenario: reviewer rejects ingest_review — moves to rejected
    Given a complete fs hearth with a knowledge artifact "20260601T0000_rej" in state "ingest_review"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_rej |
      | actor_name     | Reviewer-100003             |
      | actor_type     | agent                       |
      | actor_model    | claude-opus-4-8             |
      | actor_provider | anthropic                   |
      | satisfaction   | rejected                    |
      | at             | 2026-06-01T03:45:00Z        |
    Then the complete result is successful
    And the complete result new_state is "rejected"

  Scenario: an unrecognized satisfaction value is a typed error
    Given a complete fs hearth with a knowledge artifact "20260601T0000_bad" in state "ingest_review"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_bad |
      | actor_name     | Reviewer-100004             |
      | actor_type     | agent                       |
      | actor_model    | claude-opus-4-8             |
      | actor_provider | anthropic                   |
      | satisfaction   | satisfied                   |
      | at             | 2026-06-01T03:50:00Z        |
    Then the complete result is a CompleteError containing "satisfaction_unknown"
