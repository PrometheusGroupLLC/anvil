Feature: Engine captures sparks end-to-end
  Spark capture is projection-only through begin(spark): source and projection
  files update, but no spark artifact directory/status.yaml is scaffolded.

  Scenario: begin spark updates sparks files without creating an artifact directory
    Given a hearth seeded with the spark_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC captures spark "Live engine projection-only spark"
    Then the spark begin RPC response state is "captured"
    And the spark begin RPC response track_path is "sparks/sparks.md"
    And the hearth spark source contains "Live engine projection-only spark"
    And the hearth spark projection contains "Untriaged sparks: 1"
    And no spark artifact status.yaml exists
