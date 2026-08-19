Feature: K8 rank materialization, pending edits, and advancement coherence
  Rank is all-or-nothing: when present it has position, all five inputs, and an
  explanation (§1.9). Rank-input shape_edits stage a closed pending payload;
  recompute_rank consumes them in sequence, mirrors effort, advances age, and
  materializes the complete per-organ rank. Advancements (#1/#6/#13/#14) reset
  the target age to zero and atomically rematerialize every same-organ position
  under the private derived engine_auto role — never the orchestrator/Nick role.

  Background:
    Given a backlog fixture

  @mutations
  Scenario: A rank-input shape_edit on a rankless candidate stages a pending payload
    Given a source backlog item in state "candidate" from provenance "rankless"
    When the backlog operation "shape_edit" is attempted with role "organ_loop"
    Then the backlog operation succeeds
    And the resulting backlog state is "candidate"

  @mutations
  Scenario: recompute_rank materializes the first complete rank and mirrors effort
    Given a source backlog item in state "candidate" from provenance "all_inputs_and_effort"
    When the backlog operation "recompute_rank" is attempted with role "organ_loop"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, shape_edited, rank_recomputed"

  @mutations
  Scenario: A valid pre-triage candidate is returned unranked without blocking survivors
    Given a source backlog item in state "candidate" from provenance "pre_triage_plus_survivor"
    When the backlog operation "recompute_rank" is attempted with role "organ_loop"
    Then the backlog operation succeeds
    And the item is reported in the unranked partition

  @mutations
  Scenario Outline: An incoherent rank rejects the whole re-rank
    Given a source backlog item in state "candidate" from provenance "<provenance>"
    When the backlog operation "recompute_rank" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "<reason>"
    And no backlog residue remains under "backlog_items"

    Examples:
      | provenance               | reason               |
      | incomplete_committed_rank| incomplete rank      |
      | effort_mirror_mismatch   | effort               |
      | orchestrator_position    | role                 |

  @snapshot
  Scenario: An advancement resets age to zero and rematerializes same-organ positions under engine_auto
    Given a source backlog item in state "candidate" from provenance "ranked_organ"
    When a backlog transition from "candidate" to "ready" is attempted with role "organ_loop"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, rank_recomputed, state_change, rank_recomputed"

  @snapshot
  Scenario: A rank-sensitive transition refuses unresolved rank-affecting edits
    Given a source backlog item in state "candidate" from provenance "unresolved_rank_edit"
    When a backlog transition from "candidate" to "ready" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "unresolved"
    And no backlog residue remains under "backlog_items"

  @snapshot
  Scenario: A malformed pending payload refuses a rank-sensitive transition instead of reading as resolved
    Given a source backlog item in state "candidate" from provenance "malformed_pending_payload"
    When a backlog transition from "candidate" to "ready" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "malformed"
    And no backlog residue remains under "backlog_items"

  @mutations
  Scenario: A malformed pending payload refuses a re-rank instead of dropping the caller's inputs
    Given a source backlog item in state "candidate" from provenance "malformed_pending_payload"
    When the backlog operation "recompute_rank" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "malformed"
    And no backlog residue remains under "backlog_items"

  @mutations
  Scenario: A parked item retains rank byte-equal while filtered off the queue
    Given a source backlog item in state "parked" from provenance "parked_from_7_ranked"
    When the organ queue is read for "bn_0rgan00001"
    Then the backlog operation succeeds
    And the item is reported in the unranked partition

  @mutations
  Scenario: The cross-organ view aggregates ranked and unranked partitions stably and never writes
    Given a source backlog item in state "candidate" from provenance "two_organ_ranked_and_pre_triage"
    When the cross-organ view is read
    Then the backlog operation succeeds
    And the cross-organ view aggregates both partitions
