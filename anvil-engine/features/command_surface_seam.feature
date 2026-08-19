Feature: Every change this engine makes names the surface that asked for it

  The command line, the screens and the tool surface are peers of this engine.
  Two halves of that seam were already accountable. Every command turn already
  writes a structured record naming the command, the actor and how it came out,
  and every read a screen makes over the bridge already names the screen. The
  half that was missing is WHICH PROGRAM asked for a change: begin, snapshot,
  complete and amend all arrive over the same wire from two different programs,
  and the record they wrote could not tell them apart. "It did something I
  didn't ask for" was, at this seam, unanswerable.

  So the record those commands already write now also names the surface, and it
  is written TWICE — once before the change is attempted and once after it is
  known how the change came out, both carrying the same id. A pair is
  deliberate. One record written afterwards disappears entirely when the change
  is interrupted, and an action that left no trace reads exactly like an action
  nobody took. Two records mean an interrupted change is READABLE — asked for,
  never resolved — rather than absent.

  The surface is one of a closed set of names, and there is no default. A
  request that does not name its surface is refused and the change is not made,
  because a default is exactly how an unattributable change comes to look
  attributed, and a name the recorder supplies to itself records nothing about
  who asked. A name this engine does not recognise is refused for the same
  reason: an open field accepts anything and so distinguishes nothing.

  Scenario: A change asked for from the command line names the command line
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" name "seam named track" parent "20260411T2021_anvil_workflow_engine" approver "Nick" against that engine
    Then the anvil-hooks begin command exits 0
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | surface | cli            |
      | phase   | settled        |
      | outcome | ok             |

  Scenario: The per-command record the engine already wrote now names the surface too
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" name "seam legacy track" parent "20260411T2021_anvil_workflow_engine" approver "Nick" against that engine
    Then the anvil-hooks begin command exits 0
    And the engine stderr contains a JSON log record with fields:
      | command | begin         |
      | events  | TrackCreation |
      | outcome | ok            |
      | surface | cli           |

  Scenario: A change is asked for before it is settled, and both are readable
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" name "seam pair track" parent "20260411T2021_anvil_workflow_engine" approver "Nick" against that engine
    Then the anvil-hooks begin command exits 0
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | phase   | issued         |
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | phase   | settled        |
    And the issued and settled command seam records for "begin" carry the same command id

  Scenario: A change that was asked for and refused is still recorded as having been asked for
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" and no parent against that engine
    Then the anvil-hooks begin command exits 1
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | surface | cli            |
      | phase   | issued         |
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | surface | cli            |
      | phase   | settled        |
      | outcome | error          |

  Scenario: A change that names no surface is refused, and the change is not made
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When a begin is sent over gRPC naming no surface
    Then the gRPC call is refused with "surface_required"
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command           |
      | command | begin                    |
      | surface | (unnamed)                |
      | phase   | settled                  |
      | outcome | refused_surface_required |
    And the hearth gained no new track

  Scenario: A surface name this engine does not know is refused rather than recorded
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When a begin is sent over gRPC naming the surface "somewhere-else"
    Then the gRPC call is refused with "surface_unknown"
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command          |
      | command | begin                   |
      | surface | (unrecognised)          |
      | phase   | settled                 |
      | outcome | refused_surface_unknown |
    And the hearth gained no new track

  Scenario: Two surfaces asking for the same change are told apart
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When a begin is sent over gRPC naming the surface "cli"
    And a begin is sent over gRPC naming the surface "mcp"
    Then the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | surface | cli            |
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command |
      | command | begin          |
      | surface | mcp            |

  Scenario: A change the harness itself makes names the harness, and never a shipped surface
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
      | tracks/20260417T1000_rpc_track/                | spec   |
    And the track "20260417T1000_rpc_track" has spec.md with content "# Rpc Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Rpc Track](tracks/20260417T1000_rpc_track/) — rpc track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-16T00:00:00Z
      last_updated: 2026-04-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)
      """
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path        | tracks/20260417T1000_rpc_track |
      | to_state             | spec_review                    |
      | actor_name           | Rpc-Test-111111                |
      | actor_role           | review                         |
      | actor_type           | agent                          |
      | actor_model          | claude-opus-4-7                |
      | actor_provider       | anthropic                      |
      | actor_context_window | 1000000                        |
      | actor_sdk_version    | 0.2.111                        |
      | actor_entrypoint     | claude-desktop                 |
    Then the snapshot RPC response success is "true"
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command  |
      | command | snapshot        |
      | surface | test-harness    |
      | actor   | Rpc-Test-111111 |
      | phase   | settled         |
      | outcome | ok              |

  Scenario: All four commands that change state require a surface, not just begin
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    When a "begin" is sent over gRPC naming no surface
    And a "snapshot" is sent over gRPC naming no surface
    And a "complete" is sent over gRPC naming no surface
    And an "amend" is sent over gRPC naming no surface
    Then 4 gRPC calls were made and every one was refused with "surface_required"
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command           |
      | command | snapshot                 |
      | outcome | refused_surface_required |
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command           |
      | command | complete                 |
      | outcome | refused_surface_required |
    And the engine stderr contains a JSON log record with fields:
      | seam    | engine_command           |
      | command | amend                    |
      | outcome | refused_surface_required |
