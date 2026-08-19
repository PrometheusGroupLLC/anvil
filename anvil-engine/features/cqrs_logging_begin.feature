Feature: Begin command RPC emits a structured CQRS log record
  A begin call that creates a track emits one JSON log record carrying the
  actor, resolved hearth, command name, the real emitted event variant
  name(s), and an ok outcome.

  Scenario: Begin create-track logs a JSON command-outcome record with event names
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "cqrs log begin track" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the engine stderr contains a JSON log record with fields:
      | command | begin           |
      | actor   | Rpc-Test-000000 |
      | hearth  | <non-empty>     |
      | events  | TrackCreation   |
      | outcome | ok              |
