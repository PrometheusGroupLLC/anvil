Feature: Canonical playbook language at the MCP surface
  The `retire_the_workflow_term` track migrates the MCP shim to the canonical
  vocabulary — prose AND wire. A reusable definition is a **playbook**, one
  execution of it is a **playbook run**, the kind routing selects is an
  **artifact kind** / **driven kind**, the engine-vs-fallback discriminator is
  an **execution route**, and the router's coarse input is a **routing hint**.

  The wire moves with the prose. There is exactly ONE name for each of these:
  no accepted-argument alias, no dual-emitted response key, no legacy error
  code kept alive beside its canonical twin. A retired name is not merely
  undocumented — the real server REFUSES it, which is what makes the rename
  falsifiable instead of asserted.

  Every assertion below reads the REAL stdio MCP server: the shim binary is
  spawned against a real anvil-engine over a real hearth, and the evidence is
  `tools/list`, a real `anvil_orchestrate` resume response, a real `amend` call
  that carries `kind: playbook`, and a real `amend` call that carries the
  retired `kind: workflow`. Reading or grepping `src/main.rs` is not acceptance
  evidence.

  Scenario: An agent discovers and resumes an Anvil run
    Given a playbook run is already open for the conversation
    When the agent lists the Anvil tools and asks to resume
    Then tool guidance calls the definition a playbook
    And calls the open execution a playbook run
    And the guidance still identifies the correct continuation action
    And no advertised argument or wire value says workflow
    And the canonical amendment kind is accepted and the retired one is refused
