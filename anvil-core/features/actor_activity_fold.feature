Feature: Actor activity folds per-actor begin-markers across artifacts
  The ActorActivity fold is the LOCAL-DASHBOARD read-side that rolls up
  per-actor activity from the `activity:` begin-markers each artifact's
  status.yaml carries. It is the local-only counterpart to the salted
  distinct-actor count: it returns the RAW actor NAMES recorded in the
  begin-markers, served only to the on-machine dashboard. Each begin-marker
  contributes one to its actor's begin_count, contributes its artifact's kind
  to that actor's artifact_kinds set, and advances last_active to the latest
  timestamp seen. Results are ordered by descending begin_count, then ascending
  actor name; each actor's artifact_kinds is ordered ascending.

  Scenario: begin-markers across artifacts fold per actor with counts, kinds, and last-active
    Given an actor activity hearth with artifact begin-markers:
      | artifact_kind | actor           | state    | at                   |
      | track         | Kemalism-506603 | spec     | 2026-06-17T05:41:36Z |
      | track         | Kemalism-506603 | plan     | 2026-06-17T06:10:00Z |
      | milestone     | Kemalism-506603 | drafting | 2026-06-17T07:00:00Z |
      | track         | Borges-101010   | spec     | 2026-06-16T09:00:00Z |
    When the actor activity fold is computed
    Then the actor activity entry for actor "Kemalism-506603" has begin count 3
    And the actor activity entry for actor "Kemalism-506603" has last active "2026-06-17T07:00:00Z"
    And the actor activity entry for actor "Kemalism-506603" has playbook kinds "milestone,track"
    And the actor activity entry for actor "Borges-101010" has begin count 1
    And the actor activity entry for actor "Borges-101010" has playbook kinds "track"

  Scenario: entries are ordered by descending begin count then actor name
    Given an actor activity hearth with artifact begin-markers:
      | artifact_kind | actor    | state | at                   |
      | track         | Zara-001 | spec  | 2026-06-17T01:00:00Z |
      | track         | Aldo-002 | spec  | 2026-06-17T01:00:00Z |
      | track         | Aldo-002 | plan  | 2026-06-17T02:00:00Z |
    When the actor activity fold is computed
    Then the actor activity result order is "Aldo-002,Zara-001"

  Scenario: an empty hearth folds to no actors
    Given an actor activity hearth with no begin-markers
    When the actor activity fold is computed
    Then the actor activity result has no entries

  Scenario: begin-markers across two hearths union per actor
    Given an actor activity primary hearth with artifact begin-markers:
      | artifact_kind | actor           | state | at                   |
      | track         | Kemalism-506603 | spec  | 2026-06-16T05:00:00Z |
    And an actor activity secondary hearth with artifact begin-markers:
      | artifact_kind | actor           | state    | at                   |
      | milestone     | Kemalism-506603 | drafting | 2026-06-17T05:00:00Z |
    When the actor activity fold is computed across all hearths
    Then the actor activity entry for actor "Kemalism-506603" has begin count 2
    And the actor activity entry for actor "Kemalism-506603" has last active "2026-06-17T05:00:00Z"
    And the actor activity entry for actor "Kemalism-506603" has playbook kinds "milestone,track"
