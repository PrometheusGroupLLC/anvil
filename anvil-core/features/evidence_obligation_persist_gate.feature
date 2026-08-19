Feature: Persist write-boundary evidence-obligation gate (dark-gated)
  The persist WRITE boundary always enforces the measurement DEFINE block; the
  evidence-obligation leg rides the same boundary but is independently gated. With
  the obligation leg OFF, persisting behaves exactly as today — a DRIVEN measured
  step without an obligation is written. With the leg ON, a DRIVEN measured
  (state, role) lacking an obligation is refused, a FREE-registered machine
  declaring an obligation is refused, and an obligation-bearing DRIVEN machine
  still persists. A FREE machine that declares no obligation is never refused,
  regardless of the leg. Every fixture is measurement-conformant
  (outcome_predicate + per-step success_criteria + a begin hook) so the leg under
  test is evidence-obligation, not measurement.

  # C10 (persist) — obligation leg off preserves today's persist behavior.
  Scenario: A DRIVEN measured machine without an obligation persists while the leg is off
    Given a registry built from an empty temp owner-home
    And a driven measured machine.yaml without an obligation for kind "driven_kind"
    When the persist handler executes for kind "driven_kind" under evidence obligation enforcement "off"
    Then the persist handler succeeds
    And the persist handler returns one PlaybookPersisted event

  # C11 (persist) — the DRIVEN-missing refusal is diagnosable at the write boundary.
  Scenario: A DRIVEN measured pair without an obligation is refused while the leg is on
    Given a registry built from an empty temp owner-home
    And a driven measured machine.yaml without an obligation for kind "driven_kind"
    When the persist handler executes for kind "driven_kind" under evidence obligation enforcement "on"
    Then the persist handler returns a LoaderInvalid error
    And the persist handler error code is "playbook_evidence_obligation_missing"
    And the persist handler error names kind "driven_kind"
    And the persist handler error names state "active"
    And the persist handler error names role "doer"
    And the persist handler emits no event

  # C13 (persist) — the DRIVEN pass-case is constructible at the write boundary.
  Scenario: A DRIVEN machine whose measured pairs all carry obligations persists while the leg is on
    Given a registry built from an empty temp owner-home
    And a driven measured machine.yaml with an obligation for kind "driven_kind"
    When the persist handler executes for kind "driven_kind" under evidence obligation enforcement "on"
    Then the persist handler succeeds
    And the persist handler returns one PlaybookPersisted event

  # C17 (persist leg) — FREE kinds may not declare obligations when the leg is on.
  Scenario: A FREE machine declaring an obligation is refused while the leg is on
    Given a registry built from an empty temp owner-home
    And a free measured machine.yaml declaring an obligation for kind "free_kind"
    When the persist handler executes for kind "free_kind" under evidence obligation enforcement "on"
    Then the persist handler returns a LoaderInvalid error
    And the persist handler error code is "playbook_evidence_obligation_on_free_register"
    And the persist handler error names kind "free_kind"
    And the persist handler emits no event

  # C18 (persist) — FREE kinds without an obligation are never refused, leg on or off.
  Scenario Outline: A FREE machine without an obligation persists regardless of the leg
    Given a registry built from an empty temp owner-home
    And a free measured machine.yaml without an obligation for kind "free_kind"
    When the persist handler executes for kind "free_kind" under evidence obligation enforcement "<leg>"
    Then the persist handler succeeds
    And the persist handler returns one PlaybookPersisted event

    Examples:
      | leg |
      | off |
      | on  |
