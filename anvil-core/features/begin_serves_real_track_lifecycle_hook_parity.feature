Feature: Begin serves the real on-disk track_lifecycle hook body at every (state, role) seam (FD-4)
  FD-4 is the injection-parity proof-gate that authorizes the hard-retire of the
  forge skill bodies for track skill_content_to_hooks. The synthetic fixtures in
  begin_track_lifecycle_migrated_hooks.feature prove the begin LOGIC serves a
  registry-declared hook, but they seed a hand-typed marker string rather than the
  real file. This feature closes that gap: it drives `begin` against the REAL
  `track_seed()` (via SeedPlaybookRegistry) with the REAL hook body read from
  `playbooks/track_lifecycle/hooks/<file>`, and asserts the served context IS that
  migrated hook body — proven by each hook's distinctive HOOK-MARKER line.

  Together with the (spec, doer) and (spec_review, reviewer) parity already proven
  in begin_create_serves_hook.feature and begin_review_serves_hook.feature, this
  feature covers 100% of the (state, role) pairs the track machine declares a hook
  for: every declared seam has a begin-serves-the-real-hook assertion.

  Scenario Outline: begin (doer) serves the real migrated doer-hook body for <state>
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    And the in-memory query adapter loads the real track_lifecycle hook body for filename "<hook_file>"
    When begin is called via query adapter with the real track seed for identifier "<track>" session_role "doer"
    Then the begin outcome is successful
    And the begin outcome result state is "<state>"
    And the begin outcome result context_text contains "<marker>"

    Examples:
      | track                              | state               | artifact_file       | hook_file        | marker                                       |
      | 20260616T0900_spec                 | spec                | spec.md             | spec-writing.md  | HOOK-MARKER: TRACK-LIFECYCLE-SPEC-WRITING    |
      | 20260616T0901_spec_revision        | spec_revision       | spec.md             | spec-revision.md | HOOK-MARKER: TRACK-LIFECYCLE-SPEC-REVISION   |
      | 20260616T0902_plan                 | plan                | spec.md             | plan-writing.md  | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-WRITING    |
      | 20260616T0903_plan_revision        | plan_revision       | plan.md             | plan-writing.md  | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-WRITING    |
      | 20260616T0904_implementing         | implementing        | plan.md             | implementing.md  | HOOK-MARKER: TRACK-LIFECYCLE-IMPLEMENTING    |
      | 20260616T0905_impl_revision        | impl_revision       | plan.md             | implementing.md  | HOOK-MARKER: TRACK-LIFECYCLE-IMPLEMENTING    |
      | 20260616T0906_reflecting           | reflecting          | impl.review.md      | reflecting.md    | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTING      |
      | 20260616T0907_reflection_revision  | reflection_revision | reflection.md       | reflecting.md    | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTING      |
      | 20260616T0908_amend                | amend               | spec.md             | amend-writing.md | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-WRITING   |
      | 20260616T0909_amend_revision       | amend_revision      | spec.amendments.md  | amend-writing.md | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-WRITING   |

  Scenario: begin (complete) serves the real migrated complete-hook body on reflection_review
    Given an in-memory query adapter with track "20260616T0910_complete" in state "reflection_review" and artifact file "reflection.review.md" content "ARTIFACT-BODY"
    And the in-memory query adapter loads the real track_lifecycle hook body for filename "complete.md"
    When begin is called via query adapter with the real track seed for identifier "20260616T0910_complete" session_role "complete"
    Then the begin outcome is successful
    And the begin outcome result state is "reflection_review"
    And the begin outcome result context_text contains "HOOK-MARKER: TRACK-LIFECYCLE-COMPLETE"

  Scenario Outline: begin (reviewer) serves the real migrated reviewer-hook body for <state>
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    And the in-memory query adapter loads the real track_lifecycle hook body for filename "<hook_file>"
    When begin is called via query adapter with the real track seed for identifier "<track>" session_role "reviewer"
    Then the begin outcome is successful
    And the begin outcome result state is "<state>"
    And the begin outcome result review_context_text contains "<marker>"

    Examples:
      | track                             | state              | artifact_file       | hook_file            | marker                                            |
      | 20260616T0920_spec_review         | spec_review        | spec.md             | spec-review.md       | HOOK-MARKER: TRACK-LIFECYCLE-SPEC-REVIEW          |
      | 20260616T0921_plan_review         | plan_review        | plan.md             | plan-review.md       | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-REVIEW          |
      | 20260616T0922_impl_phase_review   | impl_phase_review  | plan.md             | impl-phase-review.md | HOOK-MARKER: TRACK-LIFECYCLE-IMPL-PHASE-REVIEW    |
      | 20260616T0923_impl_review         | impl_review        | plan.md             | impl-review.md       | HOOK-MARKER: TRACK-LIFECYCLE-IMPL-REVIEW          |
      | 20260616T0924_reflection_review   | reflection_review  | reflection.md       | reflection-review.md | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTION-REVIEW    |
      | 20260616T0925_amend_review        | amend_review       | spec.amendments.md  | amend-review.md      | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-REVIEW         |
