Feature: Begin create validates every machine required field before creating

  Scenario: create-flow missing a required name fails before parent reads or artifact creation
    Given a composite begin fs hearth seeded with an active proposal "20260411T2021_anvil_workflow_engine"
    When begin fs composite create is executed for artifact_type "track" parent "20260411T2021_anvil_workflow_engine" track_name "" approver "Nick"
    Then the begin outcome is a MissingRequiredField error for "name"
    And the handler emitted no events
    And no begin artifact status exists under "tracks"

  Scenario: create-flow missing every required track field reports every missing field
    Given a composite begin fs hearth seeded with an active proposal "20260411T2021_anvil_workflow_engine"
    When begin fs composite create is executed for artifact_type "track" parent "" track_name "" approver ""
    Then the begin outcome is a MissingRequiredField error for "name,parent_id,approver"
    And the handler emitted no events
    And no begin artifact status exists under "tracks"
