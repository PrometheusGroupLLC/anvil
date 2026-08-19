Feature: Route RPC surfaces pure route resolution
  BP2b maps the engine route RPC through the pure core resolver. The existing
  candidate-set/no_match contract remains intact: the legacy candidates field
  contains every granted driven candidate, while new response fields expose
  the resolver's selected kind, outcome, and trigger-matching candidates.

  Scenario: consulting daily recap input resolves to one playbook
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "single"
    And the route selected kind is "daily_recap"
    And the route matching candidates are exactly "daily_recap"
    And the route candidates include kind "daily_recap"
    And the route candidates include kind "weekly_recap"
    And the route candidates include kind "track"
    And the route candidates include kind "playbook"

  Scenario: consulting generic recap input returns candidates with tied trigger matches
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "recap" and ctx org "Consulting" role "read" clearance "internal"
    Then the route outcome is "candidates"
    And the route resolution outcome is "candidates"
    And no route selected kind is set
    And the route matching candidates are exactly "daily_recap,weekly_recap"
    And the route candidates include kind "daily_recap"
    And the route candidates include kind "weekly_recap"

  # FLIP (router_relevance_ranker): formerly returned the granted set as candidates.
  # Foundation is granted {track, playbook}; "daily recap" matches no trigger and
  # clears no relevance floor against their descriptions → floor-driven no_match
  # even though the granted set is non-empty (distinct from the empty-granted-set
  # no_match below). The surface is handed candidate_playbook_intake + the intent.
  Scenario: foundation context abstains when no granted playbook is relevant (floor-driven no_match)
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Foundation" role "read" clearance "internal"
    Then the route outcome is "no_match"
    And the route resolution outcome is "no_match"
    And no route selected kind is set
    And the route matching candidates are exactly ""
    And the route candidates do not include kind "daily_recap"
    And the route candidates do not include kind "weekly_recap"
    And the route handoff is "candidate_playbook_intake"
    And the route intent echoes "daily recap"

  Scenario: no granted route candidates produces the legacy no_match handoff
    Given a route hearth seeded with consulting recap fixtures plus seed playbooks
    And the engine is started with that hearth
    When the route RPC is called with message "daily recap" and ctx org "Foundation" role "read" clearance "public"
    Then the route outcome is "no_match"
    And the route resolution outcome is "no_match"
    And no route selected kind is set
    And the route matching candidates are exactly ""
    And the route candidates do not include kind "daily_recap"
    And the route candidates do not include kind "weekly_recap"
    And the route handoff is "candidate_playbook_intake"
    And the route intent echoes "daily recap"
