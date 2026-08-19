Feature: Step 2x2 fold — the blind-gate detector
  The step 2x2 folds two existing sinks into a per-(playbook_kind, step_kind)
  diagnostic pair: the one-shot gate PASS-RATE (from the activity log: an instance
  reached a step's *_review gate and never bounced back into *_revision) and the
  DEFECT-ESCAPE rate (from the review-verdict sink: a finding attributed by its
  origin_phase to a step, recorded at a gate whose phase differs from that origin
  — a defect that escaped its origin gate). Each cell carries its own N per axis.
  High pass-rate x high escape-rate is a blind gate. Both folds are pure over the
  already-read record vectors; records lacking a playbook_run_id are skipped.

  Scenario: one-shot pass-rate is gate exits that never bounced into revision
    # inst_a passes the spec gate one-shot (spec_review -> plan). inst_b bounces
    # (spec_review -> spec_revision, re-reviewed, then spec_review -> plan). So for
    # step "spec": 2 gate observations, 1 one-shot pass.
    Given a step 2x2 activity stream:
      | command  | from_state    | to_state      | at                   | artifact_kind | playbook_run_id |
      | begin    |               | spec          | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec          | spec_review   | 2026-06-15T10:00:00Z | track         | inst_a               |
      | complete | spec_review   | plan          | 2026-06-15T11:00:00Z | track         | inst_a               |
      | begin    |               | spec          | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | spec          | spec_review   | 2026-06-15T10:00:00Z | track         | inst_b               |
      | complete | spec_review   | spec_revision | 2026-06-15T11:00:00Z | track         | inst_b               |
      | complete | spec_revision | spec_review   | 2026-06-15T12:00:00Z | track         | inst_b               |
      | complete | spec_review   | plan          | 2026-06-15T13:00:00Z | track         | inst_b               |
    And an empty step 2x2 verdict stream
    When the step 2x2 is folded
    Then the step 2x2 cell for "track" step "spec" has gate observations 2
    And the step 2x2 cell for "track" step "spec" has one-shot passes 1

  Scenario: defect-escape counts findings caught at a gate other than their origin
    # Two findings attributed to origin_phase "spec": one recorded at impl_review
    # (phase "impl" != "spec" -> escaped), one at spec_review (contained). So for
    # step "spec": 2 escape observations, 1 defect escape.
    Given an empty step 2x2 activity stream
    And a step 2x2 verdict stream:
      | artifact_kind | gate_state  | dimension    | severity | origin_phase |
      | track         | impl_review | correctness  | blocker  | spec         |
      | track         | spec_review | clarity      | minor    | spec         |
    When the step 2x2 is folded
    Then the step 2x2 cell for "track" step "spec" has escape observations 2
    And the step 2x2 cell for "track" step "spec" has defect escapes 1

  Scenario: a blind gate shows high one-shot pass-rate and high escape-rate together
    # The spec gate waves everything through one-shot (2/2) yet every spec defect
    # is caught downstream (2/2 escaped) — the high/high blind-gate signature.
    Given a step 2x2 activity stream:
      | command  | from_state  | to_state    | at                   | artifact_kind | playbook_run_id |
      | begin    |             | spec        | 2026-06-15T09:00:00Z | track         | inst_a               |
      | complete | spec        | spec_review | 2026-06-15T10:00:00Z | track         | inst_a               |
      | complete | spec_review | plan        | 2026-06-15T11:00:00Z | track         | inst_a               |
      | begin    |             | spec        | 2026-06-15T09:00:00Z | track         | inst_b               |
      | complete | spec        | spec_review | 2026-06-15T10:00:00Z | track         | inst_b               |
      | complete | spec_review | plan        | 2026-06-15T11:00:00Z | track         | inst_b               |
    And a step 2x2 verdict stream:
      | artifact_kind | gate_state  | dimension   | severity | origin_phase |
      | track         | impl_review | correctness | blocker  | spec         |
      | track         | plan_review | correctness | blocker  | spec         |
    When the step 2x2 is folded
    Then the step 2x2 cell for "track" step "spec" has gate observations 2
    And the step 2x2 cell for "track" step "spec" has one-shot passes 2
    And the step 2x2 cell for "track" step "spec" has escape observations 2
    And the step 2x2 cell for "track" step "spec" has defect escapes 2

  Scenario: a finding without an origin_phase contributes nothing to the escape axis
    Given an empty step 2x2 activity stream
    And a step 2x2 verdict stream:
      | artifact_kind | gate_state  | dimension   | severity | origin_phase |
      | track         | spec_review | correctness | minor    |              |
    When the step 2x2 is folded
    Then the step 2x2 has no cell for "track" step "spec"

  Scenario: empty streams fold to an empty result
    Given an empty step 2x2 activity stream
    And an empty step 2x2 verdict stream
    When the step 2x2 is folded
    Then the step 2x2 has 0 cells
