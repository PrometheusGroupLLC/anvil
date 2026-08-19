Feature: PlaybookActivity RPC returns registered playbooks grouped by owner with call counts
  The read-only PlaybookActivity RPC backs the Foundry playbook-activity UI.
  It constructs a fresh playbook registry per call (mirroring route/describe
  always-reload), derives each playbook's owner from the `contributed_by`
  field in its status.yaml ("anvil" when absent), and folds the UNIVERSAL
  ACTIVITY LOG into a per-kind call count — the SAME fold the activity_summary
  `by_artifact_kind` view uses, so the owner roll-up reconciles with the universal
  usage view (not the narrower routing-activity sink, which only logs `route`
  turns). The response groups playbooks by owner so the surface renders one
  section per kit.

  Scenario: registered playbooks of differing owners are returned grouped by owner
    Given a playbook activity engine hearth with playbooks:
      | kind        | owner    | description           |
      | lore_query  | lore-kit | Answer a question     |
      | measurement | temper-kit | Run a measurement     |
      | track       |          | The dev-playbook track |
    And the engine is started with that hearth
    When the PlaybookActivity RPC is called
    Then the playbook activity RPC groups owner "lore-kit" with kinds "lore_query"
    And the playbook activity RPC groups owner "temper-kit" with kinds "measurement"
    And the playbook activity RPC groups owner "anvil" with kinds "track"
    And the playbook activity RPC entry for kind "lore_query" has description "Answer a question"

  Scenario: call counts reflect routing activity recorded on the route path
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner    | description       |
      | activity_one | lore-kit | First activity    |
      | activity_two | lore-kit | Second activity   |
    And the engine is started with that hearth
    When the route RPC is called with message "activity_one", signal "", conversation_id "turn-activity-1", ctx org "Foundation" role "read" clearance "internal"
    And the route RPC is called with message "activity_one", signal "", conversation_id "turn-activity-2", ctx org "Foundation" role "read" clearance "internal"
    And the route RPC is called with message "activity_two", signal "", conversation_id "turn-activity-3", ctx org "Foundation" role "read" clearance "internal"
    And the PlaybookActivity RPC is called
    Then the playbook activity RPC entry for kind "activity_one" has call count 2
    And the playbook activity RPC entry for kind "activity_two" has call count 1

  Scenario: call counts fold ALL command turns for a kind, not just routes
    # Three NON-route turns (begin/snapshot/complete) and zero routes: the
    # routing-activity sink would report 0, but the activity-log fold reports 3 —
    # reconciling the owner roll-up with the universal by_artifact_kind usage view.
    Given a playbook activity engine hearth with playbooks:
      | kind  | owner | description |
      | track | anvil | Dev track   |
    And an activity log record with command "begin" kind "track" is appended to the hearth
    And an activity log record with command "snapshot" kind "track" is appended to the hearth
    And an activity log record with command "complete" kind "track" is appended to the hearth
    And the engine is started with that hearth
    When the PlaybookActivity RPC is called
    Then the playbook activity RPC entry for kind "track" has call count 3

  Scenario: all_hearths aggregates owner roll-ups across two seeded hearths
    Given a playbook activity engine hearth with playbooks:
      | kind        | owner    | description     |
      | shared_kind | lore-kit | Shared playbook |
    And a playbook activity secondary hearth with playbooks:
      | kind        | owner    | description     |
      | shared_kind | lore-kit | Shared playbook |
      | other_kind  | lore-kit | Other playbook  |
    And the engine is started with both permitted hearths
    When the route RPC is called with message "shared_kind", signal "", conversation_id "wa-all-1", ctx org "Foundation" role "read" clearance "internal"
    And the route RPC is called with message "shared_kind", signal "", conversation_id "wa-all-2", ctx org "Foundation" role "read" clearance "internal"
    And the PlaybookActivity RPC is called across all hearths
    Then the playbook activity RPC entry for kind "shared_kind" has call count 2
    And the playbook activity RPC groups owner "lore-kit" with kinds "other_kind,shared_kind"
    And the playbook activity RPC resolved hearth is the all-hearths sentinel
    And the playbook activity RPC included 2 hearths

  Scenario: all_hearths discovers sub-hearths beneath a permitted PARENT root
    Given a permitted parent root with two sub-hearths each seeded with playbook "shared_kind" owned by "lore-kit"
    And the engine is started with the first sub-hearth and the parent as a permitted root
    When the route RPC is called with message "shared_kind", signal "", conversation_id "wa-parent-1", ctx org "Foundation" role "read" clearance "internal"
    And the PlaybookActivity RPC is called across all hearths
    Then the playbook activity RPC resolved hearth is the all-hearths sentinel
    And the playbook activity RPC included 2 hearths
    And the playbook activity RPC entry for kind "shared_kind" has call count 1
