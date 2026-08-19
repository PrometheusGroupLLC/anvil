Feature: Kit build script reproducibility
  Running the build script twice must produce identical non-binary output.
  This scenario is tagged @slow because it invokes cargo build --release twice.

  @slow
  Scenario: two sequential builds produce identical non-binary output
    Then two sequential runs of the build script produce identical non-binary output
