Feature: Structured review-verdict capture at review-gate transitions
  When a playbook instance leaves a review-gate state (the reviewer's complete
  with a satisfaction), the engine captures one structured review-verdict record
  to a dedicated durable sink (review-verdict.jsonl), keyed by the same salted
  conversation_hash / playbook_run_id the other measurement sinks use. The
  verdict carries the light structure decided in the success-rubric model: the
  gate + honest satisfaction + a final/E2E-gate flag; at the final gate it also
  carries the holistic "intent well accomplished" confidence field. Non-review
  transitions capture nothing.

  Scenario: leaving a final review gate captures a verdict with the confidence field
    Given a hearth seeded with the review_probe playbook
    And the engine is started with that hearth
    When the begin RPC is called to create a "review_probe" artifact named "verdict probe" with no parent for conversation "surface-session-verdict-001" and project root "/tmp/anvil-verdict-project"
    Then the begin RPC response state is "active"
    And the hearth review-verdict sink has exactly 0 records
    When the complete RPC is called on the begin RPC response artifact with satisfaction "" and project root "/tmp/anvil-verdict-project"
    Then the complete RPC response new_state is "review"
    And the hearth review-verdict sink has exactly 0 records
    When the complete RPC is called on the begin RPC response artifact with satisfaction "satisfied" and project root "/tmp/anvil-verdict-project"
    Then the complete RPC response new_state is "completed"
    And the hearth review-verdict sink has exactly 1 records
    And the hearth review-verdict sink has exactly 1 record for artifact_kind "review_probe" gate_state "review" satisfaction "satisfied" final_gate "true"
    And the hearth review-verdict sink record for gate_state "review" carries the intent confidence field
    And the hearth review-verdict sink record for gate_state "review" carries correlation keys for project root "/tmp/anvil-verdict-project"
    And the hearth review-verdict sink record for gate_state "review" carries a non-empty playbook_version
    And the "review-verdict.jsonl" sink does not contain raw text "surface-session-verdict-001"
    And the "review-verdict.jsonl" sink does not contain raw text "/tmp/anvil-verdict-project"
