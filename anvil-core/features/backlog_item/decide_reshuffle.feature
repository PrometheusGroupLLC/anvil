Feature: K8 DECIDE reshuffle (orchestrator proposes, only Nick commits)
  propose_reshuffle is orchestrator-only over unique ranked candidate/ready
  items and moves no position. commit_reshuffle/reject_reshuffle are
  nick_shape-only over one unresolved, non-stale proposal. Commit appends every
  rank_committed before any approved position bytes; reject appends
  rank_rejected and changes no position. No position file precedes its
  authorization entry (§1.10 / NICK-GATE DECIDE).

  Background:
    Given a backlog fixture

  @mutations
  Scenario: A proposal appends rank_proposed to every item and moves no position
    Given a source backlog item in state "candidate" from provenance "ranked_organ"
    When the backlog operation "propose_reshuffle" is attempted with role "orchestrator"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, rank_recomputed, rank_proposed"

  @mutations
  Scenario: Nick commit appends rank_committed before writing any position
    Given a source backlog item in state "candidate" from provenance "open_proposal"
    When the backlog operation "commit_reshuffle" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, rank_recomputed, rank_proposed, rank_committed"

  @mutations
  Scenario: Nick reject appends rank_rejected and changes no position
    Given a source backlog item in state "candidate" from provenance "open_proposal"
    When the backlog operation "reject_reshuffle" is attempted with role "nick_shape"
    Then the backlog operation succeeds
    And the backlog history kinds are "created, shape_edited, rank_recomputed, rank_proposed, rank_rejected"

  @mutations
  Scenario Outline: Illegal proposers and resolutions are refused
    Given a source backlog item in state "candidate" from provenance "<provenance>"
    When the backlog operation "<operation>" is attempted with role "<role>"
    Then the backlog operation is rejected because "<reason>"
    And no backlog residue remains under "backlog_items"

    Examples:
      | provenance     | operation         | role         | reason      |
      | ranked_organ   | propose_reshuffle | nick_shape   | role        |
      | ranked_organ   | commit_reshuffle  | orchestrator | role        |
      | ranked_organ   | commit_reshuffle  | nick_shape   | proposal    |
      | resolved_proposal | commit_reshuffle | nick_shape  | resolved    |
      | stale_proposal | commit_reshuffle  | nick_shape   | stale       |
      | cross_organ_proposal | propose_reshuffle | orchestrator | organ  |

  @mutations
  Scenario: A crashed commit rolls positions forward with no position before its authorization
    Given a source backlog item in state "candidate" from provenance "commit_crashed_after_authorization"
    When the backlog item is serialized and reloaded
    Then the backlog operation succeeds
