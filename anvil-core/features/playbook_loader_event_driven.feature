Feature: Playbook loader parses the event/step/queue-driven machine schema (#34)

  # Scope: some registered playbooks (e.g. import_transaction_history,
  # extract_document) use an EVENT/STEP-DRIVEN schema instead of the standard
  # transition-graph schema. Their top-level keys include `name`, `version`,
  # `anvil_kind`, `trigger`, `mcp_tool_dependencies`, and `steps`; their states
  # carry an `on:` event map and a `terminal:` flag instead of (or in addition
  # to) the standard transition graph. They have no standard `transitions:`
  # list, so they are queue/event-driven, not begin/complete-driven.
  #
  # Before #34 these machines failed to parse under #[serde(deny_unknown_fields)]
  # and were dropped into invalid_artifacts — excluded from the registry,
  # catalog, routing, and the dashboard. The loader must now accept this
  # legitimate variant (mapping anvil_kind -> kind, accepting the event/step
  # fields as optional) WITHOUT relaxing rejection of genuinely-unknown keys
  # and WITHOUT changing how the standard transition-graph schema validates.

  Scenario: an event/step-driven machine (anvil_kind + trigger + steps + state on:) loads as a valid machine
    Given a playbook machine.yaml with content:
      """
      name: import_transaction_history
      version: "0.1.0"
      anvil_kind: import_transaction_history
      description: Workflow-driven ingestion of a historical transaction CSV.
      trigger:
        kind: pending_queue
        poll_tool: list_pending_csv_imports
      mcp_tool_dependencies:
        - tool: list_pending_csv_imports
          purpose: discover import_ids awaiting workflow execution
          side_effects: read-only
        - tool: read_transaction_csv
          purpose: retrieve decrypted raw CSV bytes for an import_id
          side_effects: returns plaintext bytes across MCP boundary
          privacy_note: Raw CSV bytes cross the MCP boundary as plaintext.
      steps:
        - id: list_pending
          kind: mcp_call
          tool: list_pending_csv_imports
          description: Find an import_id awaiting workflow execution.
          next: read_csv
        - id: read_csv
          kind: mcp_call
          tool: read_transaction_csv
          description: Retrieve plaintext CSV bytes for the import_id.
          inputs:
            import_id: from list_pending result
          next: finish
        - id: finish
          kind: llm_turn
          prompt: steps/05_handoff_to_user.md
          description: Hand the proposed results off to the user.
          terminal: true
      states:
        - name: pending
          is_terminal: false
          on:
            WorkflowStarted: reading_csv
          hooks_by_role:
            doer: pending.md
          measurement_by_role:
            doer:
              intent: Poll list_pending_csv_imports and surface an import_id.
              expected_output: An import_id is identified.
        - name: reading_csv
          is_terminal: false
          on:
            CsvReadSucceeded: completed
            CsvReadFailed: failed
          hooks_by_role:
            doer: reading_csv.md
        - name: completed
          is_terminal: true
          hooks_by_role:
            doer: terminal.md
        - name: failed
          is_terminal: true
          hooks_by_role:
            doer: terminal.md
      """
    When the playbook loader parses the file with artifact id "import_transaction_history" and hook files "[\"pending.md\", \"reading_csv.md\", \"terminal.md\"]"
    Then the parse succeeds
    And the loaded playbook kind is "import_transaction_history"
    And the loaded playbook description is "Workflow-driven ingestion of a historical transaction CSV."
    And the loaded playbook has 4 states
    And the loaded playbook has 0 transitions
    And the loaded playbook route triggers are empty

  Scenario: an event/step-driven machine with no standard transitions is exempt from contiguity
    # The machine above has an empty transitions list, so the begin/complete
    # transition lifecycle never drives it. #33's empty-transitions exemption
    # must cover it: it loads as a valid registry entry, never rejected as a
    # non-terminal dead-end.
    Given a contiguity fixture machine with states:
      | name        | is_terminal |
      | pending     | false       |
      | reading_csv | false       |
      | completed   | true        |
      | failed      | true        |
    When validate_contiguity is called on the fixture
    Then the contiguity result is Ok

  Scenario: a hybrid machine carrying BOTH event fields and a standard transition graph loads
    # extract_document declares anvil_kind + trigger + steps AND a standard
    # roles/transitions graph. Both schemas must coexist in one machine.
    Given a playbook machine.yaml with content:
      """
      name: extract_document
      version: "0.1.0"
      anvil_kind: extract_document
      description: Workflow-driven field extraction from a document blob.
      trigger:
        kind: pending_queue
        poll_tool: list_pending_extractions
      mcp_tool_dependencies:
        - tool: list_pending_extractions
          purpose: discover documents awaiting extraction
          side_effects: read-only
      steps:
        - id: list_pending
          kind: mcp_call
          tool: list_pending_extractions
          description: Find a document_id awaiting extraction.
          next: read_blob
        - id: read_blob
          kind: mcp_call
          tool: read_document_blob
          description: Retrieve decrypted blob bytes for extraction.
          next: signal_complete
        - id: signal_complete
          kind: mcp_call
          tool: mark_extraction_complete
          description: Signal extraction completion or failure.
          terminal: true
      roles:
        - doer
      states:
        - name: pending
          role_filters: [doer_actionable]
          registry_section: ""
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hooks_by_role:
            doer: pending.md
        - name: extracting
          role_filters: [doer_actionable]
          registry_section: ""
          projection_targets: []
          is_review_gate: false
          is_terminal: false
          hooks_by_role:
            doer: extracting.md
        - name: accepted
          role_filters: []
          registry_section: ""
          projection_targets: []
          is_review_gate: false
          is_terminal: true
          hooks_by_role:
            doer: accepted.md
        - name: failed
          role_filters: []
          registry_section: ""
          projection_targets: []
          is_review_gate: false
          is_terminal: true
          hooks_by_role:
            doer: failed.md
      transitions:
        - from_state: pending
          to_state: extracting
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
        - from_state: pending
          to_state: failed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
        - from_state: extracting
          to_state: accepted
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
        - from_state: extracting
          to_state: failed
          required_role: doer
          required_satisfaction: ~
          requires_approver: false
      """
    When the playbook loader parses the file with artifact id "extract_document" and hook files "[\"pending.md\", \"extracting.md\", \"accepted.md\", \"failed.md\"]"
    Then the parse succeeds
    And the loaded playbook kind is "extract_document"
    And the loaded playbook has 4 states
    And the loaded playbook has 4 transitions

  Scenario: an event-driven machine declaring a top-level outcome_predicate loads (measurement enforcement)
    # Regression: the lenient mirror carries deny_unknown_fields, so before it
    # named `outcome_predicate` a declared predicate block failed to parse on
    # BOTH schemas — and into_machine hard-coded outcome_predicate: None, so even
    # stripping the block dropped the machine under measurement enforcement
    # (which requires a non-blank terminal_state). The mirror must accept the
    # top-level predicate exactly like the standard schema so event-driven kinds
    # (import_transaction_history, extract_document) can register under
    # ANVIL_ENFORCE_MEASUREMENT_DEFINITION instead of silently dropping.
    Given a playbook machine.yaml with content:
      """
      name: import_transaction_history
      version: "0.1.0"
      anvil_kind: import_transaction_history
      description: Workflow-driven ingestion of a historical transaction CSV.
      trigger:
        kind: pending_queue
        poll_tool: list_pending_csv_imports
      states:
        - name: pending
          is_terminal: false
          on:
            WorkflowStarted: completed
          hooks_by_role:
            doer: pending.md
        - name: completed
          is_terminal: true
          hooks_by_role:
            doer: terminal.md
      outcome_predicate:
        terminal_state: completed
        check: The import_id's transactions are committed and mark_import_complete was called with status=completed.
      """
    When the playbook loader parses the file with artifact id "import_transaction_history" and hook files "[\"pending.md\", \"terminal.md\"]"
    Then the parse succeeds
    And the loaded playbook kind is "import_transaction_history"
    And the loaded playbook has 2 states
    And the loaded playbook has 0 transitions

  Scenario: a genuinely unknown top-level key is still rejected on an event-driven machine
    # Relaxing the schema for the event variant must NOT blanket-disable
    # deny_unknown_fields: a key that belongs to neither schema still fails.
    Given a playbook machine.yaml with content:
      """
      name: bad_event
      anvil_kind: bad_event
      description: An event-driven machine with a bogus key.
      trigger:
        kind: pending_queue
        poll_tool: some_tool
      totally_unknown_key: 42
      states:
        - name: pending
          is_terminal: false
          on:
            WorkflowStarted: done
        - name: done
          is_terminal: true
      """
    When the playbook loader parses the file with artifact id "bad-event" and no hook files
    Then the parse fails with error code "playbook_yaml_parse_error"

  Scenario: a machine missing kind AND anvil_kind still reports missing required key
    # The standard schema's strictness for the required `kind` key is preserved:
    # anvil_kind is an accepted ALIAS, not a removal of the requirement.
    Given a playbook machine.yaml with content:
      """
      name: no_kind
      description: A machine with neither kind nor anvil_kind.
      states:
        - name: pending
          is_terminal: false
          on:
            WorkflowStarted: done
        - name: done
          is_terminal: true
      """
    When the playbook loader parses the file with artifact id "no-kind" and no hook files
    Then the parse fails with error code "playbook_missing_required_key"
    And the error carries key_path "kind"
