Feature: Begin-adoption soft-warn detection (BP2)
  complete/snapshot detect a missing open begin-marker for the
  (actor, artifact, state) key and attach a begin-adoption warning to
  `warnings` while still recording the transition. Detection is a pure
  read over status.yaml `activity:` + `transitions:`. The warning is
  soft (non-blocking) — the transition still records. Scope is the
  driven register (track/playbook/milestone); free types never warn.

  # AC-2: matching open begin → no warning
  Scenario: complete with matching open begin-marker returns no warning
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0001_open_begin" kind "track" state "spec_review" with activity:
      | kind  | actor           | state       | at                   |
      | begin | Reviewer-200001 | spec_review | 2026-06-04T09:00:00Z |
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260604T0001_open_begin |
      | actor_name     | Reviewer-200001                 |
      | actor_type     | agent                           |
      | actor_model    | claude-opus-4-7                 |
      | actor_provider | anthropic                       |
      | satisfaction   | satisfied                       |
      | at             | 2026-06-04T10:00:00Z            |
    Then the complete outcome is successful
    And the complete outcome has 0 warnings

  # AC-3: no open begin → transition still recorded + warning
  Scenario: complete without begin-marker still transitions and warns
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0002_no_begin" kind "track" state "spec_review" with activity:
      | kind  | actor           | state | at                   |
      | begin | Reviewer-200099 | plan  | 2026-06-04T08:00:00Z |
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260604T0002_no_begin |
      | actor_name     | Reviewer-200002               |
      | actor_type     | agent                         |
      | actor_model    | claude-opus-4-7               |
      | actor_provider | anthropic                     |
      | satisfaction   | satisfied                     |
      | at             | 2026-06-04T10:00:00Z          |
    Then the complete outcome is successful
    And the complete outcome event 1 is TransitionRecorded
    And the complete outcome warnings contain "begin_adoption: actor Reviewer-200002 transitioned tracks/20260604T0002_no_begin in state spec_review without a prior begin"

  # AC-6a: creating actor exempt
  Scenario: creating actor's first complete is exempt
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0003_creator" kind "track" state "spec" with transitions:
      | to   | actor       | at                   | role |
      | spec | Doer-200003 | 2026-06-04T07:00:00Z | spec |
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260604T0003_creator |
      | actor_name     | Doer-200003                  |
      | actor_type     | agent                        |
      | actor_model    | claude-opus-4-7              |
      | actor_provider | anthropic                    |
      | at             | 2026-06-04T10:00:00Z         |
    Then the complete outcome is successful
    And the complete outcome has 0 warnings

  # AC-6b: a different actor completing a freshly-created artifact is warned
  Scenario: a different actor completing a freshly-created artifact is warned
    Given an in-memory query adapter seeded with artifact "tracks/20260604T0004_other" kind "track" state "spec" with transitions:
      | to   | actor       | at                   | role |
      | spec | Doer-200004 | 2026-06-04T07:00:00Z | spec |
    When complete is called via query adapter with:
      | artifact_path  | tracks/20260604T0004_other |
      | actor_name     | Doer-299999                |
      | actor_type     | agent                      |
      | actor_model    | claude-opus-4-7            |
      | actor_provider | anthropic                  |
      | at             | 2026-06-04T10:00:00Z       |
    Then the complete outcome is successful
    And the complete outcome warnings contain "begin_adoption: actor Doer-299999 transitioned tracks/20260604T0004_other in state spec without a prior begin"

  # AC-7 / F-1: free-type artifact (decision) is never warned regardless of begin-marker absence.
  # Decisions are not engine-supported by complete; the snapshot path proves the kind gate (see snapshot scenario below).

  # AC-4: snapshot with no matching open begin-marker warns; transition still recorded
  Scenario: snapshot without begin-marker still transitions and warns
    Given a snapshot fs hearth with:
      | path                                              | content                                                                                                                                                                                                                                                                                                                                              |
      | tracks/20260604T0010_snap_no_begin/status.yaml    | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\ntransitions:\n  - to: spec\n    at: 2026-06-04T07:00:00Z\n    actor: Doer-300010\n    role: spec\n |
      | tracks/20260604T0010_snap_no_begin/spec.md        | # Snap Track\n\nExample body.                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                         | # Tracks\n\n## spec\n\n- [Snap Track](tracks/20260604T0010_snap_no_begin/) — snap track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n                                                                                                                                                          |
      | projections/execution.md                          | ---\nincremental_count: 0\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Plan (0)\n                                                                                                                                                                                                                                       |
    When snapshot fs is executed with:
      | artifact_path        | tracks/20260604T0010_snap_no_begin |
      | to_state             | spec_review                        |
      | actor_name           | Reviewer-300011                    |
      | actor_role           | review                             |
      | actor_type           | agent                              |
      | actor_model          | claude-opus-4-7                    |
      | actor_provider       | anthropic                          |
      | actor_context_window | 1000000                            |
      | actor_sdk_version    | 0.2.111                            |
      | actor_entrypoint     | claude-desktop                     |
      | at                   | 2026-06-04T10:00:00Z               |
    Then the snapshot result is successful
    And the snapshot result status_updated is "true"
    And the snapshot result warnings contain "begin_adoption: actor Reviewer-300011 transitioned tracks/20260604T0010_snap_no_begin in state spec without a prior begin"
    And the resolved state of "tracks/20260604T0010_snap_no_begin" is "spec_review"

  # AC-2 (snapshot): matching open begin → no warning
  Scenario: snapshot with matching open begin-marker returns no warning
    Given a snapshot fs hearth with:
      | path                                            | content                                                                                                                                                                                                                                                                                                                  |
      | tracks/20260604T0011_snap_open/status.yaml      | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\ntransitions:\n  - to: spec\n    at: 2026-06-04T07:00:00Z\n    actor: Doer-300010\n    role: spec\nactivity:\n  - kind: begin\n    actor: Reviewer-300011\n    state: spec\n    at: 2026-06-04T09:00:00Z\n |
      | tracks/20260604T0011_snap_open/spec.md          | # Snap Track\n\nExample body.                                                                                                                                                                                                                                                                                             |
      | tracks.md                                       | # Tracks\n\n## spec\n\n- [Snap Track](tracks/20260604T0011_snap_open/) — snap track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n                                                                                                                                |
      | projections/execution.md                        | ---\nincremental_count: 0\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (0)\n\n## Plan (0)\n                                                                                                                                                                                                         |
    When snapshot fs is executed with:
      | artifact_path        | tracks/20260604T0011_snap_open |
      | to_state             | spec_review                    |
      | actor_name           | Reviewer-300011                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
      | at                   | 2026-06-04T10:00:00Z           |
    Then the snapshot result is successful
    And the snapshot result has 0 begin-adoption warnings

  # AC-7 / F-1 / F-8: free-type (decision) snapshot completes without a begin-marker and emits NO warning.
  Scenario: free-type artifact completes without begin-marker and emits no warning
    Given a snapshot fs hearth with:
      | path                                          | content                                                                                                                                            |
      | decisions/20260604T0020_free_decision/status.yaml | version: 1\nkind: decision\ntransitions:\n  - to: tension\n    at: 2026-06-04T07:00:00Z\n    actor: Decider-400020\n    role: decide\n |
      | decisions/20260604T0020_free_decision/definition.md | # Free Decision\n\nBody.                                                                                                                       |
      | decisions.md                                  | # Decisions\n\n## Tensions\n\n- [Free Decision](decisions/20260604T0020_free_decision/)\n\n## Resolved\n                                          |
      | projections/decisions.md                      | ---\nincremental_count: 0\n---\n\n# Decisions\n                                                                                                    |
    When snapshot fs is executed with:
      | artifact_path        | decisions/20260604T0020_free_decision |
      | to_state             | decided                               |
      | actor_name           | Decider-400021                        |
      | actor_role           | decide                                |
      | actor_type           | agent                                 |
      | actor_model          | claude-opus-4-7                       |
      | actor_provider       | anthropic                             |
      | actor_context_window | 1000000                               |
      | actor_sdk_version    | 0.2.111                               |
      | actor_entrypoint     | claude-desktop                        |
      | at                   | 2026-06-04T10:00:00Z                  |
    Then the snapshot result is successful
    And the snapshot result has 0 begin-adoption warnings
