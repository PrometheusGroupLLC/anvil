Feature: Scorecard aggregation folds temper's per-instance scorecards into a per-kind measurement summary
  Temper persists one quality scorecard per instance on disk. The Atlas needs a
  per-kind view: the MEAN quality for each declared (from_state, to_state,
  role) step across every instance of that kind, the kind's overall mean
  quality, and a recent-measurements feed. This fold is pure — it takes
  already-parsed scorecards and produces the summary deterministically, so it
  works identically whether 1 or 1000 scorecards exist.

  Scenario: mean quality is computed per declared step across matching instances of a kind
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state    | role     | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | start      | spec        | doer     | Claude | 0.6           | claude-opus-4-8 |
      | inst_a   | track    | 0.5          | spec       | spec_review | reviewer | Claude | 0.4           | claude-opus-4-8 |
      | inst_b   | track    | 0.7          | start      | spec        | doer     | Claude | 0.8           | claude-opus-4-8 |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    Then the scorecard summary has instance_count 2
    And the scorecard summary has overall_mean_quality permille 600
    And the scorecard summary has 2 steps
    And the scorecard step from "start" to "spec" role "doer" has mean_quality permille 700 sample_count 2
    And the scorecard step from "spec" to "spec_review" role "reviewer" has mean_quality permille 400 sample_count 1

  Scenario: instances of a different kind are excluded from every part of the summary
    Given scorecards:
      | instance | track_id       | mean_quality | from_state | to_state | role | actor  | quality_score | model           |
      | inst_a   | track          | 0.9          | start      | spec     | doer | Claude | 0.9           | claude-opus-4-8 |
      | inst_x   | other_playbook | 0.1          | start      | gather   | doer | Claude | 0.1           | claude-opus-4-8 |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    Then the scorecard summary has instance_count 1
    And the scorecard summary has 1 steps

  Scenario: the recent feed is capped at the recent limit and preserves input order
    Given scorecards:
      | instance | track_id | mean_quality | from_state  | to_state     | role     | actor  | quality_score | model           |
      | inst_b   | track    | 0.7          | start       | spec         | doer     | Claude | 0.8           | claude-opus-4-8 |
      | inst_b   | track    | 0.7          | spec        | spec_review  | reviewer | Claude | 0.6           | claude-opus-4-8 |
      | inst_a   | track    | 0.5          | start       | spec         | doer     | Claude | 0.4           | claude-opus-4-8 |
    When the scorecard measurements are folded for kind "track" with recent limit 2
    Then the scorecard summary has 2 recent entries
    And recent entry 1 is instance "inst_b" to_state "spec" role "doer"
    And recent entry 2 is instance "inst_b" to_state "spec_review" role "reviewer"

  Scenario: no scorecards for a kind folds to an empty, non-error summary
    Given no scorecards
    When the scorecard measurements are folded for kind "track" with recent limit 5
    Then the scorecard summary has instance_count 0
    And the scorecard summary has 0 steps
    And the scorecard summary has 0 recent entries

  Scenario: artifact quality attaches to a matching coherence step, alongside (not replacing) it
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state    | role     | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | start      | spec        | doer     | Claude | 0.06          | claude-opus-4-8 |
      | inst_a   | track    | 0.5          | spec       | spec_review | reviewer | Claude | 0.08          | claude-opus-4-8 |
    And artifact quality:
      | instance | track_id | to_state | role | artifact | artifact_quality_0_10 | band     |
      | inst_a   | track    | spec     | doer | spec.md  | 6.0                   | mediocre |
      | inst_b   | track    | spec     | doer | spec.md  | 8.0                   | strong   |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    And artifact quality is folded into the scorecard steps for kind "track"
    Then the scorecard step from "start" to "spec" role "doer" has mean_quality permille 60 sample_count 1
    And the scorecard step from "start" to "spec" role "doer" has artifact_quality 7 sample_count 2
    And the scorecard step from "spec" to "spec_review" role "reviewer" has no artifact_quality

  Scenario: artifact quality with no matching coherence step is appended as its own row
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state | role | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | start      | spec     | doer | Claude | 0.06          | claude-opus-4-8 |
    And artifact quality:
      | instance | track_id | to_state | role | artifact  | artifact_quality_0_10 | band |
      | inst_a   | track    | plan     | doer | plan.md   | 5.0                   | mediocre |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    And artifact quality is folded into the scorecard steps for kind "track"
    Then the scorecard summary has 2 steps
    And the scorecard step from "" to "plan" role "doer" has artifact_quality 5 sample_count 1

  Scenario: artifact quality from a different kind is excluded
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state | role | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | start      | spec     | doer | Claude | 0.06          | claude-opus-4-8 |
    And artifact quality:
      | instance | track_id       | to_state | role | artifact | artifact_quality_0_10 | band     |
      | inst_x   | other_playbook | spec     | doer | spec.md  | 9.0                   | strong   |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    And artifact quality is folded into the scorecard steps for kind "track"
    Then the scorecard step from "start" to "spec" role "doer" has no artifact_quality

  Scenario: a step reachable via more than one edge dedupes into ONE row, not one per from_state
    # Regression: the fold used to key by (from_state, to_state, role), so a
    # step like plan_review — reachable both from plan and (skip-plan) from
    # spec_review directly — produced DUPLICATE rows the Atlas rendered 2-3x
    # over. Keying by (to_state, role) folds them into one.
    Given scorecards:
      | instance | track_id | mean_quality | from_state  | to_state    | role | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | plan        | plan_review | doer | Claude | 0.2           | claude-opus-4-8 |
      | inst_b   | track    | 0.5          | spec_review | plan_review | doer | Claude | 0.4           | claude-opus-4-8 |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    Then the scorecard summary has 1 steps
    And the scorecard step from "plan" to "plan_review" role "doer" has mean_quality permille 300 sample_count 2

  Scenario: overall artifact quality is the harsh 0-10 mean across every artifact-graded step, weighted by sample count
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state     | role | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | gather     | model        | doer | Claude | 0.06          | claude-opus-4-8 |
      | inst_a   | track    | 0.5          | model      | model_review | doer | Claude | 0.08          | claude-opus-4-8 |
    And artifact quality:
      | instance | track_id | to_state     | role | artifact | artifact_quality_0_10 | band     |
      | inst_a   | track    | model        | doer | model.md | 8.0                   | strong   |
      | inst_b   | track    | model        | doer | model.md | 6.0                   | mediocre |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    And artifact quality is folded into the scorecard steps for kind "track"
    And overall artifact quality is folded from the scorecard steps
    Then the scorecard summary has overall_artifact_quality 7 sample_count 2

  Scenario: no artifact-graded step folds to an honest zero-sample overall artifact quality
    Given scorecards:
      | instance | track_id | mean_quality | from_state | to_state | role | actor  | quality_score | model           |
      | inst_a   | track    | 0.5          | start      | spec     | doer | Claude | 0.06          | claude-opus-4-8 |
    When the scorecard measurements are folded for kind "track" with recent limit 10
    And overall artifact quality is folded from the scorecard steps
    Then the scorecard summary has overall_artifact_quality 0 sample_count 0
