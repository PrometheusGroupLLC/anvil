Feature: BeginAdoptionStatus RPC (BP3)
  The BeginAdoptionStatus RPC is a pure read — no mutation, no lock write —
  that returns has_open_begin: true when (actor, artifact, state) has an
  open begin-marker, false after the closing transition, and false when the
  actor never began. (AC-5)

  The predicate logic is identical to the soft-warn detection in
  complete/snapshot (BP2), both delegating to
  has_open_begin() in anvil_core::domain::begin_adoption.

  # AC-5 / true after begin, no subsequent transition
  Scenario: BeginAdoptionStatus returns true after reviewer begin with no closing transition
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T1200_status_track/                | spec_review  |
    And the track "20260604T1200_status_track" has spec.md with content "# Status Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Status Track](tracks/20260604T1200_status_track/) — status track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-04T00:00:00Z
      last_updated: 2026-06-04T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | Status Track | anvil-playbook-engine |

      ## Spec (0)

      ## Planned (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Review protocol."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260604T1200_status_track" and session_role "reviewer" and actor_name "Reviewer-700001"
    Then the begin RPC response state is "spec_review"
    And the begin_adoption_status RPC is called with:
      | actor_name    | Reviewer-700001                     |
      | artifact_path | tracks/20260604T1200_status_track   |
      | state         | spec_review                         |
    And the begin_adoption_status RPC response has_open_begin is "true"

  # AC-5 / false after actor's closing complete
  Scenario: BeginAdoptionStatus returns false after the actor's closing complete
    Given a hearth directory with the following structure:
      | path                                              | state        |
      | proposals/20260411T2021_anvil_workflow_engine/     | active       |
      | tracks/20260604T1201_closed_track/                | spec_review  |
    And the track "20260604T1201_closed_track" has spec.md with content "# Closed Track\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      ## spec_review

      - [Closed Track](tracks/20260604T1201_closed_track/) — closed track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## plan
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-04T00:00:00Z
      last_updated: 2026-06-04T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec Review (1)

      | Track | Proposal |
      |-------|----------|
      | Closed Track | anvil-playbook-engine |

      ## Spec (0)

      ## Planned (0)
      """
    And a playbook hook body for the spec_review reviewer hook "spec-review.md" with content "Review protocol."
    And the engine is started with that hearth
    When the begin RPC is called with identifier "20260604T1201_closed_track" and session_role "reviewer" and actor_name "Reviewer-700002"
    And the complete RPC is called with:
      | artifact_path  | tracks/20260604T1201_closed_track |
      | actor_name     | Reviewer-700002                   |
      | actor_type     | agent                             |
      | actor_model    | claude-opus-4-7                   |
      | actor_provider | anthropic                         |
      | satisfaction   | satisfied                         |
    And the begin_adoption_status RPC is called with:
      | actor_name    | Reviewer-700002                     |
      | artifact_path | tracks/20260604T1201_closed_track   |
      | state         | spec_review                         |
    Then the begin_adoption_status RPC response has_open_begin is "false"

  # AC-5 / false when actor never began
  Scenario: BeginAdoptionStatus returns false when the actor never began
    Given a hearth directory with the following structure:
      | path                                              | state   |
      | proposals/20260411T2021_anvil_workflow_engine/     | active  |
      | tracks/20260604T1202_never_began/                 | spec    |
    And the track "20260604T1202_never_began" has spec.md with content "# Never Began\n\nSpec body."
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec

      - [Never Began](tracks/20260604T1202_never_began/) — never began — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)

      ## spec_review

      ## plan
      """
    And the hearth execution.md is seeded with:
      """
      ---
      incremental_count: 0
      base_snapshot: 2026-06-04T00:00:00Z
      last_updated: 2026-06-04T00:00:00Z
      after_event: ""
      ---

      # Anvil — State of Execution

      ## Spec (1)

      | Track | Proposal |
      |-------|----------|
      | Never Began | anvil-workflow-engine |

      ## Spec Review (0)

      ## Planned (0)
      """
    And the engine is started with that hearth
    When the begin_adoption_status RPC is called with:
      | actor_name    | Reviewer-700003                   |
      | artifact_path | tracks/20260604T1202_never_began  |
      | state         | spec                              |
    Then the begin_adoption_status RPC response has_open_begin is "false"
