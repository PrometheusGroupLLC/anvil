Feature: Complete drives a domain machine's doer advance (AC2)
  CompleteCommandHandler, machine-driven, selects the unique
  None-satisfaction non-reviewer-role outgoing edge for a doer-complete
  (empty satisfaction) and drives it. On a knowledge_lifecycle artifact in
  `ingesting`, doer-complete transitions to `ingest_review` (role `ingest`).
  Guard 2 preserves the track's doer rejection from a review state.

  Scenario: knowledge doer-complete from ingesting advances to ingest_review
    Given a complete fs hearth with a knowledge artifact "20260601T0000_topic" in state "ingesting"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_topic |
      | actor_name     | Ingestor-100001               |
      | actor_type     | agent                         |
      | actor_model    | claude-opus-4-8               |
      | actor_provider | anthropic                     |
      | at             | 2026-06-01T01:00:00Z          |
    Then the complete result is successful
    And the complete result new_state is "ingest_review"
    And the resolved state of "knowledge/20260601T0000_topic" is "ingest_review"
    And a transition event for "knowledge/20260601T0000_topic" contains "role: ingest"

  Scenario: knowledge doer-complete from organizing advances to compiling
    Given a complete fs hearth with a knowledge artifact "20260601T0000_org" in state "organizing"
    When complete fs is executed with:
      | artifact_path  | knowledge/20260601T0000_org |
      | actor_name     | Organizer-100001            |
      | actor_type     | agent                       |
      | actor_model    | claude-opus-4-8             |
      | actor_provider | anthropic                   |
      | at             | 2026-06-01T02:00:00Z        |
    Then the complete result is successful
    And the complete result new_state is "compiling"

  Scenario: Guard 2 — track doer-complete from spec_review is rejected (behavior-preservation)
    Given an in-memory query adapter seeded with artifact "tracks/20260420T1922_guard2" kind "track" state "spec_review"
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260420T1922_guard2 |
      | actor_name     | Doer-100001                 |
      | actor_type     | agent                       |
      | actor_model    | claude-opus-4-8             |
      | actor_provider | anthropic                   |
      | at             | 2026-04-20T19:22:00Z        |
    Then the complete outcome is a CompleteError containing "spec_review"
