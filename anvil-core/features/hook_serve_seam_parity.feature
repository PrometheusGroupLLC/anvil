Feature: The shared hook-serving seam returns the same capped body begin serves
  Phase 1 of route_response_mirrors_begin extracts a shared hook-serving seam
  (`hook_serve::serve_hook_body`) so begin and route serve the SAME resolved +
  budget-capped, PRE-interpolation hook body for a (kind, initial_state, doer).
  This scenario pins the seam's output against the body begin reads for that same
  (kind, initial_state, doer), so the two can never drift.

  Scenario: the seam serves the same pre-interpolation capped body begin reads
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" with content "DISTINCTIVE-SEAM-BODY {{track_name}}"
    When the shared seam serves the track spec doer hook "spec-writing.md"
    Then the served seam body equals "DISTINCTIVE-SEAM-BODY {{track_name}}"
    And the served seam body retains the literal placeholder "{{track_name}}"
