Feature: Complete filesystem — reviewer carry-forward (spec_review → plan, address_in_next_step)
  Slice C: a reviewer complete with satisfaction "address_in_next_step" on a
  spec_review track advances to plan AND writes carry-forward.md with the
  verbatim findings. The transition records satisfaction: address_in_next_step
  in its metadata, distinguishing carry-forward from outright acceptance.
  Registry and projection move to plan / Planned, same as the satisfied path.

  Scenario: Carry-forward complete with single-line findings — transitions, writes carry-forward.md, moves registry and projection (R7.1a)
    Given a complete fs hearth with:
      | path                                                                | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0700_carry_forward_track/status.yaml                | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-700001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-700001\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0700_carry_forward_track/spec.md                    | # Carry Forward Track\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
      | tracks.md                                                           | # Tracks\n\n## spec\n\n## spec_review\n\n- [Carry Forward Track](tracks/20260419T0700_carry_forward_track/) — carry forward track — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                       |
      | projections/execution.md                                            | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Carry Forward Track](tracks/20260419T0700_carry_forward_track/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                       |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0700_carry_forward_track                       |
      | actor_name     | Reviewer-700001                                                |
      | actor_type     | agent                                                          |
      | actor_model    | claude-opus-4-7                                                |
      | actor_provider | anthropic                                                      |
      | satisfaction   | address_in_next_step                                           |
      | findings       | The spec is accurate but please confirm X and Y during impl.  |
      | at             | 2026-04-19T07:00:00Z                                           |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the complete result transition_at is "2026-04-19T07:00:00Z"
    And the complete result artifact_path is "tracks/20260419T0700_carry_forward_track"
    And the complete result carry_forward_path is non-empty
    And the complete result carry_forward_path ends with "tracks/20260419T0700_carry_forward_track/carry-forward.md"
    And the resolved state of "tracks/20260419T0700_carry_forward_track" is "plan"
    And a transition event for "tracks/20260419T0700_carry_forward_track" contains "to: plan"
    And a transition event for "tracks/20260419T0700_carry_forward_track" contains "role: review"
    And a transition event for "tracks/20260419T0700_carry_forward_track" contains "satisfaction: address_in_next_step"
    And the file "tracks.md" contains "## plan"
    And the file "tracks.md" does not contain "20260419T0700_carry_forward_track" under section "## spec_review"
    And the file "projections/execution.md" contains "## Planned (1)"
    And the file "projections/execution.md" contains "Carry Forward Track"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "findings_from: spec_review"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "satisfied_by: Reviewer-700001"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "at: 2026-04-19T07:00:00Z"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "satisfaction: address_in_next_step"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "# Carry-forward from spec review"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "_Reviewer: Reviewer-700001 · 2026-04-19T07:00:00Z_"
    And the file "tracks/20260419T0700_carry_forward_track/carry-forward.md" contains "The spec is accurate but please confirm X and Y during impl."

  Scenario: Carry-forward findings beginning with --- are preserved verbatim (Risk 4 — YAML frontmatter collision)
    Given a complete fs hearth with:
      | path                                                                  | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0701_carry_forward_dashes/status.yaml                 | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-700002:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-700002\n    role: spec\n    approver: mark\n |
      | tracks/20260419T0701_carry_forward_dashes/spec.md                     | # Carry Forward Dashes\n\nSpec body.                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
      | tracks.md                                                             | # Tracks\n\n## spec\n\n## spec_review\n\n- [Carry Forward Dashes](tracks/20260419T0701_carry_forward_dashes/) — carry forward dashes — [anvil-workflow-engine](proposals/20260411T2021_anvil_workflow_engine/)\n\n## plan\n\n## implementing\n                                                                                                                                                                                                                                                                   |
      | projections/execution.md                                              | ---\nincremental_count: 0\nbase_snapshot: 2026-04-19T00:00:00Z\nlast_updated: 2026-04-19T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Execution\n\n## Spec (0)\n\n## Spec Review (1)\n\n- [Carry Forward Dashes](tracks/20260419T0701_carry_forward_dashes/)\n\n## Planned (0)\n\n## Implementing (0)\n                                                                                                                                                                                                   |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0701_carry_forward_dashes        |
      | actor_name     | Reviewer-700002                                  |
      | actor_type     | agent                                            |
      | actor_model    | claude-opus-4-7                                  |
      | actor_provider | anthropic                                        |
      | satisfaction   | address_in_next_step                             |
      | findings       | ---\nlooks_like: frontmatter\n---\nbut is body   |
      | at             | 2026-04-19T07:01:00Z                             |
    Then the complete result is successful
    And the complete result new_state is "plan"
    And the file "tracks/20260419T0701_carry_forward_dashes/carry-forward.md" contains "findings_from: spec_review"
    And the file "tracks/20260419T0701_carry_forward_dashes/carry-forward.md" contains "looks_like: frontmatter"
    And the file "tracks/20260419T0701_carry_forward_dashes/carry-forward.md" contains "but is body"
