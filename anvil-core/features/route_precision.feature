Feature: Route precision shortlists relevant playbook candidates
  The route resolver should use trigger and description match strength to avoid
  flooding every granted playbook candidate into conversational turns.

  Background:
    Given a routable playbooks fixture registry with authored route triggers
    And a route resolver request context with org "Foundation" role "read" clearance "internal" space ""

  Scenario Outline: clear playbook intents resolve tightly
    When the pure route resolver runs with input "<message>"
    Then the route resolution outcome is "Single"
    And the selected route kind is "<kind>"
    And the top-ranked matching route candidate is "<kind>"
    And the matching route candidate count is at most 3

    Examples:
      | message                                                                  | kind                       |
      | compile the topic on AI safety from its evidence                         | compile_topic              |
      | what gaps are in my AI-safety topic -- what's missing or contradictory?  | lore_gap_analysis          |
      | answer this from my notes: what do I know about transformer scaling?      | lore_query                 |
      | find and verify the external sources behind this topic's bookmarks       | lore_source_research       |
      | auto-organize my uncategorized lore captures into topics                  | lore_categorize            |
      | tag the untagged images in my media library                               | lore_vision                |
      | extract the box fields from this uploaded 1099                             | extract_document           |
      | import my transaction history CSV from the old tool                        | import_transaction_history |
      | start a track to implement the new caching layer                           | track                      |
      | let's begin a new track for the auth refactor                              | track                      |
      | research this person and enrich their entity in the graph                  | entity_research            |
      | set up my tax prep for this year                                           | tax_document_collection    |

  Scenario Outline: non-playbook turns abstain instead of flooding candidates
    When the pure route resolver runs with input "<message>"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the matching route candidates are exactly ""

    Examples:
      | message                                                                             |
      | run the test suite and tell me what fails                                           |
      | rename this variable across the file                                                |
      | what is 2+2                                                                         |
      | git pull the latest changes to main                                                 |
      | explain how the route hook works                                                    |
      | fix a typo in the readme                                                            |
      | summarize this error message                                                        |
      | you are an adversarial reviewer; refute the ranking in this doc, citing file:line   |
      | capture this thought: the router floods on long inputs                              |
      | pull the recent changes to second brain and tell me what's new                      |

  Scenario: ambiguous playbook-definition intent is capped and keeps a correct candidate
    When the pure route resolver runs with input "define a new playbook for incident response"
    Then the matching route candidates include "playbook_generation"
    And the matching route candidate count is at most 3

  # router_precision fix #1 (NO-TASK ABSTENTION): the dominant justified over-match
  # is the router firing on a system-reminder-only turn. The reminder text below is
  # laced with playbook-ish words ("compile", "topic", "evidence", "organize",
  # "captures", "topics") that WOULD have flooded description-overlap candidates
  # before the gate. Stripping the injected block leaves no genuine task → NoMatch.
  Scenario: a system-reminder-only turn abstains instead of flooding candidates
    When the pure route resolver runs with input "<system-reminder>The user opened a file. Codebase instructions: follow the lifecycle. Compile the topic evidence and organize captures across topics.</system-reminder>"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the matching route candidates are exactly ""

  # router_precision fix #1 (NO-TASK ABSTENTION): a genuinely whitespace-only turn
  # carries no user request at all and must abstain. This is branch (a) of the
  # no-task gate in its purest form — nothing to strip, nothing to route. The
  # literal spaces here are preserved (not a table cell), so `trim()` is exercised.
  Scenario: a whitespace-only turn abstains
    When the pure route resolver runs with input "   "
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the matching route candidates are exactly ""

  # router_precision fix #1 RECALL GUARD: a short, low-signal continuation is REAL
  # work (the user is continuing the prior task), NOT a no-task turn. The no-task
  # gate must let it through to normal routing. "fix it" carries the content token
  # "fix", so it survives the gate and resolves like any other engineering turn
  # (NoMatch here only because no fixture playbook matches a bare "fix it").
  Scenario: a short continuation turn is not swallowed by the no-task gate
    When the pure route resolver runs with input "fix it now, then run the build"
    Then the route resolution outcome is "NoMatch"
    And the matching route candidates are exactly ""

  # router_precision fix #1: settled / task-complete acknowledgment turns carry no
  # new user request and must abstain (whole-message match; punctuation-insensitive).
  Scenario Outline: settled acknowledgment turns abstain
    When the pure route resolver runs with input "<message>"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the matching route candidates are exactly ""

    Examples:
      | message       |
      | Thanks!       |
      | looks good    |
      | task complete |
      | perfect       |
      | lgtm          |

  # router_precision fix #1 RECALL GUARD: the no-task gate must NEVER swallow a real
  # task. A genuine "start a track" intent that merely CARRIES an injected reminder
  # still routes — stripping removes only the reminder, leaving the real request.
  Scenario: a genuine task carrying an injected reminder still routes
    When the pure route resolver runs with input "start a track to implement the new caching layer <system-reminder>remember to follow the playbook</system-reminder>"
    Then the route resolution outcome is "Single"
    And the selected route kind is "track"
    And the matching route candidate count is at most 3
