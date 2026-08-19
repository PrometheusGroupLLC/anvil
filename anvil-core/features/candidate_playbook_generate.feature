Feature: Candidate playbook deterministic generation
  A CandidateWorkflow emitted by Lore can be transformed into a loader-valid
  PlaybookMachine without I/O, live agents, or mocks.

  Scenario: A multi-step candidate generates forge-shaped review diamonds
    Given a candidate playbook with intent "Applicant Interview Intake" and proposed states:
      | state      | role        | intent                                    | expected_output                         | success_criteria                                                          |
      | screen     | recruiter   | Screen the applicant for baseline fit.    | A screening note with pass or fail.      | The screening note records a `PASS` or `FAIL` verdict with a reason.      |
      | interview  | coordinator | Schedule and record the interview result. | An interview packet with a disposition.  | The interview packet records a disposition of `advance` or `reject`.     |
    When the candidate playbook is generated
    Then the generated playbook has kind "applicant_interview_intake"
    And the generated playbook has directory "applicant_interview_intakes"
    And the generated playbook directory and registry derive consistently from kind
    And the generated playbook has route description "Route here when the user asks to AUTHOR applicant interview intake playbooks. NOT for running an applicant interview intake instance."
    And the generated playbook has route triggers:
      | trigger                    |
      | create interview playbook  |
      | author interview intake    |
    And the generated playbook has roles:
      | role        |
      | recruiter   |
      | coordinator |
      | reviewer    |
    And the generated playbook has states:
      | state                | registry_section | is_review_gate | is_terminal |
      | screen               | active           | false          | false       |
      | screen_review        | active           | true           | false       |
      | screen_revision      | active           | false          | false       |
      | interview            | active           | false          | false       |
      | interview_review     | active           | true           | false       |
      | interview_revision   | active           | false          | false       |
      | outcome_reflection_review | active      | true           | false       |
      | completed            | completed        | false          | true        |
    And every generated playbook state has projection targets:
      | target       |
      | workflows.md |
    And the generated playbook has transitions:
      | from_state           | to_state          | required_role | required_satisfaction |
      | screen               | screen_review     | recruiter     |                        |
      | screen_review        | interview         | reviewer      | satisfied              |
      | screen_review        | screen_revision   | reviewer      | needs_revision         |
      | screen_revision      | screen_review     | recruiter     |                        |
      | interview            | interview_review  | coordinator   |                        |
      | interview_review     | outcome_reflection_review | reviewer | satisfied              |
      | interview_review     | interview_revision| reviewer      | needs_revision         |
      | interview_revision   | interview_review  | coordinator   |                        |
      | outcome_reflection_review | completed    | reviewer      | satisfied              |
      | outcome_reflection_review | interview_revision | reviewer  | needs_revision         |
    And the generated playbook measurement for state "screen" role "doer" has intent "Screen the applicant for baseline fit." and expected_output "A screening note with pass or fail."
    And the generated playbook measurement for state "interview" role "doer" has intent "Schedule and record the interview result." and expected_output "An interview packet with a disposition."
    And the generated playbook measurement for state "screen_review" role "reviewer" is non-empty
    And the generated playbook measurement for state "screen_revision" role "doer" is non-empty
    And the generated playbook has no measurement for state "screen" role "recruiter"
    And the generated playbook has no measurement for state "interview" role "coordinator"
    And the generated playbook has no hook references
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: Trial review guidance asks reviewers to run anchor coverage validation
    Given a candidate playbook with intent "Workflow Trial" and proposed states:
      | state | role | intent                         | expected_output            | success_criteria                                                          |
      | trial | doer | Trial the generated workflow.  | A trial result with notes. | The trial result cites at least one resolved `anchor_instance` by name.  |
    When the candidate playbook is generated
    Then the generated playbook measurement for state "trial_review" role "reviewer" contains "Run anchor coverage validation"
    And the generated playbook measurement for state "trial_review" role "reviewer" contains "none_yet_justification"

  Scenario: Route metadata is carried from the candidate onto the generated machine
    Given a candidate playbook with intent "Incident Response" route description "Route here when the user asks to AUTHOR incident response workflows. NOT for resolving an active incident." and route triggers:
      | trigger                   |
      | create incident workflow  |
      | author incident process   |
    And candidate projection targets:
      | target        |
      | operations.md |
    And candidate proposed states:
      | state  | role | intent                            | expected_output              | success_criteria                                                   |
      | intake | doer | Capture incident response inputs. | An incident response packet. | The incident response packet names an on-call owner from `oncall.yaml`. |
    When the candidate playbook is generated
    Then the generated playbook has route description "Route here when the user asks to AUTHOR incident response workflows. NOT for resolving an active incident."
    And the generated playbook has route triggers:
      | trigger                   |
      | create incident workflow  |
      | author incident process   |
    And every generated playbook state has projection targets:
      | target        |
      | operations.md |

  Scenario: Success rubric and anchors are carried from the candidate onto the generated machine
    Given a candidate playbook with intent "Evidence Triage" and proposed states:
      | state  | role | intent                  | expected_output             | success_criteria                                             |
      | triage | doer | Triage the new evidence. | A triage note with routing. | The triage note names a routing target from `owners.yaml`.  |
    And the candidate carries a success rubric with anchors:
      | dimension      | weight | evidence_class          | anchor_instance | anchor_band |
      | correctness    | 3      | artifact_of_consequence | triage-good     | good        |
      | research_rigor | 2      | verifiable_citation     | triage-trap     | trap        |
    When the candidate playbook is generated
    Then the generated playbook success rubric has dimensions:
      | dimension      | weight | evidence_class          |
      | correctness    | 3      | artifact_of_consequence |
      | research_rigor | 2      | verifiable_citation     |
    And the generated playbook success rubric has anchors:
      | instance    | band |
      | triage-good | good |
      | triage-trap | trap |
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: Outcome predicate is carried from the candidate onto the generated machine
    Given a candidate playbook with intent "Evidence Triage Predicate" and proposed states:
      | state  | role | intent                   | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence. | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    And the candidate carries an outcome predicate with terminal_state "completed" and check "documents reconciled"
    When the candidate playbook is generated
    Then the generated playbook has outcome predicate terminal_state "completed"
    And the generated playbook has outcome predicate check "documents reconciled"
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: A candidate without a declared outcome predicate generates a machine without one
    Given a candidate playbook with intent "Evidence Triage No Predicate" and proposed states:
      | state  | role | intent                   | expected_output             | success_criteria                                            |
      | triage | doer | Triage the new evidence. | A triage note with routing. | The triage note names a routing target from `owners.yaml`. |
    When the candidate playbook is generated
    Then the generated playbook has no outcome predicate
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: A single-step candidate advances from review to terminal
    Given a candidate playbook with intent "Reference Check" and proposed states:
      | state     | role      | intent                            | expected_output                  | success_criteria                                          |
      | reference | recruiter | Verify references for the person. | A reference check summary.       | The reference check summary lists at least 2 references. |
    When the candidate playbook is generated
    Then the generated playbook has transitions:
      | from_state        | to_state           | required_role | required_satisfaction |
      | reference         | reference_review   | recruiter     |                        |
      | reference_review  | outcome_reflection_review | reviewer | satisfied              |
      | reference_review  | reference_revision | reviewer      | needs_revision         |
      | reference_revision| reference_review   | recruiter     |                        |
      | outcome_reflection_review | completed | reviewer      | satisfied              |
      | outcome_reflection_review | reference_revision | reviewer | needs_revision         |
    And the generated playbook has a pre-completed outcome reflection review state
    And the generated playbook measurement for state "outcome_reflection_review" role "reviewer" contains "scores vs anchors"
    And the generated playbook passes loader validation with artifact id "candidate-intake"

  Scenario: Empty proposed states are rejected
    Given a candidate playbook with intent "Empty Candidate" and no proposed states
    When the candidate playbook is generated
    Then candidate playbook generation fails with EmptyProposedStates

  Scenario: Blank candidate intent is rejected
    Given a candidate playbook with blank intent and proposed states:
      | state  | role      | intent                    | expected_output       |
      | screen | recruiter | Screen the applicant.     | A screening note.     |
    When the candidate playbook is generated
    Then candidate playbook generation fails with BlankIntent

  Scenario: Blank candidate route description is rejected
    Given a candidate playbook with blank route description and proposed states:
      | state  | role      | intent                    | expected_output       |
      | screen | recruiter | Screen the applicant.     | A screening note.     |
    When the candidate playbook is generated
    Then candidate playbook generation fails with BlankRouteDescription

  Scenario: Empty candidate route triggers are rejected
    Given a candidate playbook with empty route triggers and proposed states:
      | state  | role      | intent                    | expected_output       |
      | screen | recruiter | Screen the applicant.     | A screening note.     |
    When the candidate playbook is generated
    Then candidate playbook generation fails with EmptyRouteTriggers

  Scenario: Empty candidate projection targets are rejected
    Given a candidate playbook with empty projection targets and proposed states:
      | state  | role      | intent                    | expected_output       |
      | screen | recruiter | Screen the applicant.     | A screening note.     |
    When the candidate playbook is generated
    Then candidate playbook generation fails with EmptyProjectionTargets
    And candidate playbook generation failure contains "projection_targets are required so generated playbooks remain auditable"

  Scenario: Proposed state entries must not have blank required fields
    Given a candidate playbook with intent "Blank Proposed State Field" and proposed states:
      | state  | role      | intent                | expected_output |
      | screen | recruiter | Screen the applicant. |                 |
    When the candidate playbook is generated
    Then candidate playbook generation fails with BlankProposedStateField for field "expected_output" at index 0

  Scenario: Proposed state cannot use the reserved reviewer role
    Given a candidate playbook with intent "Reviewer Role Collision" and proposed states:
      | state  | role     | intent                         | expected_output          |
      | screen | reviewer | Screen the applicant as review.| A conflicted screening.  |
    When the candidate playbook is generated
    Then candidate playbook generation fails with ReservedRole for role "reviewer"

  Scenario: Proposed state cannot use the generated terminal name
    Given a candidate playbook with intent "Terminal Collision" and proposed states:
      | state     | role      | intent                         | expected_output           |
      | completed | recruiter | Complete intake prematurely.   | A premature completion.   |
    When the candidate playbook is generated
    Then candidate playbook generation fails with StateNameCollision for name "completed"

  Scenario: Proposed state names must be unique
    Given a candidate playbook with intent "Duplicate State Collision" and proposed states:
      | state  | role        | intent                         | expected_output        |
      | screen | recruiter   | Screen the applicant.          | A screening note.      |
      | screen | coordinator | Screen the applicant again.    | Another screening note.|
    When the candidate playbook is generated
    Then candidate playbook generation fails with StateNameCollision for name "screen"

  Scenario: Proposed state cannot collide with generated review or revision states
    Given a candidate playbook with intent "Generated State Collision" and proposed states:
      | state         | role        | intent                         | expected_output        |
      | screen        | recruiter   | Screen the applicant.          | A screening note.      |
      | screen_review | coordinator | Reuse a generated review name. | A duplicate review.    |
    When the candidate playbook is generated
    Then candidate playbook generation fails with StateNameCollision for name "screen_review"
