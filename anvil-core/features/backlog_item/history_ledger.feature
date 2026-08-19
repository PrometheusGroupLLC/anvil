Feature: K8 append-only history ledger, strict reconciliation, and recovery
  History is append-only under one global sequence beginning with created(seq:0)
  (§1, Task 4). Named mutations contribute exactly one entry; evaluation may
  contribute rank_recomputed then state_change; advancements contribute the
  printed-role state_change then the private engine_auto rank_recomputed. Every
  public read recovers interrupted K8 transactions under the resolved-hearth
  lock, and third-value conflicts are refused rather than overwritten.

  Background:
    Given a backlog fixture

  @storage
  Scenario: Genesis writes exactly created(seq:0) and nothing else
    When backlog genesis is attempted with input "valid_candidate"
    Then the backlog operation succeeds
    And the backlog history kinds are "created"

  @storage
  Scenario Outline: Each named producer appends exactly its one history kind
    Given a source backlog item in state "<from>" from provenance "<provenance>"
    When the backlog operation "<operation>" is attempted with role "<role>"
    Then the backlog operation succeeds
    And the backlog history kinds are "<kinds>"

    Examples:
      | from       | provenance         | operation              | role         | kinds                                   |
      | candidate  | shapeable          | shape_edit             | organ_loop   | created, shape_edited                   |
      | candidate  | all_inputs_effort  | recompute_rank         | organ_loop   | created, shape_edited, rank_recomputed  |
      | ready      | ready_shaped       | stamp_execution_binding| track_driver | created, state_change, binding_stamped  |
      | in_flight  | bound              | record_outcome_signoff | nick_shape   | created, state_change, binding_stamped, state_change, signoff |
      | candidate  | shapeable          | veto_age_out           | nick_shape   | created, veto                           |

  @storage
  Scenario: The reshuffle producers append rank_proposed and rank_committed
    Given a source backlog item in state "candidate" from provenance "ranked_organ"
    And the backlog operation "propose_reshuffle" is attempted with role "orchestrator"
    When the backlog operation "commit_reshuffle" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, rank_recomputed, rank_proposed, rank_committed"

  @storage
  Scenario Outline: Strict corruption of the ledger fails loudly on the next read
    Given a source backlog item in state "candidate" from provenance "<corruption>"
    When the backlog item is serialized and reloaded
    Then the backlog operation is rejected because "<reason>"

    Examples:
      | corruption               | reason                    |
      | duplicate_id             | duplicate                 |
      | missing_history          | history                   |
      | noncontiguous_history    | contiguous                |
      | genesis_state_change     | genesis                   |
      | ledger_mirror_mismatch   | mismatch                  |
      | duplicate_event          | duplicate transition      |
      | conflicting_history_seq  | sequence                  |
      | state_change_ledger_skew | mismatch                  |
      | malformed_status_yaml    | status                    |

  @storage
  Scenario Outline: An interrupted transaction is recovered on the first public read
    Given a source backlog item in state "candidate" from provenance "interrupted_<phase>"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds

    Examples:
      | phase           |
      | prepared        |
      | applying        |
      | committed       |

  @storage
  Scenario Outline: An interrupted transaction is recovered when a first-class scan is the first read
    Given a source backlog item in state "candidate" from provenance "interrupted_applying"
    When the backlog hearth is first read through "<surface>"
    Then the backlog operation succeeds

    Examples:
      | surface  |
      | catalog  |
      | checkin  |
      | describe |

  @storage
  Scenario Outline: A crash at each per-effect point recovers idempotently on the first read
    Given a source backlog item in state "candidate" from provenance "crash_<effect>"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds

    Examples:
      | effect         |
      | after_publish  |
      | after_status   |
      | after_item     |
      | after_history  |
      | after_event    |
      | after_registry |
      | after_cleanup  |

  @storage
  Scenario: The prepared journal carries exact strict-rendered status.yaml bytes before applying
    Given a source backlog item in state "candidate" from provenance "actor_status_rendered"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds
    And the rendered actor status is byte-exact

  @storage
  Scenario: An existing status.yaml that cannot carry an actor upsert refuses the governed advance
    Given a source backlog item in state "candidate" from provenance "unusable_actors_status"
    When a backlog transition from "candidate" to "ready" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "status.yaml"

  @storage
  Scenario: A third-value conflict is refused rather than rolled forward
    Given a source backlog item in state "candidate" from provenance "interrupted_third_value_conflict"
    When the backlog item is serialized and reloaded
    Then the backlog operation is rejected because "conflict"

  @storage
  Scenario Outline: A per-effect third-value conflict is refused rather than overwritten
    Given a source backlog item in state "candidate" from provenance "conflict_<target_class>"
    When the backlog item is serialized and reloaded
    Then the backlog operation is rejected because "conflict"

    Examples:
      | target_class |
      | status       |
      | item         |
      | history      |
      | event        |
      | registry     |
      | publish      |

  @storage
  Scenario: A stale prepared capability is refused with no new write
    Given a source backlog item in state "candidate" from provenance "stale_prepared_capability"
    When a backlog transition from "candidate" to "ready" is attempted with role "organ_loop"
    Then the backlog operation is rejected because "stale"
    And no backlog residue remains under "backlog_items"
