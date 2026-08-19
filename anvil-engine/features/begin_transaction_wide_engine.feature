Feature: begin holds one hearth guard across its whole transaction (engine seam)
  begin acquires its hearth lock once at the top — before the initial read — and
  reuses that single guard across all event-arm writes (spec Req 5 / F3, plan
  N2-a). Because the per-hearth mutex is non-reentrant, the lock-bearing arms
  (ReviewTransition / TrackCreation / PlaybookCreation) must NOT re-lock; a
  re-lock would self-deadlock. This scenario drives a TrackCreation begin (a
  lock-bearing arm) end-to-end and asserts it COMPLETES and produces the
  expected on-disk state — i.e. no self-deadlock, one guard held across read and
  all writes.

  Scenario: A track-creation begin completes under a single transaction-wide guard
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "lock wide track" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path file "status.yaml" contains "state: spec"
    And the hearth file "tracks.md" contains "lock wide track"
    And the hearth file "projections/execution.md" contains "lock wide track"
