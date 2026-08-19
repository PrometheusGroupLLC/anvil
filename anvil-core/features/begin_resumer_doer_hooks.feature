Feature: Begin resumer serves doer hooks
  A resumer entering a track doer/revision state receives the doer hook body
  and is measured on the doer axis. Review-gate, terminal, and amend states
  remain outside this engine path.

  Scenario Outline: resumer begin on doer/revision state serves the doer hook
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "<hook_file>" with content "<marker>"
    When begin is called via query adapter with identifier "<track>" session_role "resumer" and a registry declaring state "<state>" role "doer" hook "<hook_file>"
    Then the begin outcome is successful
    And the begin outcome result state is "<state>"
    And the begin outcome result context_text contains "<marker>"
    And the begin outcome result measurement_role is "doer"
    And the handler emitted a BeginMarkerWritten event with state "<state>"

    Examples:
      | track                              | state               | artifact_file  | hook_file       | marker                                   |
      | 20260614T0600_plan                 | plan                | spec.md        | plan-writing.md | HOOK-MARKER: RESUMER-PLAN               |
      | 20260614T0601_implementing         | implementing        | plan.md        | implementing.md | HOOK-MARKER: RESUMER-IMPLEMENTING       |
      | 20260614T0602_reflecting           | reflecting          | reflection.md  | reflecting.md   | HOOK-MARKER: RESUMER-REFLECTING         |
      | 20260614T0603_spec_revision        | spec_revision       | spec.md        | spec-writing.md | HOOK-MARKER: RESUMER-SPEC-REVISION      |
      | 20260614T0604_plan_revision        | plan_revision       | plan.md        | plan-writing.md | HOOK-MARKER: RESUMER-PLAN-REVISION      |
      | 20260614T0605_impl_revision        | impl_revision       | plan.md        | implementing.md | HOOK-MARKER: RESUMER-IMPL-REVISION      |
      | 20260614T0606_reflection_revision  | reflection_revision | reflection.md  | reflecting.md   | HOOK-MARKER: RESUMER-REFLECTION-REVISION|

  Scenario Outline: resumer begin on non-doer state keeps the existing fallback
    Given an in-memory query adapter with track "<track>" in state "<state>" and artifact file "<artifact_file>" content "ARTIFACT-BODY"
    When begin is called via query adapter with identifier "<track>" and session_role "resumer"
    Then the begin outcome is a ModeNotImplemented error naming "forge:implement"

    Examples:
      | track                        | state             | artifact_file         |
      | 20260614T0610_plan_review    | plan_review       | plan.md               |
      | 20260614T0611_impl_review    | impl_review       | plan.md               |
      | 20260614T0612_amend          | amend             | spec.amendments.md    |
      | 20260614T0613_completed      | completed         | reflection.review.md  |
