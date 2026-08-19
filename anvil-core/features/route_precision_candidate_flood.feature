Feature: Router returns 1-3 correct options, not a single-token candidate flood
  router_precision variant #1: the dominant router failure over real traces is
  candidate-FLOODING on turns that share only ONE weak, generic content token
  ("topic", "knowledge", "evidence", ...) with several workflow descriptions. A
  lone generic token makes many candidates "relevant" in the description regime,
  so the router hands back a menu the agent ignores instead of abstaining.

  This variant tames the flood by treating those generic single-token
  coincidences as low-signal (they no longer clear the description floor on their
  own), so a weak single-token turn abstains (no_match) while a genuinely-matching
  turn — one distinctive token, or two-plus shared tokens — still surfaces its
  correct option(s). The return-1-3-correct-options contract, brine-covered.

  Background:
    Given a routable playbooks fixture registry with authored route triggers
    And a route resolver request context with org "Foundation" role "read" clearance "internal" space ""

  # RED before the variant: under RELEVANCE_FLOOR=1 with the un-extended
  # low-signal list, each of these shares only the generic token "topic" with
  # 3+ Lore workflows and floods a capped 3-candidate menu. The variant makes
  # "topic" (and its generic siblings) low-signal, so a lone occurrence abstains.
  Scenario Outline: a weak single-generic-token turn abstains instead of flooding
    When the pure route resolver runs with input "<message>"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the matching route candidates are exactly ""

    Examples:
      | message            |
      | show me that topic |
      | the topic here     |
      | got any evidence   |
      | look at the lore   |

  # Recall guard: a turn that genuinely matches — via a distinctive single token
  # or two-plus shared tokens — must still surface its correct option(s), tightly.
  # These are the same intents the shipped route_precision.feature asserts; the
  # variant must not regress them.
  Scenario Outline: a genuinely-matching turn still returns its correct option tightly
    When the pure route resolver runs with input "<message>"
    Then the route resolution outcome is "Single"
    And the selected route kind is "<kind>"
    And the matching route candidate count is at most 3

    Examples:
      | message                                                                   | kind                  |
      | answer this from my notes: what do I know about transformer scaling?      | lore_query            |
      | what gaps are in my AI-safety topic -- what's missing or contradictory?   | lore_gap_analysis     |
      | find and verify the external sources behind this topic's bookmarks        | lore_source_research  |
      | set up my tax prep for this year                                          | tax_document_collection |
      | start a track to implement the new caching layer                          | track                 |

  # A two-token overlap that clears the floor without any single generic token
  # carrying the match keeps surfacing candidates (cap enforced): the correct
  # kind is present and the set is tight (<=3), never a flood.
  Scenario: a two-generic-token turn stays a tight candidate set including the correct kind
    When the pure route resolver runs with input "compare my topic and the evidence"
    Then the matching route candidates include "compile_topic"
    And the matching route candidate count is at most 3
