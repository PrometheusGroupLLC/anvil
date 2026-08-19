Feature: Begin create accepts any machine-declared required field via a generic field bag
  Machines may declare required_fields outside the builtin set (name, parent_id,
  approver, target_owner). begin-create now carries a generic `fields` bag so ANY
  declared field can be supplied. A machine declaring `question` + `requester`
  rejects a create that omits them (typed MissingRequiredField), and a create that
  supplies them records each declared value on the emitted ArtifactCreation
  status, leaving unrelated bag entries off the status. The first hook's
  context_text interpolates `{{key}}` placeholders with the supplied values.

  Scenario: a machine requiring generic fields rejects a create that omits them
    Given a composite begin fs hearth seeded with a machine requiring generic fields question and requester
    When begin fs composite create is executed for artifact_type "generic_thing" with no generic fields
    Then the begin outcome is a MissingRequiredField error for "question,requester"

  Scenario: a create supplying the generic fields records each declared value on the ArtifactCreation status
    Given a composite begin fs hearth seeded with a machine requiring generic fields question and requester
    When begin fs composite create is executed for artifact_type "generic_thing" with field "question" = "what is anvil" and field "requester" = "Nick"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with status field "question" = "what is anvil"
    And the begin outcome emits an ArtifactCreation event with status field "requester" = "Nick"

  Scenario: only declared required fields land on the status, unrelated bag entries are dropped
    Given a composite begin fs hearth seeded with a machine requiring generic fields question and requester
    When begin fs composite create is executed for artifact_type "generic_thing" with field "question" = "q" and field "requester" = "r" and field "stowaway" = "nope"
    Then the begin outcome is successful
    And the begin outcome ArtifactCreation status has no field "stowaway"

  Scenario: the first hook context_text interpolates supplied generic field placeholders
    Given a composite begin fs hearth seeded with a machine requiring generic fields question and requester
    When begin fs composite create is executed for artifact_type "generic_thing" with field "question" = "what is anvil" and field "requester" = "Nick"
    Then the begin outcome is successful
    And the begin outcome context_text contains "what is anvil"
    And the begin outcome context_text does not contain "{{question}}"
