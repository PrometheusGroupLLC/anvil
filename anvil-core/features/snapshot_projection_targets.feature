Feature: Snapshot projection target routing
  Per spec §4 the handler routes projection updates by artifact kind
  and mode: tracks update execution.md, proposals and milestones update
  intent.md, decisions rebuild decisions.md, learnings update no
  projection, and projection-only spark events update sparks.md.
  `*_revision` transitions skip projection updates.

  Scenario: Track transition updates execution.md only
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
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "execution.md"
    And the snapshot adapter recorded an execution row move to "Spec Review"

  Scenario: Proposal transition updates intent.md only (consolidated label)
    Given a snapshot adapter
    And the snapshot adapter has artifact "proposals/p1" of kind "proposal" in state "vision_review"
    And the snapshot adapter has existing registry entry for "p1" in "proposals.md"
    When snapshot is executed with:
      | artifact_path | proposals/p1 |
      | to_state      | proposal     |
      | actor_name    | Actor-123    |
      | actor_role    | propose      |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "intent.md"
    And the snapshot adapter recorded an intent row move to "Draft"

  Scenario: Milestone transition updates intent.md only
    Given a snapshot adapter
    And the snapshot adapter has artifact "milestones/m1" of kind "milestone" in state "draft_review"
    And the snapshot adapter has existing registry entry for "m1" in "milestones.md"
    When snapshot is executed with:
      | artifact_path | milestones/m1 |
      | to_state      | active        |
      | actor_name    | Actor-123     |
      | actor_role    | milestone     |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "intent.md"
    And the snapshot adapter recorded an intent row move to "Active"

  Scenario: Decision transition rebuilds decisions.md
    Given a snapshot adapter
    And the snapshot adapter has artifact "decisions/d1" of kind "decision" in state "tension_review"
    And the snapshot adapter has existing registry entry for "d1" in "decisions.md"
    When snapshot is executed with:
      | artifact_path | decisions/d1 |
      | to_state      | investigating |
      | actor_name    | Actor-123     |
      | actor_role    | decide        |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "decisions.md"
    And the snapshot adapter recorded 1 decisions rebuilds

  Scenario: Learning transition updates no projection
    Given a snapshot adapter
    And the snapshot adapter has artifact "learnings/l1" of kind "learning" in state "observation"
    And the snapshot adapter has existing registry entry for "l1" in "learnings.md"
    When snapshot is executed with:
      | artifact_path | learnings/l1       |
      | to_state      | observation_review |
      | actor_name    | Actor-123          |
      | actor_role    | review             |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result projections_updated is empty

  Scenario: Track revision-mode transition skips projection
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
    Then the snapshot result is successful
    And the snapshot result projections_updated is empty

  Scenario: Playbook generation transition updates the authoring projection
    Given a snapshot adapter
    And the snapshot adapter has artifact "workflow_generations/wg1" of kind "playbook_generation" in state "gathering"
    And the snapshot adapter has existing registry entry for "wg1" in "workflow_generations.md"
    When snapshot is executed with:
      | artifact_path | workflow_generations/wg1 |
      | to_state      | gathering_review         |
      | actor_name    | Actor-123                |
      | actor_role    | gather                   |
      | actor_type     | agent                    |
      | actor_model    | claude-opus-4-7          |
      | actor_provider | anthropic                |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "authoring.md"
    And the snapshot adapter recorded an authoring projection for "workflow_generations/wg1" phase "Gathering Review"

  Scenario: A generated kind that declares projection_targets folds a per-artifact projection.md
    Given a snapshot adapter
    And the snapshot adapter has artifact "tax_preps/tp1" of kind "tax_prep" in state "intake"
    And the snapshot adapter has existing registry entry for "tp1" in "tax_preps.md"
    And the snapshot adapter has state "reviewing" declaring projection_targets for "tax_preps/tp1"
    When snapshot is executed with:
      | artifact_path | tax_preps/tp1   |
      | to_state      | reviewing       |
      | actor_name    | Actor-123       |
      | actor_role    | prepare         |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-7 |
      | actor_provider | anthropic       |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "projection.md"
    And the snapshot adapter recorded an artifact projection for "tax_preps/tp1" phase "Reviewing"
