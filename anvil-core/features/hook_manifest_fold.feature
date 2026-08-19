Feature: Hook manifest fold — derive the installable hook set from machines + hard_enforce policy
  The hook-DELIVERY contract a native-hook installer consumes is DERIVED, never
  duplicated. `fold_hook_manifest` folds every registered playbook machine's
  `hooks_by_role` declarations with the kit manifest's `hard_enforce` policy into
  a flat, sorted set of installable hooks. Each hook carries its
  (artifact_kind, state, role), the ACTUAL hook body served for that seam (the
  SAME content `begin` serves, read via the same hook-body port so it can never
  drift), and a gate classification: "hard" when the artifact_kind is listed in
  `hard_enforce`, else "soft". A state that declares no hooks contributes nothing.

  Scenario: a soft (unlisted) playbook's hooks are classified soft with the served body
    Given a hook manifest registry with playbook "track" hooks:
      | state | role     | filename        | body                  |
      | spec  | doer     | spec-writing.md | SPEC-WRITING-BODY     |
      | spec  | reviewer | spec-review.md  | SPEC-REVIEW-BODY      |
    And the hard_enforce policy is ""
    When fold_hook_manifest is computed
    Then the hook manifest has 2 hooks
    And the hook manifest hook 0 is artifact_kind "track" state "spec" role "doer" gate "soft"
    And the hook manifest hook 0 body contains "SPEC-WRITING-BODY"
    And the hook manifest hook 1 is artifact_kind "track" state "spec" role "reviewer" gate "soft"
    And the hook manifest hook 1 body contains "SPEC-REVIEW-BODY"

  Scenario: a artifact_kind listed in hard_enforce has its hooks classified hard
    Given a hook manifest registry with playbook "track" hooks:
      | state | role | filename        | body              |
      | spec  | doer | spec-writing.md | SPEC-WRITING-BODY |
    And the hard_enforce policy is "track"
    When fold_hook_manifest is computed
    Then the hook manifest has 1 hooks
    And the hook manifest hook 0 is artifact_kind "track" state "spec" role "doer" gate "hard"

  Scenario: a hookless state contributes nothing to the manifest
    Given a hook manifest registry with playbook "track" hooks:
      | state | role | filename        | body              |
      | spec  | doer | spec-writing.md | SPEC-WRITING-BODY |
    And the hook manifest registry has a hookless state "plan" on playbook "track"
    And the hard_enforce policy is ""
    When fold_hook_manifest is computed
    Then the hook manifest has 1 hooks
    And the hook manifest hook 0 is artifact_kind "track" state "spec" role "doer" gate "soft"

  Scenario: hooks are sorted by (artifact_kind, state, role) across two playbooks
    Given a hook manifest registry with playbook "track" hooks:
      | state | role | filename        | body              |
      | spec  | doer | spec-writing.md | TRACK-SPEC-BODY   |
    And a hook manifest registry with playbook "decision" hooks:
      | state    | role | filename     | body            |
      | resolved | doer | resolve.md   | DECISION-BODY   |
    And the hard_enforce policy is "decision"
    When fold_hook_manifest is computed
    Then the hook manifest has 2 hooks
    And the hook manifest hook 0 is artifact_kind "decision" state "resolved" role "doer" gate "hard"
    And the hook manifest hook 1 is artifact_kind "track" state "spec" role "doer" gate "soft"
