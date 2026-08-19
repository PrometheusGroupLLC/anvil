Feature: Snapshot filesystem learning transitions skip projection
  Learning artifact transitions update status and registry but do not
  touch any projection file — learnings have no projection per spec §4.

  Scenario: Learning observation → observation_review records transition + registry, no projection
    Given a snapshot fs hearth with:
      | path                                                  | content                                                                                                                                                                                                                              |
      | learnings/learning-x/status.yaml                      | version: 1\nkind: learning\nstate: observation\nactors: {}\ntransitions: []\n                                                                                                                                                          |
      | learnings/learning-x/definition.md                    | # Learning X                                                                                                                                                                                                                        |
      | learnings.md                                          | # Learnings\n\n## observation\n\n- [Learning X](learnings/learning-x/) — learning x\n                                                                                                                                                  |
    When snapshot fs is executed with:
      | artifact_path        | learnings/learning-x     |
      | to_state             | observation_review       |
      | actor_name           | Reviewer-444444          |
      | actor_role           | review                   |
      | actor_type           | agent                    |
      | actor_model          | claude-opus-4-7          |
      | actor_provider       | anthropic                |
      | actor_context_window | 1000000                  |
      | actor_sdk_version    | 0.2.111                  |
      | actor_entrypoint     | claude-desktop           |
      | at                   | 2026-04-17T06:00:00Z     |
    Then the snapshot result is successful
    And the snapshot result projections_updated is empty
    And the resolved state of "learnings/learning-x" is "observation_review"
