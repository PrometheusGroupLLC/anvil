Feature: anvil-hooks complete presents claimed evidence (T-ACT-2 CLI leg)
  The `anvil-hooks complete` command exposes a repeatable
  `--claimed-evidence <class>:<reference>` affordance that threads ordered
  evidence claims into the engine's CompleteRequest. Against the live
  `track_lifecycle` seed obligation (T-ACT-1), the resulting durable row on
  `<hearth>/step-measurement.jsonl` records the tri-state assessment. This is
  record mode: no transition is refused regardless of the claim.

  # AC1, AC5, AC8, AC11 (internal colon preserved), AC9
  Scenario: A spec doer completion presenting the obligated class records present-as-claimed
    Given a track_lifecycle hearth with a track in state "spec"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer presenting claims:
      | class                   | reference                        |
      | artifact_of_consequence | commit:deadbeefcafe              |
      | verifiable_citation     | anvil-engine/src/main.rs:467     |
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row lists claims in order:
      | class                   | reference                        |
      | artifact_of_consequence | commit:deadbeefcafe              |
      | verifiable_citation     | anvil-engine/src/main.rs:467     |
    And the emitted evidence row names missing classes ""
    And the emitted evidence row carries the track_lifecycle seed content version
    And the track resolved state is "spec_review"

  # AC7, AC9 — absent, still succeeds (record mode)
  Scenario: A spec doer completion presenting no claim records absent and still advances
    Given a track_lifecycle hearth with a track in state "spec"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer presenting no claims
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "absent"
    And the emitted evidence row names missing classes "artifact_of_consequence"
    And the emitted evidence row carries the track_lifecycle seed content version
    And the track resolved state is "spec_review"

  # AC6, AC9 — the weaker claim leaves the stronger leg unsatisfied
  Scenario: A spec doer completion presenting only the weaker class records incomplete
    Given a track_lifecycle hearth with a track in state "spec"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer presenting claims:
      | class               | reference               |
      | verifiable_citation | anvil-core/src/lib.rs:1 |
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "incomplete"
    And the emitted evidence row names missing classes "artifact_of_consequence"
    And the track resolved state is "spec_review"

  # AC11 — the opaque reference round-trips verbatim; raw note text never lands
  Scenario: A completion carrying a raw note keeps the claim reference opaque
    Given a track_lifecycle hearth with a track in state "spec"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer with note "RAW-NOTE-must-not-leak" presenting claims:
      | class                   | reference           |
      | artifact_of_consequence | commit:0ff1ce123    |
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row reference contains "commit:0ff1ce123"
    And no emitted step measurement row contains the text "RAW-NOTE-must-not-leak"

  # AC2, AC13 — omitting the flag on a non-obligated step is legacy-identical (no evidence keys)
  Scenario: A reviewer completion on a non-obligated step emits no evidence keys
    Given a track_lifecycle hearth with a track in state "spec_review"
    And the engine is started with that hearth
    When anvil-hooks completes the track as reviewer satisfied presenting no claims
    Then the anvil-hooks complete command exits 0
    And no emitted step measurement row carries any evidence key

  # AC5 substitution control (P4-S3) — a lone strong claim masks the absent citation
  Scenario: A plan doer completion presenting only the strong class records present-as-claimed
    Given a track_lifecycle hearth with a track in state "plan"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer presenting claims:
      | class                   | reference        |
      | artifact_of_consequence | commit:abc123def |
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row lists claims in order:
      | class                   | reference        |
      | artifact_of_consequence | commit:abc123def |
    And the emitted evidence row names missing classes ""
    And the track resolved state is "plan_review"

  # AC5 plan leg (P4-S2) — two distinct claims, both legs genuinely presented
  Scenario: A plan doer completion presenting both obligated classes records present-as-claimed
    Given a track_lifecycle hearth with a track in state "plan"
    And the engine is started with that hearth
    When anvil-hooks completes the track as doer presenting claims:
      | class                   | reference                    |
      | verifiable_citation     | anvil-engine/src/main.rs:467 |
      | artifact_of_consequence | commit:abc123def             |
    Then the anvil-hooks complete command exits 0
    And the emitted evidence row records status "present-as-claimed"
    And the emitted evidence row names missing classes ""
    And the emitted evidence row carries the track_lifecycle seed content version
    And the track resolved state is "plan_review"
