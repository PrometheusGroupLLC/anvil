Feature: Route attributes a source-less turn to the (unattributed) bucket
  The per-turn route hook is fire-and-forget — it cannot ask an LLM to retry — so
  the route RPC NEVER errors on a missing hearth. But it must also never charge a
  source-less turn to the global anvil-hearth's own activity record (the
  corruption this change removes). When the caller supplies no hearth and the
  engine is hearth-less, the route still resolves its kind from the GLOBAL
  playbook registry, but the attribution record is written to a dedicated
  `__unattributed__` bucket under the global hearth — its own discoverable hearth
  — so the dashboard buckets it separately, never inside anvil-hearth.

  Scenario: a no-caller-hearth route resolves globally but attributes to the bucket
    Given a hearth-less engine with a global playbooks hearth carrying driven kind "daily_recap" trigger "daily recap"
    When the route RPC is called with no caller hearth and message "daily recap"
    Then the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the (unattributed) bucket under the global hearth has a routing-activity record
    And the global hearth root has no routing-activity record
