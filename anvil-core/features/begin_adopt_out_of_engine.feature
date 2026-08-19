Feature: Adopt an out-of-engine artifact
  An artifact that exists on disk but was authored outside the engine (no
  recorded transition history AND no engine begin marker) can be ADOPTED:
  begin(identifier, adopt: true) takes it back to the machine's INITIAL state
  and drives it through every phase and review gate properly. The hand-made
  files are preserved as the doer's raw material; the reset is recorded as a
  structurally-distinct appended ArtifactAdopted event (never a rewrite).
  Adoption is explicit opt-in — a plain begin is unchanged.

  Scenario: adopt resets a pre-governance track to its initial state
    Given an in-memory query adapter with track "20260716T0001_handmade" in state "implementing" and artifact file "spec.md" content "HANDMADE-SPEC-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" with content "SPEC-DOER-HOOK-MARKER"
    When begin is called via query adapter with identifier "20260716T0001_handmade" session_role "resumer" adopt "true" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is successful
    And the begin outcome result state is "spec"
    And the handler emitted an ArtifactAdopted event to state "spec"
    And the handler emitted no ReviewTransition event
    And the handler emitted a BeginMarkerWritten event with state "spec"
    And the begin outcome result measurement_role is "doer"
    And the begin outcome result context_text contains "SPEC-DOER-HOOK-MARKER"
    And the begin outcome result context_text contains "authored outside the engine"
    And the begin outcome result context_text contains "implementing"

  Scenario: adopt refuses an already engine-governed artifact (recorded transition)
    Given an in-memory query adapter with track "20260716T0002_governed" in state "implementing" governed by a prior transition
    When begin is called via query adapter with identifier "20260716T0002_governed" session_role "resumer" adopt "true" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is an AlreadyGoverned error
    And the handler emitted no events

  Scenario: adopt refuses an artifact carrying only a begin marker (no transition)
    Given an in-memory query adapter with track "20260716T0005_markeronly" in state "implementing" carrying only a begin marker
    When begin is called via query adapter with identifier "20260716T0005_markeronly" session_role "resumer" adopt "true" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is an AlreadyGoverned error
    And the handler emitted no events

  Scenario: adopt fails closed when governance evidence is damaged
    Given an in-memory query adapter with track "20260716T0006_degraded" in state "implementing" with a degraded activity log
    When begin is called via query adapter with identifier "20260716T0006_degraded" session_role "resumer" adopt "true" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is an AdoptionEvidenceUnreadable error
    And the handler emitted no events

  Scenario: adopt fails closed when the transition evidence is damaged
    # A track with no transition in its parsed status but a DAMAGED on-disk
    # transition event store. The lenient fold would skip the damaged event and
    # read an empty history → wrongly adopt; the strict adoption read must refuse
    # rather than reset an artifact whose only governing evidence is unreadable.
    Given an in-memory query adapter with track "20260716T0012_torn" in state "implementing" with damaged transition evidence
    When begin is called via query adapter with identifier "20260716T0012_torn" session_role "resumer" adopt "true" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is an AdoptionEvidenceUnreadable error
    And the handler emitted no events

  Scenario: a plain (non-adopt) begin is unchanged
    Given an in-memory query adapter with track "20260716T0004_plain" in state "implementing" and artifact file "spec.md" content "HANDMADE-SPEC-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "implementing.md" with content "IMPL-DOER-HOOK"
    When begin is called via query adapter with identifier "20260716T0004_plain" session_role "resumer" and a registry declaring state "implementing" role "doer" hook "implementing.md"
    Then the begin outcome is successful
    And the begin outcome result state is "implementing"

  Scenario: a fallback error offers adoption when the artifact is adoptable
    Given an in-memory query adapter with track "20260716T0007_adoptable" in state "plan_review" and spec content "body"
    When begin is called via query adapter with identifier "20260716T0007_adoptable" and session_role "resumer"
    Then the begin outcome is a ModeNotImplemented error naming "forge:implement"
    And the begin outcome error message contains "adopt: true"

  Scenario: a fallback error does NOT offer adoption for an already-governed artifact
    Given an in-memory query adapter with track "20260716T0008_governed" in state "plan_review" governed by a prior transition
    When begin is called via query adapter with identifier "20260716T0008_governed" and session_role "resumer"
    Then the begin outcome is a ModeNotImplemented error naming "forge:implement"
    And the begin outcome error message does not contain "adopt: true"

  Scenario: an adoption transition never closes the adopting begin marker
    Given an in-memory query adapter seeded with artifact "20260716T0009_open" kind "track" state "spec" with activity:
      | kind  | actor       | state | at                   |
      | begin | Doer-000001 | spec  | 2026-07-16T00:00:00Z |
    And the in-memory query adapter has an adoption transition for artifact "20260716T0009_open" actor "Doer-000001" at "2026-07-16T00:00:00Z"
    When has_open_begin is evaluated for actor "Doer-000001" state "spec" on "20260716T0009_open"
    Then has_open_begin result is true

  Scenario: an ordinary same-actor transition still closes the begin marker
    Given an in-memory query adapter seeded with artifact "20260716T0010_closed" kind "track" state "spec" with activity:
      | kind  | actor       | state | at                   |
      | begin | Doer-000001 | spec  | 2026-07-16T00:00:00Z |
    And the in-memory query adapter has a closing transition for artifact "20260716T0010_closed" actor "Doer-000001" at "2026-07-16T00:00:01Z"
    When has_open_begin is evaluated for actor "Doer-000001" state "spec" on "20260716T0010_closed"
    Then has_open_begin result is false

  Scenario: an interrupted adoption is resumable from its initial spec state
    Given an in-memory query adapter with track "20260716T0011_resume" in state "spec" and artifact file "spec.md" content "HANDMADE-SPEC-BODY"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" with content "SPEC-DOER-HOOK-MARKER"
    When begin is called via query adapter with identifier "20260716T0011_resume" session_role "resumer" and a registry declaring state "spec" role "doer" hook "spec-writing.md"
    Then the begin outcome is successful
    And the begin outcome result state is "spec"
    And the begin outcome result context_text contains "SPEC-DOER-HOOK-MARKER"
    And the handler emitted a BeginMarkerWritten event with state "spec"
