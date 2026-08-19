Feature: CandidatePlaybook intake begins a seeded playbook_generation
  Submitting a CandidatePlaybook through the MCP shim begins the real
  playbook_generation builder and stores the candidate seed on the instance.

  Scenario: candidate intake drives the seeded builder to terminal and persists the generated machine
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a separate temp owner-home directory for MCP persist_playbook
    And the engine hearth playbooks entry count is recorded for MCP persist_playbook
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100010           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    Then the complete response new_state is "completed"
    And a machine.yaml exists at "playbooks/review_support_escalations/machine.yaml" under the MCP persist owner-home
    And a fresh registry from the MCP persist owner-home resolves kind "review_support_escalations"
    And the generated candidate playbook kind "review_support_escalations" has measurement for state "triage" role "doer"
    And the engine hearth playbooks entry count is unchanged for MCP persist_playbook

  Scenario: candidate playbook intake accepts the canonical MCP tool name
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a separate temp owner-home directory for MCP persist_playbook
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100110           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    Then the candidate_playbook_intake response has kind "playbook_generation"
    And the candidate_playbook_intake response has playbook_name "review_support_escalations"

  Scenario: retrying the same candidate terminal persist is idempotent
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a separate temp owner-home directory for MCP persist_playbook
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100011           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    And a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:01Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100012           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    Then the complete response new_state is "completed"
    And a machine.yaml exists at "playbooks/review_support_escalations/machine.yaml" under the MCP persist owner-home
    And a fresh registry from the MCP persist owner-home resolves kind "review_support_escalations"

  Scenario: retrying the same candidate terminal complete after local completion write failure is idempotent
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a separate temp owner-home directory for MCP persist_playbook
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100017           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to the terminal-ready state through the MCP shim
    And the intake builder artifact directory is made non-writable
    And the same intake builder terminal complete is attempted through the MCP shim
    Then the complete response is a tool error containing "status.yaml append failed"
    And the intake instance status state is not "completed"
    And a machine.yaml exists at "playbooks/review_support_escalations/machine.yaml" under the MCP persist owner-home
    And the MCP persist owner-home machine.yaml bytes are recorded for kind "review_support_escalations"
    When the intake builder artifact directory writability is restored
    And the same intake builder terminal complete is attempted through the MCP shim
    Then the complete response new_state is "completed"
    And the MCP persist owner-home machine.yaml bytes are unchanged for kind "review_support_escalations"

  Scenario: different candidate content for the same generated kind fails with already exists
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    And a separate temp owner-home directory for MCP persist_playbook
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <mcp_persist_owner_home>      |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100015           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    And a "candidate_playbook_intake" tools/call is sent with:
      | field                    | value                         |
      | source                   | lore                          |
      | intent                   | Review support escalations    |
      | at                       | 2026-06-08T00:00:01Z          |
      | evidence                 | obs-1,obs-2                   |
      | proposed_expected_output | A different triage artifact.  |
      | target_owner             | <mcp_persist_owner_home>      |
      | parent_id                | 20260606T0000_builder_parent_track |
      | actor_name               | Intake-Actor-100016           |
      | actor_type               | agent                         |
      | actor_model              | gpt-5-codex                   |
      | actor_provider           | openai                        |
    And the intake builder is driven to completed through the MCP shim
    Then the complete response is a tool error containing "playbook_duplicate_kind_registration"
    And the intake instance status state is not "completed"

  Scenario: unresolved target_owner descriptor is rejected during terminal persist
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | kit:foo                       |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100013           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    Then the complete response is a tool error containing "unresolved_target_owner"
    And the complete response is a tool error containing "target_owner must be a resolved absolute owner-home path"
    And the intake instance status state is not "completed"
    And no relative playbook directory "kit:foo/playbooks/review_support_escalations" exists under the MCP engine working directory

  Scenario: whitespace-padded target_owner path is rejected during terminal persist
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                         |
      | source         | lore                          |
      | intent         | Review support escalations    |
      | at             | 2026-06-08T00:00:00Z          |
      | evidence       | obs-1,obs-2                   |
      | target_owner   | <whitespace_padded_tmp_owner_home> |
      | parent_id      | 20260606T0000_builder_parent_track |
      | actor_name     | Intake-Actor-100014           |
      | actor_type     | agent                         |
      | actor_model    | gpt-5-codex                   |
      | actor_provider | openai                        |
    And the intake builder is driven to completed through the MCP shim
    Then the complete response is a tool error containing "unresolved_target_owner"
    And the complete response is a tool error containing "target_owner must be a resolved absolute owner-home path"
    And the intake instance status state is not "completed"
    And no relative playbook directory " /tmp/anvil-candidate-owner/playbooks/review_support_escalations" exists under the MCP engine working directory

  Scenario: candidate intake begins and seeds the builder instance
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And the engine is started with that hearth
    And a .hearth file pointing to that directory while the running engine remains active
    And the MCP shim is started in that working directory with the running engine endpoint
    And the MCP session is initialized while the running engine remains active
    When a "candidate_playbook_intake" tools/call is sent with:
      | field          | value                                       |
      | source         | lore                                        |
      | intent         | Review support escalations                  |
      | at             | 2001-01-01T00:00:00Z                        |
      | evidence       | obs-1,obs-2                                 |
      | target_owner   | /tmp/anvil-candidate-owner                  |
      | parent_id      | 20260606T0000_builder_parent_track          |
      | actor_name     | Intake-Actor-100000                         |
      | actor_type     | agent                                       |
      | actor_model    | gpt-5-codex                                 |
      | actor_provider | openai                                      |
    Then the candidate_playbook_intake response has kind "playbook_generation"
    And the candidate_playbook_intake response has playbook_name "review_support_escalations"
    And the hearth status.yaml for the intake instance contains "kind: playbook_generation"
    And the hearth status.yaml for the intake instance contains "target_owner: /tmp/anvil-candidate-owner"
    And the hearth status.yaml for the intake instance contains "playbook_name: review_support_escalations"
    And the generation context for the intake instance contains "Review support escalations"
    And the generation context for the intake instance contains "2001-01-01T00:00:00Z"
    And the generation context for the intake instance contains "triage"
    And the intake step measurement is stamped with the instance transition time and not candidate time "2001-01-01T00:00:00Z"
    And the engine stderr contains a JSON log record with fields:
      | event_kind      | step_measurement                  |
      | track_id        | playbook_generation               |
      | playbook_id     | <non-empty>                       |
      | to_state        | gathering                         |
      | role            | doer                              |
      | intent          | <non-empty>                       |
      | expected_output | <non-empty>                       |

  Scenario: repeated same-intent intake creates distinct builder instances
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When two candidate_playbook_intake tools/calls with the same intent are sent with:
      | field          | value                                       |
      | source         | lore                                        |
      | intent         | Review support escalations                  |
      | first_at       | 2026-06-08T00:00:00Z                        |
      | second_at      | 2026-06-08T00:00:01Z                        |
      | evidence       | obs-1,obs-2                                 |
      | target_owner   | /tmp/anvil-candidate-owner                  |
      | parent_id      | 20260606T0000_builder_parent_track          |
      | actor_name     | Intake-Actor-100001                         |
      | actor_type     | agent                                       |
      | actor_model    | gpt-5-codex                                 |
      | actor_provider | openai                                      |
    Then both candidate_playbook_intake responses have playbook_name "review_support_escalations"
    And the candidate_playbook_intake responses have distinct instance ids
    And both candidate_playbook_intake instances exist under the hearth

  Scenario: proposed state entries must include non-empty required fields
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a candidate_playbook_intake tools/call is sent with proposed state field "expected_output" blank
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "proposed_states[0].expected_output"

  Scenario: candidate source is required as non-empty provenance
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a candidate_playbook_intake tools/call is sent with candidate field "source" "blank"
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "candidate.source"

  Scenario: candidate source must be present
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a candidate_playbook_intake tools/call is sent with candidate field "source" "missing"
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "candidate.source"

  Scenario: candidate emission time is required as non-empty provenance
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a candidate_playbook_intake tools/call is sent with candidate field "at" "blank"
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "candidate.at"

  Scenario: candidate emission time must be present
    Given a hearth seeded with the builder machine and an active parent track "20260606T0000_builder_parent_track"
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a candidate_playbook_intake tools/call is sent with candidate field "at" "missing"
    Then the MCP response is a tool error containing "INVALID_ARGUMENT"
    And the MCP response is a tool error containing "candidate.at"
