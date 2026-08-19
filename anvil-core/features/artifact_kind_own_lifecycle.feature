Feature: Playbook kind own lifecycle transitions
  The `playbook` artifact kind transitions through its own states via `snapshot`.
  The engine accepts the `playbook` kind and routes it to the correct registry
  sections per Phase 2's `registry_section_for` mapping.
  Scope note: this track uses `snapshot` only (not `complete`) per plan M4/R3.3.

  Scenario: Playbook draft → draft_review routes to draft section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "draft"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | draft_review                          |
      | actor_name    | Doer-111111                           |
      | actor_role    | doer                                  |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "draft"

  Scenario: Playbook draft_review → draft_revision routes to draft section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "draft_review"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | draft_revision                        |
      | actor_name    | Reviewer-222222                       |
      | actor_role    | reviewer                              |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "draft"

  Scenario: Playbook draft_review → active routes to active section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "draft_review"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | active                                |
      | actor_name    | Reviewer-222222                       |
      | actor_role    | reviewer                              |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "active"

  Scenario: Playbook active → amend routes to active section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "active"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | amend                                 |
      | actor_name    | Doer-111111                           |
      | actor_role    | doer                                  |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "active"

  Scenario: Playbook amend → amend_review routes to active section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "amend"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | amend_review                          |
      | actor_name    | Doer-111111                           |
      | actor_role    | doer                                  |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "active"

  Scenario: Playbook amend_review → active routes to active section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "amend_review"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | active                                |
      | actor_name    | Reviewer-222222                       |
      | actor_role    | reviewer                              |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "active"

  Scenario: Playbook active → reflecting routes to reflecting section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "active"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | reflecting                            |
      | actor_name    | Doer-111111                           |
      | actor_role    | doer                                  |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "reflecting"

  Scenario: Playbook reflecting → reflection_review routes to reflecting section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "reflecting"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | reflection_review                     |
      | actor_name    | Doer-111111                           |
      | actor_role    | doer                                  |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "reflecting"

  Scenario: Playbook reflection_review → retired routes to retired section
    Given a snapshot adapter
    And the snapshot adapter has artifact "playbooks/20260420T1000_test_playbook" of kind "playbook" in state "reflection_review"
    And the snapshot adapter has existing registry entry for "20260420T1000_test_playbook" in "playbooks.md"
    When snapshot is executed with:
      | artifact_path | playbooks/20260420T1000_test_playbook |
      | to_state      | retired                               |
      | actor_name    | Reviewer-222222                       |
      | actor_role    | reviewer                              |
      | actor_type    | agent                                 |
      | actor_model   | claude-sonnet-4-6                     |
      | actor_provider | anthropic                            |
    Then the snapshot result is successful
    And the snapshot adapter moved registry entry "20260420T1000_test_playbook" in "playbooks.md" to section "retired"
