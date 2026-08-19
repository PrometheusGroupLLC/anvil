Feature: execution_route discriminator on describe actions
  Each ActionInfo returned by describe(identifier) carries a
  execution_route value — "engine" if that action is executed by
  the engine today, or "fallback:forge:<skill>" otherwise.

  Scenario: Track in spec state has the doer spec_review action as engine
    Given a describe handler with instances:
      | id                         | kind  | state | transitions |
      | 20260414T0405_spec_strand  | track | spec  | 1           |
    When describe is called with identifier "20260414T0405_spec_strand"
    Then the describe result is instance info
    And the describe instance available actions include "spec_review" with execution_route "engine"

  Scenario: Track in spec_review state has the reviewer plan action as engine
    Given a describe handler with instances:
      | id                         | kind  | state       | transitions |
      | 20260414T0405_spec_strand  | track | spec_review | 2           |
    When describe is called with identifier "20260414T0405_spec_strand"
    Then the describe result is instance info
    And the describe instance available actions include "plan" with execution_route "engine"

  Scenario: Track in plan state has the doer plan_review action as engine
    Given a describe handler with instances:
      | id                              | kind  | state | transitions |
      | 20260413T1349_checkin_decomp    | track | plan  | 3           |
    When describe is called with identifier "20260413T1349_checkin_decomp"
    Then the describe result is instance info
    And the describe instance available actions include "plan_review" with execution_route "engine"

  Scenario: Proposal in draft state reports engine-driven actions
    Given a describe handler with instances:
      | id                                  | kind     | state | transitions |
      | 20260411T2021_anvil_workflow_engine | proposal | draft | 1           |
    When describe is called with identifier "20260411T2021_anvil_workflow_engine"
    Then the describe result is instance info
    And the describe instance available actions include "draft_review" with execution_route "engine"

  # BP7 / AC7 — a knowledge_lifecycle driven (state, role) reports "engine" on
  # the describe seam, machine-derived from the resolved machine.yaml.
  Scenario: Knowledge in ingesting state has the ingest action as engine
    Given a describe handler with instances:
      | id                       | kind                | state     | transitions |
      | 20260601T0000_topic      | knowledge_lifecycle | ingesting | 1           |
    When describe is called with identifier "20260601T0000_topic" using the knowledge_lifecycle machine
    Then the describe result is instance info
    And the describe instance available actions include "ingest_review" with execution_route "engine"

  Scenario: Knowledge in ingest_review state has the reviewer action as engine
    Given a describe handler with instances:
      | id                       | kind                | state         | transitions |
      | 20260601T0000_rev        | knowledge_lifecycle | ingest_review | 2           |
    When describe is called with identifier "20260601T0000_rev" using the knowledge_lifecycle machine
    Then the describe result is instance info
    And the describe instance available actions include "organizing" with execution_route "engine"
