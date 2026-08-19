Feature: The generated-wire boundary B-i.2 must not cross

  B-i renames INTERNAL identifiers only. B-i.2's problem is that the identifier
  namespace is SHARED: five names denote both an `anvil-core` type/field and a
  prost-generated wire type/field, so a rename applied to the whole namespace
  COMPILES and quietly performs B-w.1's job inside B-i.

  `plan-preflight/plan-errata.md` ERRATUM-2 ledgers that wire subset into B-w.1
  under NG-PROTO-VNEXT: eleven `Playbook*` messages and NINE `playbook`-named field
  declarations over SEVEN distinct names. (The ledger first said "three" — that is
  the `artifact_kind(s)` FAMILY, not the field POPULATION; round 6 found the gap and
  `execution_route` x3, `per_artifact_kind`, `by_artifact_kind` and `playbook_step_turns`
  were added.) A ledger nothing checks is a comment. This feature is the check.

  The pins are three-legged on purpose, because each leg catches what the others
  cannot. Leg 1 names every ledgered type in a TYPE position and writes every
  ledgered field in a FIELD position, so a whole-tree rename that sweeps the
  proto AND every use site is still a compile error here. Leg 2 pins the names
  and TAGS in `proto/anvil.proto` itself, so a rename that never touches Rust
  still reds. Leg 3 pins the POPULATION, so the ledger cannot quietly become a
  sample — the round-5 lesson that a pin list a new member is not forced into is
  a pin list about a subset.

  The fourth scenario guards the opposite direction. B-i.2b renames core-domain
  Rust fields whose persisted JSONL key is a hand-written string literal, so the
  field and the key are independent. It writes a real record through the real
  filesystem adapter and reads the byte on disk.

  Scenario: A ledgered wire message cannot be renamed inside B-i
    Given the anvil proto contract source
    Then every ledgered wire message is still generated under its pinned name
    And the proto declares each ledgered message at its pinned line

  Scenario: A ledgered wire field cannot be renamed or renumbered inside B-i
    Given the anvil proto contract source
    Then the proto declares each ledgered wire field with its pinned name and tag

  Scenario: The ledgered wire set is the whole population, not a sample
    Given the anvil proto contract source
    Then the Playbook-named message set in the proto is exactly the ledgered population
    And the playbook-named field set in the proto is exactly the ledgered population
    And no message or field in the proto still carries the retired token

  Scenario: Renaming the core field does not move the persisted JSONL key
    Given an activity-log sink at a fresh hearth
    When a command turn for artifact kind "track" is recorded
    Then the persisted line still carries the key "artifact_kind" with value "track"
