Feature: Snapshot state-to-section registry routing
  For every (kind, state) pair in spec §4's table the snapshot handler
  routes the registry write to the consolidated destination section.
  Because `registry_section_for` is a pure function, we exercise it
  through the handler with the TestSnapshotAdapter as the seam — the
  recorded section must match the table regardless of what existing
  fine-grained section currently hosts the entry.

  Scenario: Track spec → spec
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1 |
      | to_state      | spec      |
      | actor_name    | Actor-123 |
      | actor_role    | spec      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "t1" in "tracks.md" to section "spec"

  Scenario: Track spec_review routes to consolidated spec section
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | spec_review |
      | actor_name    | Actor-123   |
      | actor_role    | review      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "spec"

  Scenario: Track spec_revision routes to consolidated spec section
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec_review"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1     |
      | to_state      | spec_revision |
      | actor_name    | Actor-123     |
      | actor_role    | spec          |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "spec"

  Scenario: Track plan → plan
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "spec_review"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1 |
      | to_state      | plan      |
      | actor_name    | Actor-123 |
      | actor_role    | plan      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "plan"

  Scenario: Track implementing → implementing
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "plan_review"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1    |
      | to_state      | implementing |
      | actor_name    | Actor-123    |
      | actor_role    | implement    |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "implementing"

  Scenario: Track impl_phase_review routes to implementing (consolidated)
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "implementing"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1         |
      | to_state      | impl_phase_review |
      | actor_name    | Actor-123         |
      | actor_role    | review            |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "implementing"

  Scenario: Track impl_review routes to implementing (consolidated)
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "implementing"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1   |
      | to_state      | impl_review |
      | actor_name    | Actor-123   |
      | actor_role    | review      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "implementing"

  Scenario: Track reflection_review routes to reflecting (consolidated)
    Given a snapshot adapter
    And the snapshot adapter has artifact "tracks/t1" of kind "track" in state "reflecting"
    And the snapshot adapter has existing registry entry for "t1" in "tracks.md"
    When snapshot is executed with:
      | artifact_path | tracks/t1         |
      | to_state      | reflection_review |
      | actor_name    | Actor-123         |
      | actor_role    | review            |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "t1" in "tracks.md" to section "reflecting"

  Scenario: Proposal vision_review routes to consolidated vision section
    Given a snapshot adapter
    And the snapshot adapter has artifact "proposals/p1" of kind "proposal" in state "vision"
    And the snapshot adapter has existing registry entry for "p1" in "proposals.md"
    When snapshot is executed with:
      | artifact_path | proposals/p1  |
      | to_state      | vision_review |
      | actor_name    | Actor-123     |
      | actor_role    | review        |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "p1" in "proposals.md" to section "vision"

  Scenario: Proposal vision → vision
    Given a snapshot adapter
    And the snapshot adapter has artifact "proposals/p1" of kind "proposal" in state "vision"
    And the snapshot adapter has existing registry entry for "p1" in "proposals.md"
    When snapshot is executed with:
      | artifact_path | proposals/p1 |
      | to_state      | vision       |
      | actor_name    | Actor-123    |
      | actor_role    | envision     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "p1" in "proposals.md" to section "vision"

  Scenario: Proposal active → active
    Given a snapshot adapter
    And the snapshot adapter has artifact "proposals/p1" of kind "proposal" in state "proposal_review"
    And the snapshot adapter has existing registry entry for "p1" in "proposals.md"
    When snapshot is executed with:
      | artifact_path | proposals/p1 |
      | to_state      | active       |
      | actor_name    | Actor-123    |
      | actor_role    | activate     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "p1" in "proposals.md" to section "active"

  Scenario: Milestone draft → draft
    Given a snapshot adapter
    And the snapshot adapter has artifact "milestones/m1" of kind "milestone" in state "draft"
    And the snapshot adapter has existing registry entry for "m1" in "milestones.md"
    When snapshot is executed with:
      | artifact_path | milestones/m1 |
      | to_state      | draft         |
      | actor_name    | Actor-123     |
      | actor_role    | milestone     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "m1" in "milestones.md" to section "draft"

  Scenario: Initiative draft → draft
    Given a snapshot adapter
    And the snapshot adapter has artifact "initiatives/i1" of kind "initiative" in state "draft"
    And the snapshot adapter has existing registry entry for "i1" in "initiatives.md"
    When snapshot is executed with:
      | artifact_path | initiatives/i1 |
      | to_state      | draft          |
      | actor_name    | Actor-123      |
      | actor_role    | initiative     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "i1" in "initiatives.md" to section "draft"

  Scenario: Decision tension → tension
    Given a snapshot adapter
    And the snapshot adapter has artifact "decisions/d1" of kind "decision" in state "tension"
    And the snapshot adapter has existing registry entry for "d1" in "decisions.md"
    When snapshot is executed with:
      | artifact_path | decisions/d1 |
      | to_state      | tension      |
      | actor_name    | Actor-123    |
      | actor_role    | decide       |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "d1" in "decisions.md" to section "tension"

  Scenario: Decision decided → decided
    Given a snapshot adapter
    And the snapshot adapter has artifact "decisions/d1" of kind "decision" in state "decision_review"
    And the snapshot adapter has existing registry entry for "d1" in "decisions.md"
    When snapshot is executed with:
      | artifact_path | decisions/d1 |
      | to_state      | decided      |
      | actor_name    | Actor-123    |
      | actor_role    | decide       |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "d1" in "decisions.md" to section "decided"

  Scenario: Learning observation → observation
    Given a snapshot adapter
    And the snapshot adapter has artifact "learnings/l1" of kind "learning" in state "observation"
    And the snapshot adapter has existing registry entry for "l1" in "learnings.md"
    When snapshot is executed with:
      | artifact_path | learnings/l1 |
      | to_state      | observation  |
      | actor_name    | Actor-123    |
      | actor_role    | learn        |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot adapter moved registry entry "l1" in "learnings.md" to section "observation"
