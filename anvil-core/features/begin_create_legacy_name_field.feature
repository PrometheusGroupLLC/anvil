Feature: The pre-migration name a machine declares has the same dedicated home as the canonical one
  A `machine.yaml` is persisted hearth data. This repo reads those definitions;
  it does not own them, and the live builder machine in anvil-hearth declares
  the pre-migration spelling of the playbook-name field. Both spellings resolve
  the ONE dedicated name value on the request — neither is a generic bag field,
  and there is no second value anywhere.

  If the pre-migration name were treated as a generic field instead, the value a
  caller supplies in the bag would be recorded and interpolated while the
  required-field check kept reading the dedicated one. Two homes for one value
  is how a create loop becomes unsatisfiable, so each scenario below pins one
  half of the single home.

  Scenario: the dedicated name satisfies the declared field
    Given a composite begin fs hearth seeded with a machine declaring the pre-migration name field
    When begin fs composite create is executed for artifact_type "legacy_named" with the dedicated name "daily digest"
    Then the begin outcome is successful

  Scenario: the first hook resolves the declared placeholder from the dedicated name
    Given a composite begin fs hearth seeded with a machine declaring the pre-migration name field
    When begin fs composite create is executed for artifact_type "legacy_named" with the dedicated name "daily digest"
    Then the begin outcome is successful
    And the begin outcome context_text contains "daily digest"

  Scenario: a value smuggled into the generic bag under the declared name is not a second home
    Given a composite begin fs hearth seeded with a machine declaring the pre-migration name field
    When begin fs composite create is executed for artifact_type "legacy_named" with the dedicated name "daily digest" and field "workflow_name" = "smuggled"
    Then the begin outcome is successful
    And the begin outcome ArtifactCreation status has no field "workflow_name"
    And the begin outcome context_text does not contain "smuggled"
