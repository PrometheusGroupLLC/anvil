Feature: Amendment schema validation — spec kind (AC-1, AC-2)

  # The per-kind content-element schema rejects ops that are not in the schema,
  # target an undeclared element ID, or mint a duplicate ID — each with a typed
  # error code. validate_op is a pure function over (schema, element set, op);
  # no file or prose input. The element set is a flat list of declared elements
  # (KD-1); each op names its target by author-assigned stable ID (KD-4, AC-2).

  Scenario: a legal op on a declared element validates OK
    Given a spec base document with elements:
      | id  | kind                 | body                  |
      | R1  | requirement          | The system shall X.   |
      | AC1 | acceptance_criterion | X is observable.      |
    When validate_op is called for a "revise" op targeting "AC1"
    Then validation succeeds

  Scenario: an op whose kind is not in the spec schema is rejected
    # overview is a singleton accepting Revise only; Retire is not legal for it,
    # and no spec element kind accepts an op kind absent from the schema.
    Given a spec base document with elements:
      | id       | kind     | body              |
      | overview | overview | This spec covers… |
    When validate_op is called for a "retire" op targeting "overview"
    Then validation fails with code "amendment_op_not_in_schema"

  Scenario: an op targeting an undeclared element ID is rejected
    Given a spec base document with elements:
      | id  | kind        | body                |
      | R1  | requirement | The system shall X. |
    When validate_op is called for a "revise" op targeting "R99"
    Then validation fails with code "amendment_unknown_element"

  Scenario: an Add minting a duplicate element ID is rejected
    Given a spec base document with elements:
      | id  | kind                 | body             |
      | AC1 | acceptance_criterion | X is observable. |
    When validate_op is called for an "add" op minting "AC1" of kind "acceptance_criterion" anchored "at_end"
    Then validation fails with code "amendment_duplicate_element_id"
