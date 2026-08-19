Feature: Engine-driven spark capture
  Spark is a projection-only free kind. Capturing one through begin writes the
  spark event log and updates the sparks projection without creating a lifecycle
  artifact directory or status.yaml.

  Scenario: begin spark appends to sparks.md and creates no artifact status
    Given a spark lifecycle hearth
    When begin captures spark "Projection-only spark capture belongs in the engine"
    Then the spark begin result state is "captured"
    And the spark begin result track_path is "sparks/sparks.md"
    And the spark begin context contains "Capture the idea as a spark"
    And the spark source contains "Projection-only spark capture belongs in the engine"
    And the spark projection contains "Untriaged sparks: 1"
    And no spark artifact status.yaml exists
