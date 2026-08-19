Feature: Snapshot filesystem create-vs-move dispatch
  Transitions for kinds with no existing registry entry go through
  `build_registry_entry_text` + `create_registry_entry`. Transitions
  with an existing entry use `move_registry_entry`. Missing target
  section headers are created on demand.

  Scenario: Initiative draft creates a new registry entry
    Given a snapshot fs hearth with:
      | path                                         | content                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | initiatives/new-initiative/status.yaml       | version: 1\nkind: initiative\nstate: draft\nactors: {}\ntransitions: []\n                                                                                                                                                                                                                                                                                                                                                   |
      | initiatives/new-initiative/definition.md     | # New Initiative\n\nBrief summary of the initiative.\n                                                                                                                                                                                                                                                                                                                                                                      |
      | initiatives.md                               | # Initiatives\n                                                                                                                                                                                                                                                                                                                                                                                                             |
    When snapshot fs is executed with:
      | artifact_path        | initiatives/new-initiative |
      | to_state             | draft                      |
      | actor_name           | Author-333333              |
      | actor_role           | initiative                 |
      | actor_type           | agent                      |
      | actor_model          | claude-opus-4-7            |
      | actor_provider       | anthropic                  |
      | actor_context_window | 1000000                    |
      | actor_sdk_version    | 0.2.111                    |
      | actor_entrypoint     | claude-desktop             |
      | at                   | 2026-04-17T05:00:00Z       |
    Then the snapshot result is successful
    And the file "initiatives.md" contains "## draft"
    And the file "initiatives.md" contains "New Initiative"
    And the file "initiatives.md" contains "initiatives/new-initiative"
