Feature: track_lifecycle declares the first live evidence obligation (T-ACT-1)
  The evidence-contract framework (the `evidence_obligation` field, its
  strength/substitution predicate, the unconditional emit path, and the dark
  authoring gate) shipped in T-EEC-1 but no registered machine declared an
  obligation — the rail carried no train. T-ACT-1 puts the first train on it by
  declaring `evidence_obligation` on the highest-traffic live lane,
  `track_lifecycle` (the canonical `track_seed()`), on exactly two demonstrator
  steps: `spec`/doer (a landed, review-gated artifact_of_consequence) and
  `plan`/doer (an artifact_of_consequence whose success criterion also demands
  verifiable_citation). Record-mode only: the `ANVIL_ENFORCE_EVIDENCE_OBLIGATION`
  flag stays OFF, so the emit path assesses against the declaration while the
  authoring gate never runs on the machine.

  # S1 — the two declared pairs carry EXACTLY the intended classes, and nothing
  # else appears on either (the over-declaration zero-case on each pair).
  Scenario: The spec and plan doer steps declare exactly the intended evidence classes
    Given the track seed
    Then the track seed state "spec" role "doer" declares evidence classes "artifact_of_consequence"
    And the track seed state "plan" role "doer" declares evidence classes "verifiable_citation, artifact_of_consequence"

  # S2 — none of the other 15 measured (state, role) pairs gains an obligation
  # (exhaustive, not spot-checked): the under-nothing-else zero-case guarding
  # against over-declaration flooding the live sink.
  Scenario: The other fifteen measured pairs keep an empty evidence obligation
    Given the track seed
    Then the track seed state "spec_review" role "reviewer" has an empty evidence obligation
    And the track seed state "spec_revision" role "doer" has an empty evidence obligation
    And the track seed state "plan_review" role "reviewer" has an empty evidence obligation
    And the track seed state "plan_revision" role "doer" has an empty evidence obligation
    And the track seed state "implementing" role "doer" has an empty evidence obligation
    And the track seed state "impl_phase_review" role "reviewer" has an empty evidence obligation
    And the track seed state "impl_review" role "reviewer" has an empty evidence obligation
    And the track seed state "impl_revision" role "doer" has an empty evidence obligation
    And the track seed state "reflecting" role "doer" has an empty evidence obligation
    And the track seed state "reflection_review" role "reviewer" has an empty evidence obligation
    And the track seed state "reflection_review" role "complete" has an empty evidence obligation
    And the track seed state "reflection_revision" role "doer" has an empty evidence obligation
    And the track seed state "amend" role "doer" has an empty evidence obligation
    And the track seed state "amend_review" role "reviewer" has an empty evidence obligation
    And the track seed state "amend_revision" role "doer" has an empty evidence obligation

  # S3 — the three unmeasured terminal-ish states carry no measurement entry at
  # all, so they never enter the loader's measurement scan.
  Scenario: The unmeasured states carry no measurement entry
    Given the track seed
    Then the track seed state "completed" has no measurement entry
    And the track seed state "abandoned" has no measurement entry
    And the track seed state "superseded" has no measurement entry

  # S4 — the declaration ALONE is enough for the emit path to observe evidence:
  # assess_evidence_obligation on spec/doer with no claims returns Some(Absent),
  # not None (a live spec/doer transition would now emit a non-None Absent row).
  Scenario: The declared spec obligation yields an Absent assessment with no claims
    Given the track seed
    Then assessing the track seed state "spec" role "doer" against no claims yields status "absent" and missing classes "artifact_of_consequence"
    And assessing the track seed state "plan" role "doer" against no claims yields status "absent" and missing classes "verifiable_citation, artifact_of_consequence"

  # S5 — declaring the two obligations re-hashes the machine's content version
  # (the intended variant event), proved on the REAL production seed content by
  # differencing the post-P0 seed against a copy with the two obligations reset.
  Scenario: Declaring the obligations changes the track machine content version
    Then declaring the two obligations changes the track machine content version

  # S6a — record-mode shipping posture: with the enforcement flag OFF (today's
  # default), the machine loads cleanly and keeps both declared obligations —
  # zero behavior change. This is exactly the path the live engine takes.
  Scenario: With enforcement off the track machine loads and retains its obligations
    Then with evidence-obligation enforcement off the track seed loads and retains both declared obligations

  # S6b — the (unused) flag-ON authoring gate DELIBERATELY rejects the partial
  # 2-of-17 declaration: the validator iterates every measured pair and fails on
  # the first empty one. This is correct for a record-mode-only declaration
  # (the machine is intentionally insufficient for a hypothetical flag-ON gate).
  # AC12: because track is register-driven, the FREE-register obligation branch
  # is structurally unreachable regardless of flag state.
  Scenario: Under enforcement the partial declaration is deliberately rejected
    Then validating the track seed under evidence-obligation enforcement returns error code "playbook_evidence_obligation_missing"
    And the track seed is register driven
