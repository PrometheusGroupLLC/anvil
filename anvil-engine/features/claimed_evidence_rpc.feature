Feature: Lifecycle RPCs carry claimed evidence
  Lifecycle clients can attach ordered evidence claims to state-entry requests
  without changing established behavior when no claims are supplied.

  Background:
    Given a hearth directory with the following structure:
      | path                                             | state  |
      | proposals/20260411T2021_anvil_workflow_engine/  | active |
      | tracks/20260719T1800_claimed_evidence_rpc_track/ | spec   |
    And the track "20260719T1800_claimed_evidence_rpc_track" has spec.md with content "# Claimed Evidence RPC\n\nP1 carrier fixture."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Claimed Evidence RPC](tracks/20260719T1800_claimed_evidence_rpc_track/) — P1 carrier fixture — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-19T18:00:00Z
      last_updated: 2026-07-19T18:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (1)

      ## Spec Review (0)
      """
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Write the P1 fixture spec."
    And the engine is started with that hearth

  Scenario: Complete carries ordered claimed evidence
    Given a complete lifecycle request with claimed evidence:
      | class                   | reference                  |
      | verifiable_citation     | citation:review-log:42     |
      | artifact_of_consequence | artifact:test-run:run-9173 |
    And the complete lifecycle request contains recognizable raw path and user text
    When the complete lifecycle request crosses the engine boundary
    Then the complete request carries the claimed evidence in order
    And each claim contains only its class and opaque reference
    And the mapped claims contain neither the raw path nor the raw user text
    And the durable step measurement contains no raw project root, user note, or artifact path

  Scenario: Complete without claimed evidence remains legacy-identical
    Given a legacy complete lifecycle request without claimed evidence
    When the complete lifecycle request crosses the engine boundary
    Then the legacy complete payload bytes are unchanged
    And the complete request carries no claimed evidence
    And the successful completion behavior is unchanged

  Scenario: Begin state entry carries ordered claimed evidence
    Given a begin state-entry request with claimed evidence:
      | class                   | reference                    |
      | artifact_of_consequence | artifact:spec-draft:sha-1234 |
      | self_description        | attestation:author:5678       |
    When the begin state-entry request crosses the engine boundary
    Then the begin request carries the claimed evidence in order
    And each claim contains only its class and opaque reference

  Scenario: Snapshot state entry carries claims while projection events do not assess them
    Given a snapshot state-entry request with claimed evidence:
      | class               | reference                 |
      | verifiable_citation | citation:status-event:901 |
      | self_description    | attestation:actor:2345    |
    When the snapshot state-entry request crosses the engine boundary
    Then the snapshot request carries the claimed evidence in order
    And each claim contains only its class and opaque reference
    And a projection-only snapshot does not assess or persist claimed evidence
