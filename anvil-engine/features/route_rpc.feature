Feature: Route RPC — candidate provider for the surface LLM

  playbook_routing_layer BP2: the engine `route(input)` RPC returns the active,
  driven (registry-resolvable, register != free) candidate artifact_kinds with
  the selection metadata the surface LLM needs to choose. The surface selects;
  the engine then begins the selected machine. When the driven candidate set is
  empty, route returns a typed no_match -> candidate_playbook_intake outcome
  carrying the originating intent (never an error). The registry is constructed
  fresh per call, so machine.yaml + register edits live-reload.

  # router_relevance_ranker: the message must now be RELEVANT for the playbook to be
  # offered (the ranker no longer returns the whole granted set for a vague message).
  # A relevant single match still has legacy outcome "candidates" and the candidates
  # field still carries the full granted set with metadata — the provider contract.
  Scenario: route returns the driven candidate set with selection metadata (AC1)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "track" machine
    And the engine is started with that hearth
    When the route RPC is called with message "run the knowledge lifecycle for this transcript"
    Then the route outcome is "candidates"
    And the route candidates include kind "knowledge_lifecycle"
    And the route candidate "knowledge_lifecycle" carries a description and required_fields metadata

  Scenario: route candidate metadata prefers route.description for router selection
    Given a hearth directory with playbook files:
      | path                                      | content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
      | playbooks/router_track_kind/machine.yaml | kind: router_track\ndirectory: router_tracks\nregistry: router_tracks.md\ndescription: "Catalog prose description for humans."\nregister: driven\nroute:\n  description: "Route here when the user asks to IMPLEMENT a router-specific feature. NOT for authoring a playbook definition."\n  triggers: ["router feature"]\nrequired_fields: []\nroles:\n  - doer\n  - reviewer\nstates:\n  - name: active\n    role_filters: []\n    registry_section: active\n    projection_targets: []\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
    And the engine is started with that hearth
    When the route RPC is called with message "router feature"
    Then the route candidates include kind "router_track"
    And the route candidate "router_track" description is "Route here when the user asks to IMPLEMENT a router-specific feature. NOT for authoring a playbook definition."
    And the route candidate "router_track" description is not "Catalog prose description for humans."

  # router_relevance_ranker: a spark-flavored message has no relevant DRIVEN playbook
  # (spark is free) → no_match, and spark is never offered as a candidate. The
  # free-exclusion invariant holds across both candidates and abstention.
  Scenario: free kinds are excluded from candidates (AC4)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a free "spark" machine
    And the engine is started with that hearth
    When the route RPC is called with message "capture this idea"
    Then the route outcome is "no_match"
    And the route candidates do not include kind "spark"

  Scenario: a malformed machine.yaml is absent from candidates (AC9)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a malformed machine.yaml
    And the engine is started with that hearth
    When the route RPC is called with message "ingest knowledge for the project"
    Then the route outcome is "candidates"
    And the route candidates include kind "knowledge_lifecycle"
    And the route candidates do not include kind "broken"

  # AC3-v0 (no_match → candidate_playbook_intake) is proven at the core domain
  # seam in anvil-core/features/route_handler.feature: the engine's composite
  # registry always includes the compiled-in driven seeds (track, playbook per
  # decision driven-workflows-vs-free-generative-types), so an empty driven set
  # is unreachable at the engine RPC in v0 (RISK R-B). The non-empty-narrowing
  # form of AC3 is deferred to the BP4 ranker. We do NOT fake it here.

  Scenario: candidates live-reload — a new driven machine appears without restart (AC7)
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the route RPC is called with message "first"
    Then the route candidates do not include kind "fresh_kind"
    When a driven "fresh_kind" machine is dropped into the route hearth
    And the route RPC is called with message "second"
    Then the route candidates include kind "fresh_kind"

  Scenario: live-reload selects a newly registered described and triggered kind without restart
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the route RPC is called with message "second"
    Then the route candidates do not include kind "fresh_kind"
    When a driven "fresh_kind" machine is dropped into the route hearth
    And the route RPC is called with message "second"
    Then the route resolution outcome is "single"
    And the route candidates include kind "fresh_kind"

  Scenario: empty-description driven machines are excluded from route candidates
    Given a hearth directory with playbook files:
      | path                                             | content                                                                                                                                                                                                                                                                                                                                                                               |
      | playbooks/empty_description_kind/machine.yaml    | kind: empty_description\ndirectory: empty_descriptions\nregistry: empty_descriptions.md\ndescription: ""\nregister: driven\nroute:\n  triggers: ["empty route"]\nrequired_fields: []\nroles:\n  - doer\n  - reviewer\nstates:\n  - name: active\n    role_filters: []\n    registry_section: active\n    projection_targets: []\n    is_review_gate: false\n    is_terminal: false\ntransitions: []\n |
    And the engine is started with that hearth
    When the route RPC is called with message "empty route"
    Then the route candidates do not include kind "empty_description"
