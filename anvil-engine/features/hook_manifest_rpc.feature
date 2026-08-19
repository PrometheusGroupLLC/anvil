Feature: HookManifest RPC returns the resolved installable hook set for the hearth
  The read-only HookManifest query backs the harness-agnostic kit-hook DELIVERY
  contract a native-hook installer (and Foundry) consumes. The engine folds every
  registered playbook machine's `hooks_by_role` declarations with the kit
  manifest's `hard_enforce` policy into a flat, sorted hook set. Each hook carries
  its (artifact_kind, state, role), the ACTUAL body served for that seam (the same
  content `begin` serves), and a gate of "hard" (artifact_kind in hard_enforce) or
  "soft". The gRPC RPC and the /ws `hook_manifest` method return identical data.

  Scenario: the gRPC RPC returns each declared hook with its served body and soft gate
    Given a hook manifest engine hearth with playbook "track" hooks:
      | state | role     | filename        | body                        |
      | spec  | doer     | spec-writing.md | DOER-SPEC-WRITING-BODY      |
      | spec  | reviewer | spec-review.md  | REVIEWER-SPEC-REVIEW-BODY   |
    And the engine is started with that hearth
    When the HookManifest RPC is called
    Then the hook manifest RPC gate_query is "begin_adoption_status"
    And the hook manifest RPC has a hook for artifact_kind "track" state "spec" role "doer" with gate "soft"
    And the hook manifest RPC hook for artifact_kind "track" state "spec" role "doer" body contains "DOER-SPEC-WRITING-BODY"
    And the hook manifest RPC has a hook for artifact_kind "track" state "spec" role "reviewer" with gate "soft"

  Scenario: the /ws hook_manifest method returns the identical resolved hook set
    Given a hook manifest engine hearth with playbook "track" hooks:
      | state | role | filename        | body                   |
      | spec  | doer | spec-writing.md | DOER-SPEC-WRITING-BODY |
    And the engine is started with that hearth
    When a hook_manifest JSON-RPC request is sent over /ws with hearth_path ""
    Then the /ws hook_manifest result gate_query is "begin_adoption_status"
    And the /ws hook_manifest result has a hook for artifact_kind "track" state "spec" role "doer" with gate "soft"
    And the /ws hook_manifest result hook for artifact_kind "track" state "spec" role "doer" body contains "DOER-SPEC-WRITING-BODY"
