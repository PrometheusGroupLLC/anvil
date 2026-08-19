Feature: AC2 + AC3 — doer-complete and reviewer-complete drive a builder gate
  The rewritten builder machine drives through the keystone's complete selector:
  a doer-complete (empty satisfaction) from a working state advances to its
  review gate; a reviewer-complete from the gate selects the satisfaction-matched
  edge (approved → advance, revision_needed → revision). These exercise the
  core CompleteCommandHandler over a real fs hearth holding the builder machine.

  Scenario: AC2 — doer-complete advances gathering to gathering_review
    Given a complete fs hearth with a builder artifact "20260606T0001_wf" in state "gathering"
    When complete fs is executed with:
      | artifact_path        | workflow_generations/20260606T0001_wf |
      | actor_name           | Builder-Doer-000000                   |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-8                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 1000000                               |
      | actor_entrypoint     | claude-code                           |
    Then the complete result is successful
    And the complete result new_state is "gathering_review"

  Scenario: AC3 — reviewer-complete approved advances gathering_review to analyzing
    Given a complete fs hearth with a builder artifact "20260606T0002_wf" in state "gathering_review"
    When complete fs is executed with:
      | artifact_path        | workflow_generations/20260606T0002_wf |
      | actor_name           | Builder-Reviewer-000000               |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-8                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 1000000                               |
      | actor_entrypoint     | claude-code                           |
      | satisfaction         | approved                              |
    Then the complete result is successful
    And the complete result new_state is "analyzing"

  Scenario: AC3 — reviewer-complete revision_needed routes gathering_review to gathering_revision
    Given a complete fs hearth with a builder artifact "20260606T0003_wf" in state "gathering_review"
    When complete fs is executed with:
      | artifact_path        | workflow_generations/20260606T0003_wf |
      | actor_name           | Builder-Reviewer-000000               |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-8                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 1000000                               |
      | actor_entrypoint     | claude-code                           |
      | satisfaction         | revision_needed                       |
    Then the complete result is successful
    And the complete result new_state is "gathering_revision"
