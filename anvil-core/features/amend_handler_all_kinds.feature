Feature: AmendCommandHandler records a valid op for all 8 amendment kinds (BP2)

  # Op-log-only recording is unconditional across all 8 amendment kinds: each kind
  # resolves a schema via schema_for_kind and a valid Add records. The artifact's
  # lifecycle kind/state is seeded so read_artifact_kind/read_artifact_state
  # resolve (MEDIUM-4); the "draft" state is not a machine amend edge, so
  # new_state is None.

  Scenario Outline: a valid Add records for amendment kind <kind>
    Given an in-memory amend query adapter with artifact "artifacts/a" kind "<kind>" state "draft"
    When amend Add is called for artifact "artifacts/a" kind "<kind>" document "<kind>" target_id "el-1" new_kind "<element_kind>" body "a valid body" at "2026-06-04T21:15:00Z"
    Then the amend outcome is successful
    And the amend outcome has 2 events
    And the amend outcome event 0 is OpRecorded
    And the amend outcome new_state is absent

    Examples:
      | kind       | element_kind       |
      | proposal   | slice              |
      | track      | goal               |
      | spec       | requirement        |
      | plan       | phase              |
      | milestone  | success_criterion  |
      | initiative | expectation        |
      | decision   | validity_condition |
      | learning   | evidence           |
