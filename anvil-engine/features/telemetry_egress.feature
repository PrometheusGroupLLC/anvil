Feature: Anvil emits unified content-free telemetry rollups
  Anvil records a small, CONTENT-FREE set of routing-plane events through the
  cross-kit foundry-kit-telemetry contract as kit_id "anvil", appended to
  <home>/.anvil/anvil-telemetry.jsonl (the same ~/.anvil rendezvous dir the
  engine.json record and router.json already live under). Every categorical label
  and field name is a safe token; every measure is numeric; every identifier is a
  salted hash — never a raw id, message, or context. The fleet emitter then rolls
  these up alongside the other kits into a validating rollup Envelope.

  # ── (a) A route decision writes a route_decision row ──
  Scenario: a route decision writes a content-free route_decision row
    Given a throwaway anvil home for telemetry
    When a route decision is recorded for actor "user:alice@corp.example" with outcome "routed" target tier "kiln" confidence "0.82" and latency "37"
    Then the anvil telemetry file contains a "route_decision" row

  # ── (b) The row is content-free: raw id absent, only its hash + safe tokens + numerics ──
  Scenario: the route_decision row carries the hashed actor, never the raw id
    Given a throwaway anvil home for telemetry
    When a route decision is recorded for actor "user:alice@corp.example" with outcome "routed" target tier "kiln" confidence "0.82" and latency "37"
    Then the raw actor id "user:alice@corp.example" does not appear in the anvil telemetry file
    And the salted actor hash appears in the anvil telemetry file

  # ── (c) The recorded rows roll up into a validating Envelope ──
  Scenario: recorded rows roll up into a validating envelope
    Given a throwaway anvil home for telemetry
    When a route decision is recorded for actor "user:bob@corp.example" with outcome "abstained" target tier "none" confidence "0.10" and latency "5"
    And an abstention is recorded with reason "router_abstain"
    And a ws session is recorded with duration "1200" and close reason "peer_close"
    Then a telemetry rollup over the full window yields a validating envelope
    And the rollup envelope kit id is "anvil"
