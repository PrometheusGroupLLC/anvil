Feature: Amendment — registered kinds have a non-empty schema (AC-7, Phase-11)

  # Stability freeze guard (KD-3, AC-7): schema_for_kind returns a non-empty
  # ContentElementSchema (at least one ElementKindDef) for every registered
  # amendment kind string. This proves no kind is left as a stub.

  Scenario Outline: schema_for_kind returns a non-empty schema for <kind>
    When schema_for_kind is called for kind "<kind>"
    Then the schema is present
    And the schema has at least one element kind

    Examples:
      | kind        |
      | proposal    |
      | track       |
      | spec        |
      | plan        |
      | milestone   |
      | initiative  |
      | decision    |
      | learning    |
      | playbook    |

  Scenario: schema_for_kind returns None for an unknown kind string
    When schema_for_kind is called for kind "not_a_kind"
    Then the schema is absent
