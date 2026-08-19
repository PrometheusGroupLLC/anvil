Feature: K8 done-rule (row #10, in_flight -> done)
  Row #10 lands only when a stored reading exists (engine_auto path) or the
  outcome is unmeasurable_signed with Nick sign-off (nick_shape path). Track
  completion alone is never sufficient. The external Temper reading is the one
  labeled out-of-scope producer fixture (§3); MCP uses the Nick sign-off path.

  Background:
    Given a backlog fixture

  @evaluate
  Scenario: A stored reading satisfies the done rule under engine_auto
    Given a source backlog item in state "in_flight" from provenance "bound"
    And an external Temper reading is seeded for the in-flight item
    When a backlog transition from "in_flight" to "done" is attempted with role "engine_auto"
    Then the backlog operation succeeds
    And the resulting backlog state is "done"

  @snapshot
  Scenario: Unmeasurable-signed with Nick sign-off satisfies the done rule
    Given a source backlog item in state "in_flight" from provenance "bound"
    And the backlog operation "record_outcome_signoff" is attempted with role "nick_shape"
    When a backlog transition from "in_flight" to "done" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the resulting backlog state is "done"

  @snapshot
  Scenario Outline: The done rule refuses when its predicate is unsatisfied
    Given a source backlog item in state "in_flight" from provenance "<provenance>"
    When a backlog transition from "in_flight" to "done" is attempted with role "<role>"
    Then the backlog operation is rejected because "<reason>"
    And no backlog residue remains under "backlog_items"

    Examples:
      | provenance             | role        | reason               |
      | registered_only        | engine_auto | reading              |
      | unsigned_unmeasurable  | nick_shape  | sign-off             |
      | track_completion_note  | nick_shape  | done                 |
