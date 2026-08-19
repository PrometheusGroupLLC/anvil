Feature: Complete — satisfaction unknown rejection
  CompleteCommandHandler returns SatisfactionUnknown when satisfaction is
  an unrecognized value — distinct from out-of-scope (which names well-known
  deferred values). One scenario per R8.1(d) with a "bogus" value.
  Message must embed satisfaction_unknown code, name the offending value,
  and enumerate all four well-known values.

  Scenario: satisfaction "bogus" returns satisfaction_unknown with all well-known values listed
    Given a complete fs hearth with:
      | path                                                              | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
      | tracks/20260419T0420_satisfaction_unknown/status.yaml            | version: 1\nkind: track\nstate: spec_review\nproposal: 20260411T2021_anvil_workflow_engine\nactors:\n  Author-420001:\n    type: agent\n    configurations:\n      - at: "2026-04-19T00:00:00Z"\n        model: claude-opus-4-6\n        provider: anthropic\n        details:\n          context_window: 1000000\n          sdk_version: ""\n          entrypoint: claude-code\ntransitions:\n  - to: spec_review\n    at: 2026-04-19T00:00:00Z\n    actor: Author-420001\n    role: spec\n    approver: mark\n |
    When complete fs is executed with:
      | artifact_path  | tracks/20260419T0420_satisfaction_unknown |
      | actor_name     | Reviewer-420001                           |
      | actor_type     | agent                                     |
      | actor_model    | claude-opus-4-7                           |
      | actor_provider | anthropic                                 |
      | satisfaction   | bogus                                     |
      | at             | 2026-04-19T04:20:00Z                      |
    Then the complete result is a CompleteError containing "satisfaction_unknown"
    And the complete result is a CompleteError containing "bogus"
    And the complete result is a CompleteError containing "satisfied"
    And the complete result is a CompleteError containing "full_revision"
    And the complete result is a CompleteError containing "address_in_next_step"
