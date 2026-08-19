Feature: Route resolution narrows granted driven playbooks by declared triggers

  BP2 (playbook_router): the pure core resolver chooses exactly one playbook
  only when the caller is granted that driven playbook and exactly one granted
  playbook wins the most-specific trigger phrase appearing in the user's input.
  Consulting recap playbooks come from real fixture machine.yaml files;
  Foundation playbooks come from the compiled seed registry.

  Background:
    Given a core route registry with the daily_recap and weekly_recap fixtures plus seed playbooks

  Scenario: a consulting daily recap request resolves to the daily_recap playbook
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "daily recap"
    Then the route resolution outcome is "Single"
    And the selected route kind is "daily_recap"
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly "daily_recap"

  Scenario: a natural consulting daily recap request resolves by its most-specific trigger
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "can you give me my daily recap for today"
    Then the route resolution outcome is "Single"
    And the selected route kind is "daily_recap"
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly "daily_recap"

  Scenario: a described kind with a trigger resolves through the trigger tier
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "daily recap"
    Then the route resolution outcome is "Single"
    And the selected route kind is "daily_recap"
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly "daily_recap"

  Scenario: a described kind without a matching trigger resolves through description overlap
    Given a route resolver request context with org "Foundation" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "artifact lifecycle state machine hooks"
    Then the route resolution outcome is "Single"
    And the selected route kind is "playbook"
    And the granted route candidates are exactly "playbook,track"
    And the matching route candidates are exactly "playbook"

  Scenario: a natural consulting weekly recap request resolves by its most-specific trigger
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "send the weekly recap to the team"
    Then the route resolution outcome is "Single"
    And the selected route kind is "weekly_recap"
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly "weekly_recap"

  Scenario: a consulting recap request returns candidates across daily and weekly recap playbooks
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "recap"
    Then the route resolution outcome is "Candidates"
    And no route kind is selected
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly "daily_recap,weekly_recap"

  # FLIP (router_relevance_ranker): formerly returned the granted set as Candidates.
  # Foundation is granted {track, playbook}; "daily recap" matches no trigger and
  # shares no content token with their descriptions, so it clears no relevance floor
  # → floor-driven NoMatch even though the granted set is non-empty.
  Scenario: floor-driven abstention — granted foundation playbooks are irrelevant to a recap request
    Given a route resolver request context with org "Foundation" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "daily recap"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the granted route candidates are exactly "playbook,track"
    And the matching route candidates are exactly ""

  # FLIP (router_relevance_ranker): this is the over-routing bug the ranker kills —
  # off-topic input formerly handed back the whole granted set; now it abstains.
  Scenario: AC2 — off-topic consulting input abstains even though granted candidates exist
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "something unrelated"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly ""

  # AC1 (router_relevance_ranker): description-regime recall. "deliver the team report"
  # matches no trigger, but its content tokens {deliver, team, report} appear in the
  # daily_recap description (and not in weekly_recap/track/playbook) → Single daily_recap.
  Scenario: AC1 — description overlap recalls a playbook with no trigger match
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "deliver the team report"
    Then the route resolution outcome is "Single"
    And the selected route kind is "daily_recap"
    And the matching route candidates are exactly "daily_recap"

  # AC2 (router_relevance_ranker): an off-topic question abstains (granted set non-empty).
  Scenario: AC2 — an off-topic question abstains
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "what's the weather tomorrow"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly ""

  Scenario: no granted driven playbooks produces a no-match handoff
    Given a route resolver request context with org "Foundation" role "read" clearance "public" space ""
    When the pure route resolver runs with input "daily recap"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the granted route candidates are exactly ""
    And the matching route candidates are exactly ""

  # Abstain gate (experiment #002): a free-kind creation request (decision/proposal/
  # milestone/spark/learning) is begin-able directly, so the router must abstain even
  # though driven candidates are granted and the message's tokens overlap their
  # descriptions. Without the gate this over-routes (the production failure mode).
  Scenario: abstain gate — a free-kind decision request abstains despite granted candidates
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "record a decision about the recap storage model"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the granted route candidates are exactly "daily_recap,playbook,track,weekly_recap"
    And the matching route candidates are exactly ""

  # The gate is ACTION-scoped: a request that merely mentions a free-kind NOUN
  # ("decision engine") but is not a free-kind creation must still route normally.
  Scenario: abstain gate does not catch a recap request that mentions a free-kind noun
    Given a route resolver request context with org "Consulting" role "read" clearance "internal" space ""
    When the pure route resolver runs with input "give me the daily recap for the decision engine work"
    Then the route resolution outcome is "Single"
    And the selected route kind is "daily_recap"
    And the matching route candidates are exactly "daily_recap"
