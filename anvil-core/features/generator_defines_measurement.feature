Feature: Generator authors per-step measurement criteria by construction
  The candidate-to-machine generator does not just carry a proposed state's
  intent and expected_output onto the generated doer/review MeasurementSpecs —
  it also carries the author-supplied success_criteria, and refuses to
  generate a driven playbook whose steps have no checkable success signal.
  Presence of a criteria string is not enough: it must be falsifiable
  (checkable), or generation is rejected. This is the anti-Goodhart gate.

  Under the same enforcement gate, a driven candidate must also declare an
  outcome_predicate with a non-blank terminal_state — the checkable FACT
  ("did the world-change happen") distinct from success_criteria's HOW WELL.
  A generated playbook with no declared predicate can never be evaluated by
  the outcome-predicate fold, only guessed at via the registry's generic
  per-state terminal flag.

  Scenario: Falsifiable per-step success criteria carry onto both doer and review measurements
    Given a candidate playbook with intent "Evidence Triage With Criteria" and proposed states:
      | state  | role | intent                    | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence.  | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    When the candidate playbook is generated
    Then the generated playbook measurement for state "triage" role "doer" has success_criteria "The triage note names a routing target from `owners.yaml`."
    And the generated playbook measurement for state "triage_review" role "reviewer" has success_criteria "The triage note names a routing target from `owners.yaml`."
    And the generated playbook measurement for state "triage_revision" role "doer" has success_criteria "The triage note names a routing target from `owners.yaml`."
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: A doer state with no success_criteria at all is rejected under enforcement
    Given a candidate playbook with intent "Evidence Triage Missing Criteria" and proposed states:
      | state  | role | intent                   | expected_output             |
      | triage | doer | Triage the new evidence. | A triage note with routing. |
    When the candidate playbook is generated with measurement enforcement
    Then candidate playbook generation fails with VacuousSuccessCriteria for state "triage"

  Scenario: A vacuous per-step success criterion is rejected under enforcement
    Given a candidate playbook with intent "Evidence Triage Vacuous Criteria" and proposed states:
      | state  | role | intent                   | expected_output             | success_criteria             |
      | triage | doer | Triage the new evidence. | A triage note with routing. | The output is high quality.  |
    When the candidate playbook is generated with measurement enforcement
    Then candidate playbook generation fails with VacuousSuccessCriteria for state "triage"

  Scenario: A driven candidate missing an outcome predicate is rejected under enforcement
    Given a candidate playbook with intent "Evidence Triage Missing Predicate" and proposed states:
      | state  | role | intent                    | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence.  | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    When the candidate playbook is generated with measurement enforcement
    Then candidate playbook generation fails with MissingOutcomePredicate

  Scenario: A driven candidate with a blank outcome predicate terminal_state is rejected under enforcement
    Given a candidate playbook with intent "Evidence Triage Blank Predicate" and proposed states:
      | state  | role | intent                    | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence.  | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    And the candidate carries an outcome predicate with terminal_state "   "
    When the candidate playbook is generated with measurement enforcement
    Then candidate playbook generation fails with MissingOutcomePredicate

  Scenario: A driven candidate with success criteria and an outcome predicate passes enforcement
    Given a candidate playbook with intent "Evidence Triage Complete Definition" and proposed states:
      | state  | role | intent                    | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence.  | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    And the candidate carries an outcome predicate with terminal_state "completed"
    When the candidate playbook is generated with measurement enforcement
    Then the generated playbook has kind "evidence_triage_complete_definition"
    And the generated playbook has outcome predicate terminal_state "completed"
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: The default (non-enforcing) generate accepts a candidate with no success_criteria
    Given a candidate playbook with intent "Evidence Triage No Criteria Default" and proposed states:
      | state  | role | intent                   | expected_output             |
      | triage | doer | Triage the new evidence. | A triage note with routing. |
    When the candidate playbook is generated
    Then the generated playbook has kind "evidence_triage_no_criteria_default"
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario Outline: The falsifiability lint accepts checkable criteria and rejects vague ones
    When the falsifiability lint evaluates the criterion "<criterion>"
    Then the criterion is judged "<verdict>"

    Examples:
      | criterion                                                          | verdict     |
      | Test coverage for the touched module is at least 90%.             | falsifiable |
      | The process exits and the log names ECONNREFUSED.                 | falsifiable |
      | The generated file is anvil-core/src/domain/playbook/generate.rs. | falsifiable |
      | The fix is verified by running cargo test for the crate.          | falsifiable |
      | The response body contains the token `not_found`.                 | falsifiable |
      | The retry queue is empty after processing completes.              | falsifiable |
      | The output is high quality.                                       | vacuous     |
      | It looks good and feels right.                                    | vacuous     |
      | Great work overall on this step.                                  | vacuous     |
      | Not too bad, seems fine.                                          | vacuous     |
