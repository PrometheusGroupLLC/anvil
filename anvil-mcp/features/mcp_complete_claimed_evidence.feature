Feature: MCP complete tool presents claimed evidence (T-ACT-2 MCP leg)
  The MCP `complete` tool exposes an optional `claimed_evidence` argument — an
  ordered array of `{class, reference}` objects — that threads real evidence
  claims into the engine's CompleteRequest, mirroring the CLI's
  `--claimed-evidence` affordance covered by
  `anvil_hooks_complete_claimed_evidence.feature`. Against the live
  `track_lifecycle` seed obligation (T-ACT-1), the resulting durable row on
  `<hearth>/step-measurement.jsonl` records the tri-state assessment. This is
  record mode: no transition is refused regardless of the claim.

  # AC3 (schema leg) — the property exists and is optional
  Scenario: The complete tool schema advertises claimed_evidence as an optional property
    Given the MCP shim is started
    And the MCP session is initialized
    When a tools/list request is sent
    Then the "complete" tool has schema property "claimed_evidence"
    And the "complete" tool does not require field "claimed_evidence"

  # AC3 (behavior + order leg), AC5, AC8 — a tools/call supplying claims
  # populates CompleteRequest.claimed_evidence in the same order, verified via
  # the resulting hearth measurement record (same technique as the CLI leg).
  Scenario: A spec doer completion via MCP presenting the obligated class records present-as-claimed in order
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260721T2200_mcp_ce_present/               | spec   |
    And the track "20260721T2200_mcp_ce_present" has spec.md with content "# MCP Claimed Evidence Present\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [MCP Claimed Evidence Present](tracks/20260721T2200_mcp_ce_present/) — mcp claimed evidence present — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-21T22:00:00Z
      last_updated: 2026-07-21T22:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent for "tracks/20260721T2200_mcp_ce_present" as doer presenting claims:
      | class                   | reference                    |
      | artifact_of_consequence | commit:mcpdeadbeef1          |
      | verifiable_citation     | anvil-mcp/src/main.rs:705    |
    Then the complete response new_state is "spec_review"
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row lists claims in order:
      | class                   | reference                    |
      | artifact_of_consequence | commit:mcpdeadbeef1          |
      | verifiable_citation     | anvil-mcp/src/main.rs:705    |
    And the emitted evidence row names missing classes ""
    And the emitted evidence row carries the track_lifecycle seed content version

  # AC4, AC7 — omission is wire-level legacy-identical (empty claimed_evidence
  # array reaches the engine exactly as it did before this affordance shipped)
  # and the transition still succeeds against the live obligation (record mode).
  Scenario: A spec doer completion via MCP presenting no claims is legacy-identical and still advances
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260721T2201_mcp_ce_absent/                | spec   |
    And the track "20260721T2201_mcp_ce_absent" has spec.md with content "# MCP Claimed Evidence Absent\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [MCP Claimed Evidence Absent](tracks/20260721T2201_mcp_ce_absent/) — mcp claimed evidence absent — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-21T22:00:00Z
      last_updated: 2026-07-21T22:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent for "tracks/20260721T2201_mcp_ce_absent" as doer presenting no claims
    Then the complete response new_state is "spec_review"
    And the emitted evidence row records status "absent"
    And the emitted evidence row names missing classes "artifact_of_consequence"
    And the emitted evidence row carries the track_lifecycle seed content version

  # AC11 (MCP leg) — the opaque reference round-trips verbatim through the
  # MCP wire; raw note text never lands in the durable measurement stream.
  Scenario: A completion via MCP carrying a raw note keeps the claim reference opaque
    Given a hearth directory with the following structure:
      | path                                              | state |
      | proposals/20260411T2021_anvil_workflow_engine/     | active |
      | tracks/20260721T2202_mcp_ce_opaque/                | spec   |
    And the track "20260721T2202_mcp_ce_opaque" has spec.md with content "# MCP Claimed Evidence Opaque\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [MCP Claimed Evidence Opaque](tracks/20260721T2202_mcp_ce_opaque/) — mcp claimed evidence opaque — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-21T22:00:00Z
      last_updated: 2026-07-21T22:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent for "tracks/20260721T2202_mcp_ce_opaque" as doer with note "RAW-MCP-NOTE-must-not-leak" presenting claims:
      | class                   | reference          |
      | artifact_of_consequence | commit:mcp0ff1ce   |
    Then the complete response new_state is "spec_review"
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row reference contains "commit:mcp0ff1ce"
    And no emitted step measurement row contains the text "RAW-MCP-NOTE-must-not-leak"

  # AC13 (MCP leg), AC2 — the affordance being reachable does not manufacture
  # an assessment where no obligation exists (reviewer/spec_review is not an
  # obligated pair per T-ACT-1's track_lifecycle table).
  Scenario: A reviewer completion via MCP on a non-obligated step emits no evidence keys
    Given a hearth directory with the following structure:
      | path                                              | state       |
      | proposals/20260411T2021_anvil_workflow_engine/     | active      |
      | tracks/20260721T2203_mcp_ce_silent/                | spec_review |
    And the track "20260721T2203_mcp_ce_silent" has spec.md with content "# MCP Claimed Evidence Silent\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [MCP Claimed Evidence Silent](tracks/20260721T2203_mcp_ce_silent/) — mcp claimed evidence silent — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan

      ## implementing
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-21T22:00:00Z
      last_updated: 2026-07-21T22:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (1)

      ## Planned (0)

      ## Implementing (0)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a complete tools/call is sent for "tracks/20260721T2203_mcp_ce_silent" as reviewer satisfied presenting no claims
    Then the complete response new_state is "plan"
    And no emitted step measurement row carries any evidence key
