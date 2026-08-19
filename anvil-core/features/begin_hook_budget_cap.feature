Feature: Served hook content is capped to a deterministic budget (T5)
  resolve_and_read_hook serves the machine-declared hook body through
  context_text. To stop hook serving from re-bloating the context window the
  moment it works, the served body is capped at a single deterministic byte
  budget (HOOK_CONTEXT_BUDGET_BYTES). When the body exceeds the budget the
  engine truncates it deterministically at a UTF-8 char boundary and appends a
  clear truncation marker; an under-budget body is served verbatim.

  # AC: an under-budget hook is served verbatim, with no truncation marker.
  Scenario: under-budget hook body is served verbatim
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" with content "SHORT-VERBATIM-BODY"
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine" and a registry declaring spec doer hook "spec-writing.md"
    Then the begin outcome is successful
    And the begin outcome result context_text contains "SHORT-VERBATIM-BODY"
    And the begin outcome result context_text does not contain "[hook content truncated"

  # AC: an over-budget hook is capped to the budget and marked truncated.
  Scenario: over-budget hook body is capped and marked truncated
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" that is over the hook context budget
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine" and a registry declaring spec doer hook "spec-writing.md"
    Then the begin outcome is successful
    And the begin outcome result context_text contains "[hook content truncated"
    And the begin outcome result context_text is within the hook context budget
