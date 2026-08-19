Feature: Canonical playbook language on the engine's wire
  The engine's read surface is consumed by the Anvil kit frontend over the
  loopback JSON-RPC bridge on `/ws`, and by gRPC clients over the same port.
  Both are the SAME fold, so the method names and response keys the bridge
  emits ARE the wire contract.

  This is the wire half of the `workflow` -> `playbook` migration: a reusable
  lifecycle definition is a **playbook**, one execution of it is a **playbook
  run**, the kind routing selects is an **artifact kind**, and the
  engine-vs-fallback discriminator is an **execution route**.

  There is exactly ONE name for each of these. The retired names are not
  aliased, not dual-emitted, and not answered — a client that sends one gets a
  method-not-found error, which is what makes "the rename landed" falsifiable
  rather than asserted.

  Every assertion below reads a REAL engine process over a REAL hearth through
  a REAL WebSocket. Grepping source is not acceptance evidence.

  Scenario: The bridge answers only canonical method names
    Given a real engine is serving a hearth carrying a playbook run
    When the client calls every read method on the engine's JSON-RPC bridge
    Then every canonical read method answers
    And the retired workflow-named methods are refused

  Scenario: No response key on the read surface carries the retired noun
    Given a real engine is serving a hearth carrying a playbook run
    When the client calls every read method on the engine's JSON-RPC bridge
    Then no key anywhere in any response says workflow
    And each actor lists the artifact kinds it has begun under the canonical key
