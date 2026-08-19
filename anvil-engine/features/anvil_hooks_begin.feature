Feature: anvil-hooks begin begins a playbook through the direct-engine channel
  The begin subcommand lets a harness BEGIN a playbook by calling the engine's
  Begin RPC directly — the same gRPC path the route hook uses, bypassing the
  anvil MCP server. It is NOT broker-independent against a Foundry-mode engine
  (that engine requires a bearer and the broker is what mints one); see
  anvil_hooks_foundry_credential.feature for the credential contract.
  This is the adoption / conversion lever: the route hook always reaches the
  engine, but the anvil MCP tool channel is broker-gated and intermittent, so a
  nudge that can only be acted on via `anvil_orchestrate` is often un-actionable.
  `anvil-hooks begin` (always on PATH, no broker) routes around that.

  Unlike route-turn (advisory, fail-silent, always exit 0), begin is an ACTION
  and reports its outcome: success exits 0 with the scoped first step; a rejection
  the engine answered with exits non-zero carrying the real reason; an unreachable
  engine exits non-zero. This seam proves the real BINARY against a real engine.

  Scenario: begin accepts a track under a seeded parent and returns the scoped first step
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" name "reliable begin track" parent "20260411T2021_anvil_workflow_engine" approver "Nick" against that engine
    Then the anvil-hooks begin command exits 0
    And the anvil-hooks begin output contains "Began `track`"
    And the anvil-hooks begin output contains "state `spec`"

  Scenario: begin surfaces the engine's real rejection reason when required fields are missing
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "track" and no parent against that engine
    Then the anvil-hooks begin command exits 1
    And the anvil-hooks begin output contains "anvil begin rejected"
    And the anvil-hooks begin output contains "missing_required_field"

  # ── The two modes ─────────────────────────────────────────────────────────
  # begin was CREATION-ONLY at this seam even though the wire
  # (BeginRequest.identifier) and the MCP shim have always carried both modes.
  # A harness driving a track therefore had no way to say "re-enter the artifact
  # I already have": calling begin with an existing artifact's NAME minted a
  # SECOND artifact plus a stray registry line. `--identifier` closes that —
  # creation mode (--artifact-type …) and resume mode (--identifier …) are
  # separate, and asking for both at once is refused rather than guessed at.

  Scenario: begin --identifier resumes an existing track and mints no second artifact
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
      | tracks/20260812T0900_hooks_resume/             | plan   |
    And the track "20260812T0900_hooks_resume" has spec.md with content "# Hooks Resume Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## plan

      - [Hooks Resume Track](tracks/20260812T0900_hooks_resume/) — hooks resume track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## implementing

      ## reflecting
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-08-12T00:00:00Z
      last_updated: 2026-08-12T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Planned (1)

      - [Hooks Resume Track](tracks/20260812T0900_hooks_resume/)
      """
    And a playbook hook body for the track hook "plan-writing.md" with content "HOOKS-RESUME: PLAN DOER CONTEXT"
    And the engine is started with that hearth
    When anvil-hooks begin runs with identifier "20260812T0900_hooks_resume" against that engine
    Then the anvil-hooks begin command exits 0
    And the anvil-hooks begin output contains "Resumed `20260812T0900_hooks_resume`"
    And the anvil-hooks begin output contains "state `plan`"
    And the anvil-hooks begin output contains "HOOKS-RESUME: PLAN DOER CONTEXT"
    And the hearth contains exactly 1 artifact directories under "tracks"

  Scenario: begin --identifier is refused when the named artifact does not exist
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with identifier "20260812T0901_no_such_track" against that engine
    Then the anvil-hooks begin command exits 1
    And the anvil-hooks begin output contains "anvil begin rejected"
    And the anvil-hooks begin output contains "not found in the hearth"
    And the hearth contains exactly 0 artifact directories under "tracks"

  # Ambiguity is the defect, so it is refused — NOT resolved by preferring one
  # mode. Before this, --identifier was an unknown flag: the creation flags won
  # silently and a duplicate artifact appeared.
  Scenario: begin refuses --identifier together with the creation-mode flags
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with identifier "20260812T0902_ambiguous" and artifact-type "track" name "ambiguous track" parent "20260411T2021_anvil_workflow_engine" approver "Nick" against that engine
    Then the anvil-hooks begin command exits 1
    And the anvil-hooks begin output contains "anvil begin rejected"
    And the anvil-hooks begin output contains "--identifier"
    And the anvil-hooks begin output contains "--artifact-type"
    And the hearth contains exactly 0 artifact directories under "tracks"

  # --artifact-type stays required in creation mode; it is simply no longer the
  # only way in. With neither mode selected the refusal names BOTH.
  Scenario: begin names both modes when neither --artifact-type nor --identifier is given
    Given a hearth directory with the following structure:
      | path                                           | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And a playbook hook body for the spec doer hook "spec-writing.md" with content "Spec writing guidance."
    And the engine is started with that hearth
    When anvil-hooks begin runs with neither artifact-type nor identifier against that engine
    Then the anvil-hooks begin command exits 1
    And the anvil-hooks begin output contains "anvil begin rejected"
    And the anvil-hooks begin output contains "--artifact-type"
    And the anvil-hooks begin output contains "--identifier"
    And the hearth contains exactly 0 artifact directories under "tracks"

  # The conversion lever: a routed DOMAIN kind (lore_query) with machine-declared
  # required fields begins in ONE call via repeatable `--field k=v` → create_fields.
  # This is what lets the nudge emit a ready-to-run begin one-liner for any kind.
  Scenario: begin satisfies machine-declared required fields via --field in one call
    Given a hearth seeded with a lore_query machine requiring question and requester
    And the engine is started with that hearth
    When anvil-hooks begin runs with artifact-type "lore_query" and fields "question"="what is anvil" "requester"="Nick" against that engine
    Then the anvil-hooks begin command exits 0
    And the anvil-hooks begin output contains "Began `lore_query`"
