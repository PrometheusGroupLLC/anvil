Feature: Playbook access scoping over route and begin RPCs
  Route enumerates only playbook machines granted to the caller context, and
  begin rechecks the same grant before creating any artifact.

  Scenario: BP4 route narrows candidates for a restrictive public context
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "public_intake" machine with access org "Foundation" role "read" sensitivity "public"
    And the engine is started with that hearth
    When the route RPC is called with message "triage a public intake" and ctx org "acme" role "read" clearance "public"
    Then the route outcome is "candidates"
    And the route candidates include kind "public_intake"
    And the route candidates do not include kind "knowledge_lifecycle"

  Scenario: BP5 begin denies a generic create when the context is not granted
    Given a hearth seeded with the restricted knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "restricted topic" with no parent and ctx org "Foundation" role "read" clearance "internal"
    Then the begin RPC returns gRPC status "PERMISSION_DENIED"
    And the begin RPC error message contains "access_denied"

  Scenario: BP5 begin allows a generic create when the context is granted
    Given a hearth seeded with the restricted knowledge_lifecycle machine
    And the engine is started with that hearth
    When the begin RPC is called to create a "knowledge_lifecycle" artifact named "restricted topic" with no parent and ctx org "acme" role "admin" clearance "phi"
    Then the begin RPC response track_path starts with "knowledge/"
    And the begin RPC response track_path file "status.yaml" contains "kind: knowledge_lifecycle"

  Scenario: BP5 begin denies the playbook create arm when the context is not granted
    Given a hearth seeded with the restricted playbook machine and an active parent track
    And the engine is started with that hearth
    When the begin RPC is called to create a playbook named "restricted playbook" under parent "20260607T0000_access_scoping_parent" with ctx org "Foundation" role "read" clearance "internal"
    Then the begin RPC returns gRPC status "PERMISSION_DENIED"
    And the begin RPC error message contains "access_denied"

  Scenario: BP6 default-safe route offers every driven seed and cached route machine
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth also has a driven "public_intake" machine with access org "Foundation" role "read" sensitivity "public"
    And the route hearth also has a driven "default_internal" machine
    And the engine is started with that hearth
    When the route RPC is called with message "show default candidates"
    Then the route candidates equal every driven seed and cached route machine
