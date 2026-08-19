Feature: Cross-slice landing guarantee — reflection_notes after Slice B ships (R6.3)
  Per spec R6.3: when Slice B has shipped, reviewer-full_revision +
  reflection_notes works and writes to spec_review_reflection/.
  This feature file lands now; all scenarios are @pending until Slice B ships.
  Enable by removing @pending and confirming Slice B (full_revision path) is on main.

  # @pending Scenario: Reviewer-complete with full_revision + reflection_notes works after Slice B
  # (spec R6.3) — conditional on Slice B (full_revision path) shipping.
  # Enable by adding the scenario back when Slice B is on main.
