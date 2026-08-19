Feature: Amendment render is structured-only — spec kind (AC-6)

  # render(schema, base, log) takes only a structured base document and a
  # structured op log — no file path, no prose string, no *.amendments.md
  # parameter. The no-prose guarantee is type-level: there is no prose-parsing
  # code path by construction (the step definitions pass only structured inputs).
  # The rendered output equals base + structured ops only.

  Background:
    Given a spec base document with elements:
      | id  | kind                 | body              |
      | R1  | requirement          | The system shall X. |
      | AC1 | acceptance_criterion | X is observable.  |

  Scenario: render of an empty log equals the base document
    Given an empty op log
    When render is called on the spec base and log
    Then render succeeds
    And the rendered document equals the base document

  Scenario: render reflects base plus the structured ops only
    Given an empty op log
    And the log has a "revise" op "op-1" on "AC1" with body "structured revise" accepted at "2026-06-01T00:00:00Z"
    When render is called on the spec base and log
    Then render succeeds
    And rendered element "AC1" has body "structured revise"
    And rendered element "R1" has body "The system shall X."
