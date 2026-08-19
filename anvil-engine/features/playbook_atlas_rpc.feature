Feature: PlaybookAtlas RPC surfaces the honest measurement status of every registry kind
  The read-only PlaybookAtlas query backs the in-app Atlas measurement surface. It
  folds every registry-resolved playbook machine in the hearth into a per-kind
  measurement summary — owner kit + lifecycle state (from status.yaml), register,
  and the SHARED `playbook_integrity` fold (rubber-stamp gates, unmeasured states,
  anchors_count, grader_declared). INVALID machines are INCLUDED as degenerate
  entries carrying their load error, so the Atlas shows breakage instead of hiding
  it. The payload is split list vs detail: `playbook_atlas` returns the sidebar rows
  (kind, owner_kit, state, register, integrity, calibration, counts); the companion
  `playbook_atlas_detail(kind)` returns the full states + edges + success_rubric for
  one selected kind. The gRPC RPC and the /ws `playbook_atlas` method fold the SAME
  core path, so the two surfaces can never diverge.

  Scenario: the gRPC PlaybookAtlas RPC returns every registry kind incl. an invalid-with-error entry
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When the PlaybookAtlas RPC is called
    Then the atlas has an entry for kind "playbook_generation"
    And the atlas has an entry for kind "council_experiment_design"
    And the atlas has an entry for kind "free_probe"
    And the atlas entry "broken_playbook" reports loads false with an error
    And the atlas entry for kind "playbook_generation" reports loads true
    And the atlas entry for kind "council_experiment_design" reports rubber-stamp gate "design_review"
    And the atlas entry for kind "playbook_generation" reports no rubber-stamp gates
    And the atlas entry for kind "playbook_generation" has register "driven"
    And the atlas entry for kind "free_probe" has register "free"
    And the atlas entry for kind "playbook_generation" has owner_kit "forge-kit"
    And the atlas entry for kind "council_experiment_design" has owner_kit ""
    And the atlas entry for kind "playbook_generation" has calibration "seed"
    And the atlas entry for kind "free_probe" has calibration "none"
    # C-d.1 round 4. A directory under the canonical root the loader resolved
    # NOTHING from. The registry has always recorded it — "Recorded, not
    # swallowed... The projection states it" — and `excluded_directories()` had
    # no consumer anywhere, nor does the projection that reads it, so the record
    # was made and surfaced nowhere. It carries no load error of its own, so it
    # is the ONE case the invalid-artifact rows above cannot cover.
    And the atlas entry "20260909T0000_no_machine_dir" reports loads false with an error
    And the atlas entry "20260909T0000_no_machine_dir" reports error code "playbook_directory_not_loaded"
    # C-d.1 round 5 (L-2). The de-dupe that keeps an errored directory from being
    # listed twice compared an artifact ID against other entries' KINDS. Here the
    # machine's directory id and its kind DIFFER, and an unrelated empty
    # directory carries that kind as its basename: under the namespace mix it is
    # suppressed and the operator sees a directory on disk the atlas denies. Both
    # rows are asserted, so a de-dupe that over-suppresses AND one that
    # double-lists are each red.
    And the atlas lists both a loaded kind and a not-loaded directory named "atlas_shadow_kind"

  Scenario: the PlaybookAtlasDetail RPC carries the playbook_generation 4-dim/8-anchor rubric
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail has 4 rubric dimensions
    And the atlas detail rubric anchors_count is 8
    And the atlas detail rubric grader_declared is true
    And the atlas detail has a review-gate state "model_review"
    And the atlas detail has an edge from "model_review" to "completed" with required_satisfaction "satisfied"

  Scenario: the /ws playbook_atlas method folds the identical resolved atlas
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When a playbook_atlas JSON-RPC request is sent over /ws with hearth_path ""
    Then the /ws playbook_atlas result has an entry for kind "playbook_generation"
    And the /ws playbook_atlas entry for kind "council_experiment_design" reports rubber-stamp gate "design_review"
    And the /ws playbook_atlas entry "broken_playbook" reports loads false with an error
    And the /ws playbook_atlas entry for kind "free_probe" has register "free"

  Scenario: the /ws playbook_atlas_detail method returns the identical per-kind detail
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When a playbook_atlas_detail JSON-RPC request is sent over /ws for kind "playbook_generation"
    Then the /ws playbook_atlas_detail result has 4 rubric dimensions
    And the /ws playbook_atlas_detail result rubric anchors_count is 8

  Scenario: the PlaybookAtlasDetail RPC surfaces temper's real per-step scorecard quality
    # Reuses resolve_temper_home: "the engine is started with that hearth" sets
    # ANVIL_TEMPER_HOME to <hearth>/__temper_home__, the SAME root the §0 stream
    # writes under. The fixture writes scorecard.json files there directly —
    # the read is per-request (no caching), so writing after engine start still
    # lands in time for the RPC call below.
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.5":
      | from_state | to_state     | role | actor  | quality_score | model           |
      | gather     | model        | doer | Claude | 0.6           | claude-opus-4-8 |
      | model      | model_review | doer | Claude | 0.4           | claude-opus-4-8 |
    And a temper scorecard for instance "inst_b" kind "playbook_generation" mean_quality "0.7":
      | from_state | to_state | role | actor  | quality_score | model           |
      | gather     | model    | doer | Claude | 0.8           | claude-opus-4-8 |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail has measured_instance_count 2
    And the atlas detail has overall_mean_quality permille 600
    And the atlas detail step from "gather" to "model" role "doer" has mean_quality permille 700 sample_count 2
    And the atlas detail step from "model" to "model_review" role "doer" has mean_quality permille 400 sample_count 1
    And the atlas detail has 3 recent measurements

  Scenario: a kind with no temper scorecards on disk reports zero measurement, not an error
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When the PlaybookAtlasDetail RPC is called for kind "free_probe"
    Then the atlas detail has measured_instance_count 0
    And the atlas detail has overall_mean_quality permille 0
    And the atlas detail has 0 recent measurements

  Scenario: the PlaybookAtlasDetail RPC surfaces the REAL artifact-quality score alongside coherence
    # artifact_quality.json is a SIBLING of scorecard.json, written by a
    # SEPARATE grader — the real, harsh 0-10 anchored score of the artifact
    # the step produced, distinct from the generic coherence quality_score.
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state | role | actor  | quality_score | model           |
      | gather     | model    | doer | Claude | 0.06          | claude-opus-4-8 |
    And a temper artifact quality for instance "inst_a" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band     |
      | model    | doer | model.md | 6.0                    | mediocre |
    And a temper artifact quality for instance "inst_b" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band   |
      | model    | doer | model.md | 8.0                    | strong |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail step from "gather" to "model" role "doer" has artifact_quality 7 sample_count 2
    And the atlas detail step from "gather" to "model" role "doer" has mean_quality permille 60 sample_count 1

  Scenario: a kind with no artifact_quality.json on disk carries no artifact_quality, not a fabricated zero
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state | role | actor  | quality_score | model           |
      | gather     | model    | doer | Claude | 0.06          | claude-opus-4-8 |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail step from "gather" to "model" role "doer" has no artifact_quality

  Scenario: the ARTIFACT-QUALITY overall score, not coherence, is the headline number
    # This is the bug fix: the Atlas masthead used to show
    # overall_mean_quality (coherence, uniformly ~0.14 -> renders as 1.4) as
    # the headline. overall_artifact_quality is the HARSH 0-10 artifact score
    # and must be computed independently, weighted across every artifact-
    # graded step.
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state     | role | actor  | quality_score | model           |
      | gather     | model        | doer | Claude | 0.06          | claude-opus-4-8 |
      | model      | model_review | doer | Claude | 0.08          | claude-opus-4-8 |
    And a temper artifact quality for instance "inst_a" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band     |
      | model    | doer | model.md | 8.0                    | strong   |
    And a temper artifact quality for instance "inst_b" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band     |
      | model    | doer | model.md | 6.0                    | mediocre |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail has overall_artifact_quality 7 sample_count 2

  Scenario: a kind with no artifact-graded step reports an honest not-yet-graded overall artifact quality
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    When the PlaybookAtlasDetail RPC is called for kind "free_probe"
    Then the atlas detail has overall_artifact_quality 0 sample_count 0

  Scenario: a step reachable via more than one edge dedupes to ONE step_quality row in the RPC response
    # Regression: the fold used to key coherence rows by (from_state, to_state,
    # role), so a step reachable via two edges (e.g. model_review re-entered
    # both by "model" and by a revision loop) produced DUPLICATE rows the
    # Atlas rendered 2-3x over.
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state     | role | actor  | quality_score | model           |
      | gather     | model_review | doer | Claude | 0.2           | claude-opus-4-8 |
    And a temper scorecard for instance "inst_b" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state     | role | actor  | quality_score | model           |
      | model      | model_review | doer | Claude | 0.4           | claude-opus-4-8 |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail has exactly 1 step_quality row for to_state "model_review" role "doer"

  Scenario: the /ws playbook_atlas_detail method carries artifact_quality alongside coherence in step_quality
    # Regression: the /ws JSON mapping (the ACTUAL wire shape the frontend
    # consumes — it talks to the engine over /ws, never gRPC directly) used to
    # omit artifact_quality/artifact_sample_count entirely, so the frontend
    # silently always fell back to the misleading coherence score even when
    # the engine HAD computed a real artifact-quality measurement.
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state | role | actor  | quality_score | model           |
      | gather     | model    | doer | Claude | 0.06          | claude-opus-4-8 |
    And a temper artifact quality for instance "inst_a" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band   |
      | model    | doer | model.md | 8.0                    | strong |
    When a playbook_atlas_detail JSON-RPC request is sent over /ws for kind "playbook_generation"
    Then the /ws playbook_atlas_detail result step "model" role "doer" has artifact_quality 8 sample_count 1
    And the /ws playbook_atlas_detail result has overall_artifact_quality 8 sample_count 1

  Scenario: the recent-measurements feed carries the joined artifact-quality aggregate alongside coherence
    Given a playbook atlas engine hearth
    And the engine is started with that hearth
    And a temper scorecard for instance "inst_a" kind "playbook_generation" mean_quality "0.1":
      | from_state | to_state | role | actor  | quality_score | model           |
      | gather     | model    | doer | Claude | 0.06          | claude-opus-4-8 |
    And a temper artifact quality for instance "inst_a" kind "playbook_generation":
      | to_state | role | artifact | artifact_quality_0_10 | band   |
      | model    | doer | model.md | 8.0                    | strong |
    When the PlaybookAtlasDetail RPC is called for kind "playbook_generation"
    Then the atlas detail recent measurement 1 has artifact_quality 8 sample_count 1
