Feature: Live-instances fold classifies OPEN instances as LIVE or DORMANT
  Every non-terminal artifact is "open", but most open artifacts are dormant —
  a proposal sitting untouched in `active`, a track quietly `shelved`. Rendering
  every open artifact as a "live agent" is dishonest. This fold classifies each
  open instance as LIVE when EITHER its most-recent §0 event is within the live
  window of `now`, OR it carries a raw actor from a begin-marker. An instance
  satisfying neither is DORMANT — excluded from the live list and counted (not
  silently dropped) in `idle_count`. The fold also carries the per-instance
  detail the honest Atlas panel needs: current_step, action_count, artifact_dir.

  Scenario: an instance with a recent §0 event but no actor is LIVE
    Given live-instance inputs:
      | instance | kind  | state    | actor | begin_at | action_count | last_step0_at        | artifact_dir      |
      | inst_a   | track | spec     |       |          | 4            | 2026-07-09T12:00:00Z | /hearth/tracks/a  |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has an instance "inst_a"
    And the live instance "inst_a" has current_step "spec"
    And the live instance "inst_a" has action_count 4
    And the live instance "inst_a" has artifact_dir "/hearth/tracks/a"
    And the live view has idle_count 0

  Scenario: an instance with an actor but a STALE §0 event is still LIVE (actor path)
    Given live-instance inputs:
      | instance | kind  | state   | actor            | begin_at             | action_count | last_step0_at        | artifact_dir       |
      | inst_b   | track | implement | Atlas-Doer-4211 | 2026-06-01T00:00:00Z | 1            | 2026-06-01T00:05:00Z | /hearth/tracks/b   |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has an instance "inst_b"
    And the live instance "inst_b" has actor "Atlas-Doer-4211"
    And the live view has idle_count 0

  Scenario: an instance with neither a recent §0 event nor an actor is DORMANT
    Given live-instance inputs:
      | instance | kind     | state  | actor | begin_at | action_count | last_step0_at | artifact_dir          |
      | inst_c   | proposal | active |       |          | 0            |                | /hearth/proposals/c   |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has no instance "inst_c"
    And the live view has idle_count 1

  Scenario: a §0 event just past the live window is DORMANT unless it also has an actor
    Given live-instance inputs:
      | instance | kind  | state | actor | begin_at | action_count | last_step0_at        | artifact_dir      |
      | inst_d   | track | plan  |       |          | 2            | 2026-07-08T19:59:59Z | /hearth/tracks/d  |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has no instance "inst_d"
    And the live view has idle_count 1

  Scenario: a §0 event exactly at the 24h boundary is LIVE (inclusive)
    Given live-instance inputs:
      | instance | kind  | state | actor | begin_at | action_count | last_step0_at        | artifact_dir      |
      | inst_e   | track | plan  |       |          | 3            | 2026-07-08T20:00:00Z | /hearth/tracks/e  |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has an instance "inst_e"
    And the live view has idle_count 0

  Scenario: a mix of live and dormant instances is partitioned, ordered ascending by instance id
    Given live-instance inputs:
      | instance | kind     | state    | actor            | begin_at              | action_count | last_step0_at         | artifact_dir           |
      | zzz_live | track    | spec     | Atlas-Doer-4211  | 2026-07-09T10:00:00Z  | 2            | 2026-07-09T10:05:00Z  | /hearth/tracks/zzz     |
      | aaa_live | track    | spec     |                  |                       | 5            | 2026-07-09T18:00:00Z  | /hearth/tracks/aaa     |
      | mid_idle | proposal | shelved  |                  |                       | 0            |                       | /hearth/proposals/mid  |
    When the live instances are folded with now "2026-07-09T20:00:00Z"
    Then the live view has an instance "zzz_live"
    And the live view has an instance "aaa_live"
    And the live view has no instance "mid_idle"
    And the live view has idle_count 1
