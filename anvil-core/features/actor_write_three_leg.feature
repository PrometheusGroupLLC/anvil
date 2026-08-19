Feature: Actor write three-leg rule
  ActorWritePort.upsert_actor_configuration enforces the uniform actor-write
  rule shared by begin and snapshot:
    1. add-if-absent: actor not in actors table → new entry with single-entry
       configurations list
    2. match-no-op: actor present and call's params match the latest stored
       configuration → no mutation
    3. mismatch-append: actor present but call's params differ → append a new
       configuration entry; existing entries are preserved

  These scenarios drive the port directly via TestActorWriteAdapter. The
  filesystem adapter and command-handler integration are exercised in
  other feature files.

  Scenario: add-if-absent — fresh actors table gains a new entry
    Given an actor write adapter with no existing actors at "tracks/test"
    When upsert_actor_configuration is called on "tracks/test" with name "Sarigue-938624", model "claude-opus-4-7", provider "anthropic"
    Then the actor write adapter has a single configurations entry for "Sarigue-938624" at "tracks/test"
    And the actor write adapter's stored configuration for "Sarigue-938624" has model "claude-opus-4-7"

  Scenario: match-no-op — re-upsert with identical params leaves configurations untouched
    Given an actor write adapter with actor "Sarigue-938624" at "tracks/test" with model "claude-opus-4-7", provider "anthropic"
    When upsert_actor_configuration is called on "tracks/test" with name "Sarigue-938624", model "claude-opus-4-7", provider "anthropic"
    Then the actor write adapter has a single configurations entry for "Sarigue-938624" at "tracks/test"

  Scenario: mismatch-append — differing params append a new configuration entry
    Given an actor write adapter with actor "Sarigue-938624" at "tracks/test" with model "claude-opus-4-7", provider "anthropic"
    When upsert_actor_configuration is called on "tracks/test" with name "Sarigue-938624", model "claude-sonnet-4-6", provider "anthropic"
    Then the actor write adapter has 2 configurations entries for "Sarigue-938624" at "tracks/test"
    And the actor write adapter's first configurations entry for "Sarigue-938624" has model "claude-opus-4-7"
    And the actor write adapter's last configurations entry for "Sarigue-938624" has model "claude-sonnet-4-6"
