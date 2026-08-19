Feature: AC2 + AC3 (engine) — doer-complete and reviewer-complete drive a builder gate over the RPC
  The same gate drive as the core seam, exercised over the real engine RPC: a
  begun playbook_generation artifact (against an active parent track) advances
  gathering -> gathering_review via doer-complete, then gathering_review ->
  analyzing via reviewer-complete(approved). The driver asserts each hop's
  machine-declared new_state internally.

  Scenario: doer-complete then reviewer-complete drive the first builder gate
    Given a hearth seeded with the builder machine and an active parent track
    And the engine is started with that hearth
    When the engine drives a playbook_generation artifact through the first gate
    Then the e2e final state is "analyzing"
