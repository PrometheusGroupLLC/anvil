Feature: Learning conclusion-review → established transitions
  Learnings transition from `conclusion_review` to `established` via the
  natural state-to-section mapping — no conditional routing on the note.
  This feature verifies the happy path; note-annotated transitions carry
  the note into the status.yaml transition but do not alter registry
  routing.

  Scenario: conclusion_review → established moves entry to ## established
    Given a snapshot adapter
    And the snapshot adapter has artifact "learnings/l1" of kind "learning" in state "conclusion_review"
    And the snapshot adapter has existing registry entry for "l1" in "learnings.md"
    When snapshot is executed with:
      | artifact_path  | learnings/l1    |
      | to_state       | established     |
      | actor_name     | Actor-123       |
      | actor_role     | learn           |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "l1" in "learnings.md" to section "established"

  Scenario: direct-experience establishment note is carried into the transition
    Given a snapshot adapter
    And the snapshot adapter has artifact "learnings/l1" of kind "learning" in state "conclusion_review"
    And the snapshot adapter has existing registry entry for "l1" in "learnings.md"
    When snapshot is executed with:
      | artifact_path  | learnings/l1                    |
      | to_state       | established                     |
      | actor_name     | Actor-123                       |
      | actor_role     | learn                           |
      | note           | direct-experience establishment |
      | actor_type     | agent                           |
      | actor_model    | claude-opus-4-7                 |
      | actor_provider | anthropic                       |
    Then the snapshot result is successful
    And the snapshot adapter transition note is "direct-experience establishment"
