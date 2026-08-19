Feature: MCP per-call hearth resolution
  The MCP shim resolves the target hearth for every tool call so one
  long-lived MCP process can safely operate on more than one project.

  Scenario: One MCP process routes explicit project calls to separate hearths
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a catalog tools/call is sent for seam project "alpha"
    Then the MCP catalog result includes artifact "20260419T1100_track_alpha"
    And the MCP catalog result does not include "20260419T1100_track_beta"
    When a catalog tools/call is sent for seam project "beta"
    Then the MCP catalog result includes artifact "20260419T1100_track_beta"
    And the MCP catalog result does not include "20260419T1100_track_alpha"

  Scenario: Explicit hearth overrides a prior hearth in the same MCP process
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a catalog tools/call is sent for seam hearth "alpha"
    Then the MCP catalog result includes artifact "20260419T1100_track_alpha"
    When a catalog tools/call is sent for seam hearth "beta"
    Then the MCP catalog result includes artifact "20260419T1100_track_beta"
    And the MCP catalog result does not include "20260419T1100_track_alpha"

  Scenario: Snapshot derives the hearth from the artifact path after a different-hearth call
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a catalog tools/call is sent for seam project "alpha"
    And a snapshot tools/call is sent for seam project "beta" using its artifact path
    Then seam project "beta" artifact "20260419T1100_track_beta" is in state "spec_review"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"

  Scenario: A no-signal call refuses with a typed ambiguous hearth error
    Given no .hearth file in the working directory
    And the MCP shim is started in that working directory with a dead engine endpoint
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "ambiguous_hearth"

  Scenario: Single-project sessions still use the launch default
    Given a hearth directory with the following structure:
      | path                                             | state |
      | tracks/20260403T1500_forge_lifecycle/            | spec  |
    And the track "20260403T1500_forge_lifecycle" has spec.md with content "# Forge lifecycle spec"
    And the hearth tracks.md is seeded with:
      """
      # Tracks

      ## spec
      - [Forge](tracks/20260403T1500_forge_lifecycle/)
      """
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory
    And the MCP session is initialized
    When a tools/call request is sent for "catalog"
    Then the MCP catalog result includes artifact "20260403T1500_forge_lifecycle"
