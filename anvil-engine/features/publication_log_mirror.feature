Feature: Domain events mirror to the Crucible publication log
  When the engine runs on a real data dir (here: with
  $FOUNDRY_PUBLICATION_LOG_DIR set to a temp dir), every dispatched domain
  event is teed, best-effort, into the standard append-only
  events-YYYY-MM-DD.jsonl publication log as a canonical envelope
  (eventId, kind, timestamp, data) — without touching the authoritative writes.

  Scenario: A begin create-track is mirrored as a canonical publication-log envelope
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth and a publication log
    When the begin RPC is called to create a track named "publog mirror track" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the publication log contains an event with kind "ArtifactCreation"
