Feature: ActorActivity RPC returns raw actor names from begin-markers (LOCAL-ONLY)
  The read-only ActorActivity RPC is the local-dashboard counterpart to the
  salted distinct-actor count. It folds the `activity:` begin-markers each
  artifact's status.yaml carries into per-actor activity and returns the RAW
  actor NAMES, served over the on-machine gRPC surface and the loopback /ws
  bridge only — never telemetry. The gRPC RPC and the /ws method fold the SAME
  core path, so the two surfaces can never diverge. Entries are ordered by
  descending begin_count then ascending actor name.

  Scenario: ActorActivity over gRPC returns named actors folded from begin-markers
    Given an actor activity engine hearth with artifacts:
      | artifact_id | artifact_kind | actor           | state    | at                   |
      | track-alpha | track         | Kemalism-506603 | spec     | 2026-06-17T05:41:36Z |
      | track-alpha | track         | Kemalism-506603 | plan     | 2026-06-17T06:10:00Z |
      | ms-beta     | milestone     | Kemalism-506603 | drafting | 2026-06-17T07:00:00Z |
      | track-gamma | track         | Borges-101010   | spec     | 2026-06-16T09:00:00Z |
    And the engine is started with that hearth
    When the ActorActivity RPC is called
    Then the actor activity RPC entry for actor "Kemalism-506603" has begin count 3
    And the actor activity RPC entry for actor "Kemalism-506603" has last active "2026-06-17T07:00:00Z"
    And the actor activity RPC entry for actor "Kemalism-506603" has playbook kinds "milestone,track"
    And the actor activity RPC entry for actor "Borges-101010" has begin count 1

  Scenario: ActorActivity over /ws returns named actors with begin_count as a JSON number
    Given an actor activity engine hearth with artifacts:
      | artifact_id | artifact_kind | actor           | state | at                   |
      | track-alpha | track         | Kemalism-506603 | spec  | 2026-06-17T05:41:36Z |
      | track-alpha | track         | Kemalism-506603 | plan  | 2026-06-17T06:10:00Z |
      | track-gamma | track         | Borges-101010   | spec  | 2026-06-16T09:00:00Z |
    And the engine is started with that hearth
    When an actor_activity JSON-RPC request is sent over /ws with hearth_path ""
    Then the /ws actor activity entry for actor "Kemalism-506603" has begin count 2
    And the /ws actor activity entry for actor "Borges-101010" has begin count 1
    And the /ws actor activity begin_count for actor "Kemalism-506603" is a JSON number

  Scenario: ActorActivity across all hearths unions per actor
    Given an actor activity primary hearth with artifacts:
      | artifact_id | artifact_kind | actor           | state | at                   |
      | track-alpha | track         | Kemalism-506603 | spec  | 2026-06-16T05:00:00Z |
    And an actor activity secondary hearth with artifacts:
      | artifact_id | artifact_kind | actor           | state    | at                   |
      | ms-beta     | milestone     | Kemalism-506603 | drafting | 2026-06-17T05:00:00Z |
    And the engine is started with both permitted hearths
    When the ActorActivity RPC is called across all hearths
    Then the actor activity RPC entry for actor "Kemalism-506603" has begin count 2
    And the actor activity RPC entry for actor "Kemalism-506603" has playbook kinds "milestone,track"
    And the actor activity RPC resolved hearth is the all-hearths sentinel
    And the actor activity RPC included 2 hearths
