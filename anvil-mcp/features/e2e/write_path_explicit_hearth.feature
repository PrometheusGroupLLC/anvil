Feature: Write-path per-call hearth resolution honors the explicit arg
  The MCP shim's write tools (complete, snapshot) must resolve their target
  hearth from the explicit per-call `hearth` argument with the SAME
  precedence as the read tools (checkin/describe/begin): explicit arg wins.
  A relative `artifact_path` is, by the tool contract, relative to the
  resolved hearth — it must never be re-derived against the shim's working
  directory and then treated as a hearth that can CONFLICT with the explicit
  arg. That regression wedged a live session: after a mid-session shim reload
  the fresh shim's cwd belonged to one project while the caller targeted
  another via an explicit `hearth` arg, so every write returned
  `ambiguous_hearth` even though the reads on the same explicit arg resolved.

  Scenario: A fresh shim completes a write when the explicit hearth differs from cwd
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a complete tools/call is sent for seam project "beta" with an explicit hearth and a relative artifact path
    Then the complete response new_state is "spec_review"
    And seam project "beta" artifact "20260419T1100_track_beta" is in state "spec_review"

  Scenario: A fresh shim snapshots a write when the explicit hearth differs from cwd
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a snapshot tools/call is sent for seam project "beta" with an explicit hearth and a relative artifact path
    Then seam project "beta" artifact "20260419T1100_track_beta" is in state "spec_review"

  Scenario: The explicit hearth arg wins for a write while a sibling hearth stays untouched
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a snapshot tools/call is sent for seam project "beta" with an explicit hearth and a relative artifact path
    Then seam project "beta" artifact "20260419T1100_track_beta" is in state "spec_review"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"

  Scenario: A genuinely ambiguous write with no explicit hearth still refuses
    Given no .hearth file in the working directory
    And the MCP shim is started in that working directory with a dead engine endpoint
    And the MCP session is initialized
    When a snapshot tools/call for a relative artifact path with no hearth argument is sent
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "ambiguous_hearth"

  # Item 1 (caller-safety regression): a nested cwd inside a project, with NO
  # explicit arg and NO launch default, must still resolve via the relative
  # artifact_path cwd walk-up. The unconditional early-return regressed this to
  # ambiguous_hearth; relative derivation as a fallback restores it.
  Scenario: A nested-cwd write with no explicit hearth resolves via the cwd walk-up
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a nested working directory inside seam project "beta"
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a snapshot tools/call is sent for seam project "beta" with a relative artifact path and no hearth argument
    Then seam project "beta" artifact "20260419T1100_track_beta" is in state "spec_review"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"

  # Item 3: begin_adoption_status is a READ tool that carries a relative
  # artifact_path — it must follow the same precedence (explicit hearth wins
  # over the shim's sibling cwd), not silently read the wrong project.
  Scenario: begin_adoption_status honors the explicit hearth from a sibling cwd
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a begin_adoption_status tools/call is sent for seam project "beta" with an explicit hearth and a relative artifact path
    Then the MCP response is a successful tool result containing "has_open_begin"

  # Item 4: amend is a write tool carrying a relative artifact_path — the
  # explicit hearth must win from a sibling cwd, and the sibling stays untouched.
  Scenario: amend honors the explicit hearth from a sibling cwd
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When an amend tools/call is sent for seam project "beta" with an explicit hearth and a relative artifact path
    Then the amend response op_id is non-empty
    And seam project "beta" artifact "20260419T1100_track_beta" is in state "spec"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"

  # Item 2 (security): artifact containment. A `..`-traversal aimed at the
  # sibling project is refused with invalid_artifact_path before any write.
  Scenario: A parent-escape artifact_path is rejected as invalid_artifact_path
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a snapshot tools/call is sent for seam project "beta" with an explicit hearth and a parent-escape artifact path
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "invalid_artifact_path"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"

  # Item 3 (ordering): path-independent SYNTAX validation runs BEFORE hearth
  # resolution. A `..`-traversal with NO resolvable hearth is refused as
  # invalid_artifact_path, not ambiguous_hearth (which it returned before the
  # syntax check was hoisted ahead of resolution).
  Scenario: A parent-escape artifact_path with no resolvable hearth is invalid_artifact_path
    Given no .hearth file in the working directory
    And the MCP shim is started in that working directory with a dead engine endpoint
    And the MCP session is initialized
    When a snapshot tools/call for a parent-escape artifact path with no hearth argument is sent
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "invalid_artifact_path"

  # Item 2 (security): an absolute artifact_path resolving outside the resolved
  # hearth (into the sibling project) is refused with invalid_artifact_path.
  Scenario: An absolute artifact_path outside the hearth is rejected as invalid_artifact_path
    Given two seam project hearths under one permitted root and a global playbooks hearth with knowledge_lifecycle body "Global playbook body"
    And the canonical hearth-less daemon is started with the global playbooks hearth for the seam projects
    And a .hearth file pointing to that directory
    And the MCP shim is started in that working directory with canonical daemon discovery
    And the MCP session is initialized while the canonical engine remains active
    When a snapshot tools/call is sent for seam project "beta" with an explicit hearth and an absolute artifact path into seam project "alpha"
    Then the MCP response is a JSON-RPC error
    And the MCP response error message contains "invalid_artifact_path"
    And seam project "alpha" artifact "20260419T1100_track_alpha" is in state "spec"
