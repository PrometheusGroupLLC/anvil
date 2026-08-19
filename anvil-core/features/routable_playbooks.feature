Feature: Stated intent routes to routable playbook machines
  The routable-playbooks trigger table should drive the pure route resolver over
  a deterministic fixture registry. The fixture uses the authored triggers and
  avoids reading the live anvil-hearth checkout.

  Background:
    Given a routable playbooks fixture registry with authored route triggers
    And a route resolver request context with org "Foundation" role "read" clearance "internal" space ""

  Scenario: an unambiguous playbook-authoring intent resolves to playbook_generation
    When the pure route resolver runs with input "create a workflow for daily standups"
    Then the route resolution outcome is "Single"
    And the selected route kind is "playbook_generation"

  Scenario: a tax intent resolves to tax_document_collection
    When the pure route resolver runs with input "prepare taxes for 2025"
    Then the route resolution outcome is "Single"
    And the selected route kind is "tax_document_collection"

  Scenario: a knowledge question routes to lore_query
    When the pure route resolver runs with input "ask lore what we know about pricing"
    Then the route resolution outcome is "Single"
    And the matching route candidates include "lore_query"

  Scenario: a dev intent routes to the canonical track playbook
    When the pure route resolver runs with input "start a track to add SSO"
    Then the route resolution outcome is "Single"
    And the selected route kind is "track"
    And the selected route source playbook id is "20260422T0000_track_lifecycle"

  Scenario: a dev intent without the track kind name routes to the canonical track playbook
    When the pure route resolver runs with input "implement a feature for SSO"
    Then the route resolution outcome is "Single"
    And the selected route kind is "track"

  Scenario: a playbook-authoring intent without the workflow kind name routes to playbook
    When the pure route resolver runs with input "add a lifecycle type"
    Then the route resolution outcome is "Single"
    And the selected route kind is "playbook"

  Scenario: an ambiguous intent returns candidates for the surface to pick
    When the pure route resolver runs with input "ask lore tax prep"
    Then the route resolution outcome is "Candidates"
    And no route kind is selected
    And the matching route candidates are exactly "lore_query,tax_document_collection"
    And the matching route candidate descriptions are returned for selection

  Scenario: no granted route candidates hand off to candidate_playbook_intake
    Given a route resolver request context with org "Foundation" role "read" clearance "public" space ""
    When the pure route resolver runs with input "what's the weather today"
    Then the route resolution outcome is "NoMatch"
    And no route kind is selected
    And the no-match handoff is "candidate_playbook_intake"

  Scenario: queue-driven playbooks are not conversationally routable
    Given the playbook "extract_document" is a pending_queue kind
    Then it declares no conversational route triggers
    But it still declares a routing-grade description
