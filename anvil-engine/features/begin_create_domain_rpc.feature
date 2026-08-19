Feature: Begin RPC creates a parent-less domain-machine artifact
  The engine's begin RPC drives ANY registry-resolved machine. A
  knowledge_lifecycle create (no parent) scaffolds into knowledge/<id>/ with a
  machine-derived status.yaml (kind: knowledge_lifecycle, state: ingesting), no
  spec.md placeholder, and ZERO "track" literals on the path. The track create
  flow stays intact (behavior-preservation oracle lives in begin_event_routing).
  (S1 engine half / AC1 end-to-end.)

  Scenario: knowledge_lifecycle create scaffolds into knowledge/ with machine-derived status
    Given a hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "alpha topic" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path starts with "knowledge/"
    And the begin RPC response track_path file "status.yaml" contains "kind: knowledge_lifecycle"
    And the begin RPC response track_path file "status.yaml" contains "state: ingesting"
    And the begin RPC response track_path has no "spec.md" file

  Scenario: directory-less lore_query create scaffolds into runs with machine-derived status
    Given a hearth seeded with the lore_query run-backed machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "lore_query" artifact named "ask about plans" with no parent
    Then the begin RPC response has non-empty "track_path"
    And the begin RPC response track_path starts with "runs/"
    And the begin RPC response track_path contains "_lore_query_ask_about_plans"
    And the begin RPC response track_path file "status.yaml" contains "kind: lore_query"
    And the begin RPC response track_path file "status.yaml" contains "state: answering"
    And the begin RPC response "context_text" is exactly "LORE-QUERY-RUN-BACKED-HOOK\n\nUse the routed Lore query and return a sourced answer.\n"
    And the begin RPC response "intent" is exactly "Answer the user's Lore question from linked topic evidence."
    And the begin RPC response "expected_output" is exactly "A sourced answer that names the evidence used."
    When the complete RPC is called on the begin RPC response artifact with satisfaction ""
    Then the complete RPC response new_state is "completed"
    And the complete RPC response has 0 warnings
    And no registry markdown file contains the begin RPC response track_path
