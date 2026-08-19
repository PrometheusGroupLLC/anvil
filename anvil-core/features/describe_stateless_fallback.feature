Feature: Describe resolves a stateless-but-transitioned artifact via the fallback
  FileSystemDescribeAdapter::read_instance reads an artifact's state from the
  top-level `state` field. When that field is absent but transitions exist, it
  must resolve state from the last transition's `to` (via the shared accessor),
  while still reporting last_transition as a separate field. This drives the
  REAL FileSystemDescribeAdapter, not the synthetic TestDescribeAdapter.

  Scenario: read_instance resolves state from the last transition when top-level state is absent
    Given a describe fs hearth with:
      | path                                        | content                                                                    |
      | decisions/event-format-choice/status.yaml    | version: 1\nkind: decision\ntransitions:\n  - to: tension\n  - to: decided |
    When describe fs read_instance is called for "event-format-choice"
    Then the describe fs instance state is "decided"
    And the describe fs instance last_transition to is "decided"

  Scenario: read_instance folds the transition tail over a stale top-level state
    Given a describe fs hearth with:
      | path                                      | content                                                                  |
      | tracks/20260417T0200_topstate/status.yaml  | version: 1\nkind: track\nstate: spec\ntransitions:\n  - to: spec\n  - to: plan |
    When describe fs read_instance is called for "20260417T0200_topstate"
    Then the describe fs instance state is "plan"
    And the describe fs instance last_transition to is "plan"
