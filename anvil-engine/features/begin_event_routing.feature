Feature: Begin RPC event routing pipeline
  The engine's begin RPC routes domain events to the correct subsystem:
  TrackCreation events are routed via Amendment 2 hybrid routing —
  ArtifactPort::scaffold_track_directory creates the directory, status.yaml,
  and placeholder spec.md, then SnapshotCommandHandler::execute appends the
  first transition, creates the registry entry, and inserts the execution
  projection row;
  ReviewDocCreated events go to ArtifactPort::create_review_doc.

  This feature asserts on filesystem-mediated consequences at the engine seam,
  giving diagnostic value beyond the RPC-response assertions in begin_spec_review_rpc.feature
  and begin_error_taxonomy.feature. When a routing regression occurs, these scenarios
  localise the fault to the routing layer rather than surfacing only as an E2E failure.

  Note: ReviewTransition events are NOT emitted by the begin RPC in the post-Slice-A world.
  The spec→spec_review transition is now driven by the complete() call, not begin().
  The create-flow emits TrackCreation (routed via ArtifactPort scaffold + SnapshotCommandHandler),
  and the review-flow emits only ReviewDocCreated (routed via ArtifactPort::create_review_doc).
  See plan.amendments.md Amendment 1 and Amendment 2.

  Scenario: TrackCreation event routes via scaffold and snapshot — filesystem state reflects new track
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When the begin RPC is called to create a track named "event routing test track" under parent "20260411T2021_anvil_workflow_engine"
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path file "status.yaml" contains "state: spec"
    And the hearth file "tracks.md" contains "event routing test track"
    And the hearth file "projections/execution.md" contains "event routing test track"

  Scenario: ReviewDocCreated event routes to artifact port — spec.review.md created on disk
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-14T00:00:00Z
      last_updated: 2026-04-14T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |

      ## Spec (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer"
    Then the begin RPC response has non-empty "review_doc_path"
    And the begin RPC response review_doc_path file contains "# Review:"
    And the begin RPC response review_doc_path file contains "## Round 1"

  Scenario: BeginMarkerWritten event routes to the activity write adapter — begin-marker written to status.yaml on disk
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-14T00:00:00Z
      last_updated: 2026-04-14T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |

      ## Spec (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer" and actor_name "Reviewer-998877"
    Then the begin RPC response state is "spec_review"
    And the begin RPC response track_path file "status.yaml" contains "activity:"
    And the begin RPC response track_path file "status.yaml" contains "kind: begin"
    And the begin RPC response track_path file "status.yaml" contains "actor: Reviewer-998877"
    And the begin RPC response track_path file "status.yaml" contains "state: spec_review"

  Scenario: a begin carrying a conversation_id records it on the durable open-begin marker (resume-aware routing)
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260414T0405_review_spec_strand/           | spec_review  |
    And the track "20260414T0405_review_spec_strand" has spec.md with content "# Review Spec Strand\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [review spec strand](tracks/20260414T0405_review_spec_strand/) — review spec strand
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-04-14T00:00:00Z
      last_updated: 2026-04-14T00:00:00Z
      after_event: "seed"
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | review spec strand | anvil-playbook-engine |

      ## Spec (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Combined review protocol and spec criteria."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260414T0405_review_spec_strand" and session_role "reviewer" and actor_name "Reviewer-998877" and conversation_id "Conv-42"
    Then the begin RPC response state is "spec_review"
    And the begin RPC response track_path file "status.yaml" contains "kind: begin"
    And the begin RPC response track_path file "status.yaml" contains "conversation_id: \"Conv-42\""
