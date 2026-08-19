Feature: Adopt an out-of-engine track via the begin RPC
  A track authored outside the engine (a status.yaml on disk with no recorded
  transition history) can be adopted: begin(identifier, adopt: true) resets it
  to the machine's initial state, records the adoption as an appended
  transition, preserves the hand-made files, and drives it through the review
  gates — where the reviewer genuinely evaluates the pre-existing spec.

  Scenario: adopt resets a hand-made track to spec and drives it through the first review gate
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260716T0700_handmade_track/               | implementing |
    And the track "20260716T0700_handmade_track" has spec.md with content "# Handmade Spec\n\nAuthored outside the engine."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      ## plan

      ## implementing

      - [Handmade Track](tracks/20260716T0700_handmade_track/) — handmade track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## reflecting

      ## completed
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-16T00:00:00Z
      last_updated: 2026-07-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (1)

      - [Handmade Track](tracks/20260716T0700_handmade_track/)

      ## Reflecting (0)

      ## Completed (0)
      """
    And a playbook hook body for the track hook "spec-writing.md" with content "ADOPT-SPEC-DOER-HOOK"
    And a playbook hook body for the track hook "spec-review.md" with content "ADOPT-SPEC-REVIEW-HOOK"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260716T0700_handmade_track", session_role "resumer", and adopt "true"
    Then the begin RPC response state is "spec"
    And the begin RPC response "context_text" contains "authored outside the engine"
    And the begin RPC response "context_text" contains "implementing"
    And the begin RPC response "context_text" contains "ADOPT-SPEC-DOER-HOOK"
    And the resolved state of "tracks/20260716T0700_handmade_track" in the hearth is "spec"
    And the hearth file "tracks/20260716T0700_handmade_track/spec.md" contains "Authored outside the engine"
    When the complete RPC is called with:
      | artifact_path  | tracks/20260716T0700_handmade_track |
      | actor_name     | Doer-700001                         |
      | actor_type     | agent                               |
      | actor_model    | test-model                          |
      | actor_provider | test                                |
    Then the complete RPC response new_state is "spec_review"
    When the begin RPC is called with identifier "20260716T0700_handmade_track" and session_role "reviewer"
    Then the begin RPC response state is "spec_review"
    And the begin RPC response "artifact_text" contains "Authored outside the engine"

  Scenario: adoption leaves an OPEN begin and the track is engine-resumable at spec
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260716T0800_handmade_resume/              | implementing |
    And the track "20260716T0800_handmade_resume" has spec.md with content "# Handmade Spec\n\nAuthored outside the engine."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      ## plan

      ## implementing

      - [Handmade Resume](tracks/20260716T0800_handmade_resume/) — handmade resume — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## reflecting

      ## completed
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-16T00:00:00Z
      last_updated: 2026-07-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (1)

      - [Handmade Resume](tracks/20260716T0800_handmade_resume/)

      ## Reflecting (0)

      ## Completed (0)
      """
    And a playbook hook body for the track hook "spec-writing.md" with content "ADOPT-SPEC-DOER-HOOK"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260716T0800_handmade_resume", session_role "resumer", and adopt "true"
    Then the begin RPC response state is "spec"
    # Finding 2/3: the adoption reset does NOT close the adopting actor's begin —
    # its begin remains OPEN (the reset transition carries event_type "adoption"
    # and is skipped by the close-by-comparison predicate).
    When the begin_adoption_status RPC is called with:
      | actor_name    | Adopter-000000                       |
      | artifact_path | tracks/20260716T0800_handmade_resume |
      | state         | spec                                 |
    Then the begin_adoption_status RPC response has_open_begin is "true"
    # Finding 5: an interrupted adoption is engine-resumable from its initial spec
    # state — a plain resumer begin re-serves the spec doer hook (no stranding).
    When the begin RPC is called with identifier "20260716T0800_handmade_resume" and session_role "resumer"
    Then the begin RPC response state is "spec"
    And the begin RPC response "context_text" contains "ADOPT-SPEC-DOER-HOOK"

  Scenario: adopt refuses a track whose only transition evidence is a damaged event file
    # Blocker: the lenient event-store fold silently skips an unreadable or
    # unparseable transition file, so a governed track whose only surviving
    # transition is damaged would look ungoverned and be RESET. Adoption reads
    # the evidence STRICTLY and must fail closed (FAILED_PRECONDITION) instead
    # of clobbering the artifact — its state stays put and no event is written.
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260716T0900_torn_track/                   | implementing |
    And the track "20260716T0900_torn_track" has spec.md with content "# Torn\n\nAuthored outside the engine."
    And the track "20260716T0900_torn_track" has a damaged transition event file
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      ## plan

      ## implementing

      - [Torn Track](tracks/20260716T0900_torn_track/) — torn track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## reflecting

      ## completed
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-07-16T00:00:00Z
      last_updated: 2026-07-16T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (0)

      ## Spec Review (0)

      ## Planned (0)

      ## Implementing (1)

      - [Torn Track](tracks/20260716T0900_torn_track/)

      ## Reflecting (0)

      ## Completed (0)
      """
    And a playbook hook body for the track hook "spec-writing.md" with content "ADOPT-SPEC-DOER-HOOK"
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260716T0900_torn_track", session_role "resumer", and adopt "true"
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "evidence cannot be read cleanly"
    And the resolved state of "tracks/20260716T0900_torn_track" in the hearth is "implementing"
