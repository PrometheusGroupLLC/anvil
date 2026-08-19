Feature: Begin create records target_owner and gates it machine-driven
  target_owner is a first-class begin input. When the resolved machine declares
  a `target_owner` descriptor in its required_fields, begin-create requires it;
  omitting it yields a typed MissingRequiredField error (no kind literal — the
  check reads machine.required_fields). When supplied, target_owner rides on the
  emitted ArtifactCreation event's status. Kinds whose machine does NOT declare
  target_owner are unaffected. (Anvil-lane 1b; A1/A2/A3, pure handler.)

  Scenario: a machine that requires target_owner rejects a create that omits it
    Given a composite begin fs hearth seeded with a machine requiring target_owner and an active parent "20260606T0000_parent"
    When begin fs composite create is executed for artifact_type "owned_thing" parent "20260606T0000_parent" target_owner ""
    Then the begin outcome is a MissingRequiredField error for "target_owner"

  Scenario: a create supplying target_owner records it on the ArtifactCreation status
    Given a composite begin fs hearth seeded with a machine requiring target_owner and an active parent "20260606T0000_parent"
    When begin fs composite create is executed for artifact_type "owned_thing" parent "20260606T0000_parent" target_owner "kit:test-owner"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with status target_owner "kit:test-owner"

  Scenario: a kind whose machine does not require target_owner creates with none and records empty
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine
    When begin fs composite create is executed for artifact_type "knowledge_lifecycle" parent "" target_owner ""
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with status target_owner ""
