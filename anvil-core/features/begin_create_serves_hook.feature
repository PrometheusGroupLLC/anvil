Feature: Begin create flow serves the (spec, doer) hook into context_text
  Per AC-1/AC-5 of the hook_content_serving track, `begin(artifact_type="track")`
  resolves the `track` machine's `spec` state `(doer)` hook via the
  `PlaybookRegistry`, reads its body via `QueryPort::read_playbook_hook_body`,
  and serves that body verbatim through `context_text`. The hook declaration
  lives on the playbook machine (registry), not hardcoded in the handler.

  Per AC-2, when the `spec` state declares no `(doer)` hook, `context_text` is
  empty and the begin call still succeeds — the absence of a declared hook is
  not an error.

  Scenario: begin create serves the declared (spec, doer) hook body verbatim
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" with content "DISTINCTIVE-SPEC-WRITING-HOOK-BODY"
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine" and a registry declaring spec doer hook "spec-writing.md"
    Then the begin outcome is successful
    And the begin outcome result context_text contains "DISTINCTIVE-SPEC-WRITING-HOOK-BODY"

  Scenario: begin create leaves context_text empty when no (spec, doer) hook is declared
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with parent "20260411T2021_anvil_workflow_engine" and a registry declaring no spec doer hook
    Then the begin outcome is successful
    And the begin outcome result context_text is empty
