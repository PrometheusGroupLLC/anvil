Feature: Snapshot create-vs-move registry dispatch
  Per M12 resolution the handler dispatches between `move_registry_entry`
  and `create_registry_entry` by asking the port whether the entry
  already exists. For tracks this always returns true (begin creates the
  entry); for initiative/decision/learning drafts this can return false,
  causing the handler to call `build_registry_entry_text` + `create`.

  Scenario: Existing entry dispatches to move
    Given a snapshot adapter
    And the snapshot adapter has artifact "proposals/p1" of kind "proposal" in state "vision"
    And the snapshot adapter has existing registry entry for "p1" in "proposals.md"
    When snapshot is executed with:
      | artifact_path | proposals/p1  |
      | to_state      | vision_review |
      | actor_name    | Actor-123     |
      | actor_role    | review        |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "p1" in "proposals.md" to section "vision"
    And the snapshot adapter recorded no registry creates

  Scenario: Missing entry dispatches to create
    Given a snapshot adapter
    And the snapshot adapter has artifact "initiatives/i1" of kind "initiative" in state "draft"
    And the snapshot adapter has fixed registry entry text for "initiatives/i1": "- [Some Initiative](initiatives/i1/) — some initiative"
    When snapshot is executed with:
      | artifact_path | initiatives/i1 |
      | to_state      | draft          |
      | actor_name    | Actor-123      |
      | actor_role    | initiative     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot adapter created registry entry in "initiatives.md" under section "draft"
    And the snapshot adapter created registry entry with text containing "Some Initiative"
    And the snapshot adapter recorded no registry moves
