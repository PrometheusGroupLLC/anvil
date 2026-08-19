Feature: HearthPlaybookRegistry — on-disk adapter resolves kind names from machine.yaml files

  # AC3: adapter-level scenarios for HearthPlaybookRegistry.
  #
  # The adapter reads workflows/*/machine.yaml from a hearth directory at
  # construction time, keying each loaded PlaybookMachine by its `kind` field.
  # machine_for(kind) returns a reference into the adapter's internal map.
  #
  # Per-call construction IS the always-reload mechanism — the engine constructs
  # a fresh HearthPlaybookRegistry on each describe RPC call; no internal cache
  # is maintained across construction boundaries.
  #
  # Steps use tempfile scratch directories for hermeticity.

  Scenario: HearthPlaybookRegistry resolves kind "track" from an on-disk machine.yaml
    Given a temp hearth with a valid "track" machine.yaml at "workflows/20260422T0000_track_lifecycle/machine.yaml"
    When the hearth registry resolves kind "track"
    Then the hearth-resolved machine has kind "track"

  Scenario: HearthPlaybookRegistry resolves canonical playbook machines from playbooks
    Given a temp hearth with a valid "report" machine.yaml at "playbooks/report_lifecycle/machine.yaml"
    When the hearth registry resolves kind "report"
    Then the hearth-resolved machine has kind "report"
    And the hearth registry resolves playbook id "report_lifecycle" for kind "report"

  Scenario: HearthPlaybookRegistry preserves legacy workflow machine compatibility
    Given a temp hearth with a valid "legacy_report" machine.yaml at "workflows/legacy_report_lifecycle/machine.yaml"
    When the hearth registry resolves kind "legacy_report"
    Then the hearth-resolved machine has kind "legacy_report"
    And the hearth registry resolves playbook id "legacy_report_lifecycle" for kind "legacy_report"

  Scenario: HearthPlaybookRegistry returns None for an unknown kind
    Given a temp hearth with a valid "track" machine.yaml at "workflows/20260422T0000_track_lifecycle/machine.yaml"
    When the hearth registry resolves kind "proposal"
    Then the hearth-resolved machine is absent

  Scenario: HearthPlaybookRegistry loads all discovered kinds, not just track
    Given a temp hearth with a valid "track" machine.yaml at "workflows/20260422T0000_track_lifecycle/machine.yaml"
    And a temp hearth has a valid "milestone" machine.yaml at "workflows/20260422T0001_milestone_lifecycle/machine.yaml"
    When the hearth registry resolves kind "track"
    Then the hearth-resolved machine has kind "track"
    When the hearth registry resolves kind "milestone"
    Then the hearth-resolved machine has kind "milestone"

  Scenario: HearthPlaybookRegistry registers an event/step/queue-driven machine.yaml (#34)
    # An event-driven machine (anvil_kind + trigger + steps + per-state on:,
    # no standard transitions:) used to fail to parse under deny_unknown_fields
    # and land in invalid_artifacts. It must now load as a valid registry entry,
    # resolvable by its anvil_kind, with NO load error recorded.
    Given a temp hearth with an event-driven "import_transaction_history" machine.yaml at "workflows/20260601T1254_import_transaction_history/machine.yaml"
    When the hearth registry is constructed from the temp hearth
    Then the hearth registry construction succeeds
    And the hearth registry has no load errors
    When the hearth registry resolves kind "import_transaction_history"
    Then the hearth-resolved machine has kind "import_transaction_history"

  Scenario: HearthPlaybookRegistry surfaces a malformed machine.yaml via the load-error channel without panicking
    Given a temp hearth with a malformed machine.yaml at "workflows/20260422T0002_bad_lifecycle/machine.yaml"
    When the hearth registry is constructed from the temp hearth
    Then the hearth registry construction succeeds
    And the hearth registry has load errors
    When the hearth registry resolves kind "bad"
    Then the hearth-resolved machine is absent
