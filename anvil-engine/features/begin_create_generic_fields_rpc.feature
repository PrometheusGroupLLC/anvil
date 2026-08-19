Feature: Begin RPC accepts any machine-declared required field via create_fields
  Over the real engine: a kit-action playbook (lore_query) whose machine declares
  generic required fields (question, requester) outside the builtin set can now be
  begun by supplying them in the proto `create_fields` map. WITHOUT them the engine
  returns FAILED_PRECONDITION carrying the `missing_required_field` substring; WITH
  them the begin succeeds and the field values persist to the created artifact's
  status.yaml. This is the dogfood proof that kit-action adoption is unblocked.

  Scenario: lore_query create without the generic fields is rejected machine-driven
    Given a hearth seeded with a lore_query machine requiring question and requester
    And the engine is started with that hearth
    When the begin RPC creates a lore_query with no create_fields
    Then the begin RPC returns gRPC status "FAILED_PRECONDITION"
    And the begin RPC error message contains "missing_required_field"

  Scenario: lore_query create supplying the generic fields persists them to status.yaml
    Given a hearth seeded with a lore_query machine requiring question and requester
    And the engine is started with that hearth
    When the begin RPC creates a lore_query with create_fields question "what is anvil" requester "Nick"
    Then the e2e artifact status.yaml contains "fields:"
    And the e2e artifact status.yaml contains "question: what is anvil"
    And the e2e artifact status.yaml contains "requester: Nick"
