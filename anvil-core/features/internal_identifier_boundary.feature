Feature: Internal playbook identifiers cross no wire surface

  B-i renames INTERNAL identifiers only. `spec.md:767` forbids a count-only
  rename: each family must carry a semantic assertion that says what the renamed
  thing MEANS, not merely that the token is gone.

  The meaning under test here is the phase boundary itself. `PlaybookLoadError`
  (was `WorkflowLoadError`) is a Rust type with no `Serialize` derive; the
  `playbook_*` strings its `code()` returns ARE the wire. In Phase A they were
  frozen byte-verbatim as `workflow_*` and this gate held them there, because a
  rename of the type that also moved a code string would have silently performed
  a Phase B-w change inside Phase B-i. THE FREEZE IS OVER: with the vocabulary
  ratified (docs/vocabulary.md), the wire moved whole — proto, RPC names,
  response keys and these codes — reader and writer in one commit, with no alias
  and no dual emission. The gate does not disappear with the freeze; it now pins
  the codes in their NEW spelling, exhaustively, so the next drift reds it. An
  error code is computed at the moment of failure and is never read back off
  disk, which is exactly why it may move now while the durable JSONL keys may
  not.

  The population is EXHAUSTIVE: one constructed variant per `code()` arm, each
  pinned to its code by name. Round 4 falsified a 3-variant sample — renaming
  `playbook_no_terminal_reachable` passed every gate this phase runs.

  `code()` has 18 arms, not 17: `hook_path_invalid` is deliberately not
  `playbook_`-prefixed, because it names a malformed path rather than a playbook
  concept. It is pinned anyway — B-i must move NO code, not merely no
  `playbook_`-prefixed one.

  Scenario: The load-error type emits exactly its pinned wire codes
    Given the playbook load-error family
    Then the unknown-hook-reference code is exactly "playbook_unknown_hook_reference"
    And the yaml-parse code is exactly "playbook_yaml_parse_error"
    And every variant emits exactly its pinned code

  Scenario: The pinned code set is the whole code space, not a sample
    Given the playbook load-error family
    Then exactly 18 codes are pinned, 17 of them playbook-prefixed

  Scenario: The hook-body port serves bodies from the canonical playbooks tree
    Given a hearth whose only definition directory is a legacy workflows dir
    When the hearth playbook registry is constructed against it
    Then the legacy directory has been renamed to the canonical playbooks directory
    And no definition is scanned from the legacy location
