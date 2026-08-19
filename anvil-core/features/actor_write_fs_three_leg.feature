Feature: Actor write three-leg rule on the filesystem
  FileSystemActorWriteAdapter applies the three-leg rule against a real
  status.yaml on disk. Scenarios seed a temp hearth with a status.yaml,
  invoke upsert_actor_configuration, and verify the file contents.

  Scenario: add-if-absent — fresh actors table gains a new entry
    Given an actor write fs hearth with status.yaml at "tracks/t1":
      """
      version: 1
      kind: track
      state: spec
      proposal: 20260411T2021_anvil_workflow_engine
      actors:
        Existing-100000:
          type: agent
          configurations:
            - at: "2026-04-17T00:00:00Z"
              model: claude-opus-4-6
              provider: anthropic
              details:
                context_window: 1000000
                sdk_version: ""
                entrypoint: claude-code
      transitions:
      """
    When upsert_actor_configuration on fs is called for "tracks/t1" with name "Sarigue-938624", model "claude-opus-4-7", provider "anthropic"
    Then the fs status.yaml at "tracks/t1" contains "  Sarigue-938624:"
    And the fs status.yaml at "tracks/t1" contains "        model: claude-opus-4-7"
    And the fs status.yaml at "tracks/t1" still contains "  Existing-100000:"

  Scenario: match-no-op — re-upsert with identical params leaves the file byte-identical
    Given an actor write fs hearth with status.yaml at "tracks/t1":
      """
      version: 1
      kind: track
      state: spec
      proposal: 20260411T2021_anvil_workflow_engine
      actors:
        Sarigue-938624:
          type: agent
          configurations:
            - at: "2026-04-17T00:00:00Z"
              model: claude-opus-4-7
              provider: anthropic
              details:
                context_window: 1000000
                sdk_version: ""
                entrypoint: claude-code
      transitions:
      """
    When upsert_actor_configuration on fs is called for "tracks/t1" with name "Sarigue-938624", model "claude-opus-4-7", provider "anthropic"
    Then the fs status.yaml at "tracks/t1" contains exactly one occurrence of "model: claude-opus-4-7"

  Scenario: mismatch-append — differing params add a new configuration entry under the existing actor
    Given an actor write fs hearth with status.yaml at "tracks/t1":
      """
      version: 1
      kind: track
      state: spec
      proposal: 20260411T2021_anvil_workflow_engine
      actors:
        Sarigue-938624:
          type: agent
          configurations:
            - at: "2026-04-17T00:00:00Z"
              model: claude-opus-4-7
              provider: anthropic
              details:
                context_window: 1000000
                sdk_version: ""
                entrypoint: claude-code
      transitions:
      """
    When upsert_actor_configuration on fs is called for "tracks/t1" with name "Sarigue-938624", model "claude-sonnet-4-6", provider "anthropic"
    Then the fs status.yaml at "tracks/t1" contains "        model: claude-opus-4-7"
    And the fs status.yaml at "tracks/t1" contains "        model: claude-sonnet-4-6"
    And the fs status.yaml at "tracks/t1" contains exactly one occurrence of "  Sarigue-938624:"
