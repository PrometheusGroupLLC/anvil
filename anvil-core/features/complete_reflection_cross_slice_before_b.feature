Feature: Cross-slice landing guarantee — reflection_notes before Slice B ships (R6.2)
  Per spec R6.2: when this track ships before Slice B, reflection_notes is
  honored on Slice A's two call shapes but full_revision + reflection_notes
  is still rejected (Slice A's guard unchanged).

  Scenario: Doer-complete on spec with reflection_notes works before Slice B
    Given a complete fs hearth with:
      | path                                                                         | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | tracks/20260420T0210_xslice_doer/status.yaml                                 | version: 1\nkind: track\nstate: spec\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-xslice01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec\n    at: 2026-04-20T00:00:00Z\n    actor: Author-xslice01\n    role: spec\n    approver: mark\n                    |
      | tracks/20260420T0210_xslice_doer/spec.md                                     | # Cross Slice Doer\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                                    | # Tracks\n\n## spec\n\n- [Cross Slice Doer](tracks/20260420T0210_xslice_doer/) — cross slice doer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## spec_review\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                              |
      | projections/execution.md                                                     | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (1)\n\n- [Cross Slice Doer](tracks/20260420T0210_xslice_doer/)\n\n## Spec Review (0)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                         |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_xslice_doer |
      | actor_name            | Doer-xslice01                    |
      | actor_type            | agent                            |
      | actor_model           | claude-opus-4-7                  |
      | actor_provider        | anthropic                        |
      | actor_context_window  | 200000                           |
      | actor_entrypoint      | claude-code                      |
      | reflection_notes      | Doer observation before Slice B. |
      | at                    | 2026-04-20T11:00:00Z             |
    Then the complete result is successful
    And the complete result new_state is "spec_review"
    And the complete result reflection_path ends with "spec_reflection/20260420T110000Z-Doer-xslice01.md"

  Scenario: Reviewer-complete on spec_review with satisfied + reflection_notes works before Slice B
    Given a complete fs hearth with:
      | path                                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
      | tracks/20260420T0210_xslice_reviewer/status.yaml                                  | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-xsliceR01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-xsliceR01\n    role: spec\n    approver: mark\n            |
      | tracks/20260420T0210_xslice_reviewer/spec.md                                      | # Cross Slice Reviewer\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
      | tracks.md                                                                         | # Tracks\n\n## spec\n\n## spec_review\n\n- [Cross Slice Reviewer](tracks/20260420T0210_xslice_reviewer/) — cross slice reviewer — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                       |
      | projections/execution.md                                                          | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Cross Slice Reviewer](tracks/20260420T0210_xslice_reviewer/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                           |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_xslice_reviewer |
      | actor_name            | Reviewer-xslice01                    |
      | actor_type            | agent                                |
      | actor_model           | claude-opus-4-7                      |
      | actor_provider        | anthropic                            |
      | actor_context_window  | 200000                               |
      | actor_entrypoint      | claude-code                          |
      | satisfaction          | satisfied                            |
      | reflection_notes      | Reviewer observation before Slice B. |
      | at                    | 2026-04-20T11:05:00Z                 |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result reflection_path ends with "spec_review_reflection/20260420T110500Z-Reviewer-xslice01.md"

  Scenario: address_in_next_step without findings + reflection_notes still rejected, no reflection file (R6.2 negative)
    # reflection_notes is an input-shape addition; it does not change
    # satisfaction validation. Slice C accepts address_in_next_step but requires
    # findings — absent findings the call is rejected and short-circuits before
    # any reflection file is written.
    Given a complete fs hearth with:
      | path                                                                           | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
      | tracks/20260420T0210_xslice_reject/status.yaml                                 | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-xsliceJ01:\n    type: agent\n    configurations:\n      - at: "2026-04-20T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-20T00:00:00Z\n    actor: Author-xsliceJ01\n    role: spec\n    approver: mark\n |
      | tracks/20260420T0210_xslice_reject/spec.md                                     | # Cross Slice Reject\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
      | tracks.md                                                                      | # Tracks\n\n## spec\n\n## spec_review\n\n- [Cross Slice Reject](tracks/20260420T0210_xslice_reject/) — cross slice reject — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                                        |
      | projections/execution.md                                                       | ---\nincremental_count: 0\nbase_snapshot: 2026-04-20T00:00:00Z\nlast_updated: 2026-04-20T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Cross Slice Reject](tracks/20260420T0210_xslice_reject/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                                              |
    When complete fs is executed with:
      | artifact_path         | tracks/20260420T0210_xslice_reject |
      | actor_name            | Reviewer-xsliceJ01                 |
      | actor_type            | agent                              |
      | actor_model           | claude-opus-4-7                    |
      | actor_provider        | anthropic                          |
      | actor_context_window  | 200000                             |
      | actor_entrypoint      | claude-code                        |
      | satisfaction          | address_in_next_step               |
      | reflection_notes      | This should be rejected.           |
      | at                    | 2026-04-20T11:10:00Z               |
    Then the complete result is a CompleteError containing "findings_required_for_address_in_next_step"
    And the directory "tracks/20260420T0210_xslice_reject/spec_review_reflection" does not exist
