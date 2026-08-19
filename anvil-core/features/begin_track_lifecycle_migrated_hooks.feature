Feature: Begin serves migrated track lifecycle hooks
  Track C2 migrates forge skill body guidance into track_lifecycle hooks.
  The begin seam must serve each migrated state-role hook body from the
  playbook machine so the manual skill body is no longer required for that
  phase.

  Scenario Outline: begin on a track doer phase serves the migrated hook body
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "<hook_file>" with content "<marker>"
    When begin is called via query adapter with identifier "<track>" session_role "<role>" and a registry declaring state "<state>" role "<role>" hook "<hook_file>"
    Then the begin outcome is successful
    And the begin outcome result state is "<state>"
    And the begin outcome result context_text contains "<marker>"

    Examples:
      | track                              | state               | role     | artifact_file          | hook_file           | marker                                           |
      | 20260613T1800_plan                 | plan                | doer     | spec.md                | plan-writing.md     | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-WRITING       |
      | 20260613T1801_plan_revision        | plan_revision       | doer     | plan.md                | plan-writing.md     | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-WRITING       |
      | 20260613T1802_implementing         | implementing        | doer     | plan.md                | implementing.md     | HOOK-MARKER: TRACK-LIFECYCLE-IMPLEMENTING       |
      | 20260613T1803_impl_revision        | impl_revision       | doer     | plan.md                | implementing.md     | HOOK-MARKER: TRACK-LIFECYCLE-IMPLEMENTING       |
      | 20260613T1804_reflecting           | reflecting          | doer     | impl.review.md         | reflecting.md       | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTING         |
      | 20260613T1805_reflection_revision  | reflection_revision | doer     | reflection.md          | reflecting.md       | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTING         |
      | 20260613T1806_amend                | amend               | doer     | spec.md                | amend-writing.md    | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-WRITING      |
      | 20260613T1807_amend_revision       | amend_revision      | doer     | spec.amendments.md     | amend-writing.md    | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-WRITING      |
      | 20260613T1808_complete             | reflection_review   | complete | reflection.review.md   | complete.md         | HOOK-MARKER: TRACK-LIFECYCLE-COMPLETE           |

  Scenario Outline: begin on a track reviewer phase serves the migrated hook body
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "<hook_file>" with content "<marker>"
    When begin is called via query adapter with identifier "<track>" session_role "reviewer" and a registry declaring state "<state>" role "reviewer" hook "<hook_file>"
    Then the begin outcome is successful
    And the begin outcome result state is "<state>"
    And the begin outcome result review_context_text contains "<marker>"

    Examples:
      | track                             | state              | artifact_file      | hook_file              | marker                                                 |
      | 20260613T1810_plan_review         | plan_review        | plan.md            | plan-review.md         | HOOK-MARKER: TRACK-LIFECYCLE-PLAN-REVIEW              |
      | 20260613T1811_impl_phase_review   | impl_phase_review  | plan.md            | impl-phase-review.md   | HOOK-MARKER: TRACK-LIFECYCLE-IMPL-PHASE-REVIEW        |
      | 20260613T1812_impl_review         | impl_review        | plan.md            | impl-review.md         | HOOK-MARKER: TRACK-LIFECYCLE-IMPL-REVIEW              |
      | 20260613T1813_reflection_review   | reflection_review  | reflection.md      | reflection-review.md   | HOOK-MARKER: TRACK-LIFECYCLE-REFLECTION-REVIEW        |
      | 20260613T1814_amend_review        | amend_review       | spec.amendments.md | amend-review.md        | HOOK-MARKER: TRACK-LIFECYCLE-AMEND-REVIEW             |
