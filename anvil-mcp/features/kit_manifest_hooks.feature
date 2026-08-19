Feature: Kit manifest hooks block declares the harness-agnostic gate policy
  The kit manifest's `hooks` block declares the gate POLICY only: a non-empty
  `gate_query` (the open-begin-session query an installer consults) and a
  `hard_enforce` BOOLEAN gate flag. Foundry's manifest schema types `hard_enforce`
  as a boolean (install rejects a sequence), so the manifest carries `false` and
  the engine safe-defaults to no HARD pre-mutation gate (everything is SOFT-warn).
  The hooks themselves are discovered from the playbooks the kit declares in
  `playbooks.definitions`.

  Background:
    Given the kit manifest at "kit/foundry-manifest.json" is loaded

  Scenario: hooks.gate_query is non-empty
    Then the manifest hooks gate_query is non-empty

  Scenario: hooks.gate_query is the open-begin-session query
    Then the manifest field "hooks.gate_query" equals "begin_adoption_status"

  Scenario: hooks.hard_enforce is a boolean gate flag (Foundry schema)
    Then the manifest field "hooks.hard_enforce" equals "false"
