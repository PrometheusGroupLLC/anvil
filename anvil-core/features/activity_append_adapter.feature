Feature: Activity append adapter fidelity
  The FileSystemActivityWriteAdapter appends a begin-marker to the artifact's
  status.yaml `activity:` block via a textual line-level edit — it does NOT
  re-serialize FullStatusYaml. The pre-existing `actors:` and `transitions:`
  sections must be preserved byte-for-byte across an append, and a second
  append must extend the same `activity:` list rather than create a new one
  (BP1, F-7).

  Scenario: append_activity on a status.yaml with no activity key creates the list and preserves actors and transitions
    Given an activity write fs hearth with status.yaml at "tracks/20260414T0405_review_spec_strand":
      """
      version: 1
      kind: track
      state: spec_review
      proposal: 20260411T2021_anvil_workflow_engine
      actors:
        Reviewer-123456:
          type: agent
          configurations:
            - at: "2026-06-04T09:00:00Z"
              model: opus
              provider: anthropic
              details:
                context_window: 1000000
                sdk_version: "1.0"
                entrypoint: claude-code
      transitions:
        - to: spec
          at: "2026-06-04T09:00:00Z"
          actor: Doer-000001
          role: spec
        - to: spec_review
          at: "2026-06-04T09:30:00Z"
          actor: Doer-000001
          role: doer
      """
    When append_activity on fs is called for "tracks/20260414T0405_review_spec_strand" with actor "Reviewer-123456", state "spec_review", kind "begin", at "2026-06-04T10:00:00Z"
    Then the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "activity:"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "kind: begin"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "actor: Reviewer-123456"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "state: spec_review"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "at: \"2026-06-04T10:00:00Z\""
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" still contains "Reviewer-123456:"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" still contains "model: opus"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" still contains "actor: Doer-000001"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" still contains "to: spec_review"

  Scenario: a second append_activity extends the existing activity list
    Given an activity write fs hearth with status.yaml at "tracks/20260414T0405_review_spec_strand":
      """
      version: 1
      kind: track
      state: spec_review
      transitions:
        - to: spec_review
          at: "2026-06-04T09:30:00Z"
          actor: Doer-000001
          role: doer
      """
    When append_activity on fs is called for "tracks/20260414T0405_review_spec_strand" with actor "Reviewer-111111", state "spec_review", kind "begin", at "2026-06-04T10:00:00Z"
    And append_activity on fs is called for "tracks/20260414T0405_review_spec_strand" with actor "Reviewer-222222", state "spec_review", kind "begin", at "2026-06-04T10:05:00Z"
    Then the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "actor: Reviewer-111111"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains "actor: Reviewer-222222"
    And the fs activity status.yaml at "tracks/20260414T0405_review_spec_strand" contains exactly one occurrence of "activity:"
