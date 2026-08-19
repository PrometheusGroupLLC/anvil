Feature: Track review gates route honestly via complete(satisfaction)
  Every track review gate is now is_review_gate:true with satisfaction-tokened
  exits, so a reviewer records an honest verdict through the complete RPC and the
  engine routes on that verdict: "satisfied" advances to the forward target;
  "full_revision" routes to the paired revision state. This exercises the honest
  routing end-to-end for the five gates not already covered by a dedicated
  feature (plan_review, impl_phase_review, impl_review, reflection_review,
  amend_review), and locks the two invariants the gate-flip introduced: snapshot
  still fires from a flipped gate (the escape hatch survives the flip), and park
  stays snapshot-only (complete(satisfaction: "abandoned") is rejected).

  # spec_review's honest routing is covered by dedicated features and is NOT
  # duplicated here:
  #   complete_rpc_reviewer_satisfied.feature     satisfied            -> plan
  #   complete_rpc_full_revision.feature          full_revision        -> spec_revision
  #   complete_rpc_address_in_next_step.feature   address_in_next_step -> plan (carry-forward)

  Scenario: plan_review reviewer satisfied advances to implementing
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0300_plan_review_satisfied/    | plan_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0300_plan_review_satisfied |
      | actor_name     | Reviewer-300001                            |
      | actor_type     | agent                                      |
      | actor_model    | test-model                                 |
      | actor_provider | test                                       |
      | satisfaction   | satisfied                                  |
    Then the complete RPC response new_state is "implementing"
    And the resolved state of "tracks/20260707T0300_plan_review_satisfied" in the hearth is "implementing"
    And a hearth transition event for "tracks/20260707T0300_plan_review_satisfied" contains "to: implementing"
    And a hearth transition event for "tracks/20260707T0300_plan_review_satisfied" contains "role: review"

  Scenario: plan_review reviewer full_revision routes to plan_revision
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0301_plan_review_revision/     | plan_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0301_plan_review_revision |
      | actor_name     | Reviewer-300002                           |
      | actor_type     | agent                                     |
      | actor_model    | test-model                                |
      | actor_provider | test                                      |
      | satisfaction   | full_revision                             |
    Then the complete RPC response new_state is "plan_revision"
    And the resolved state of "tracks/20260707T0301_plan_review_revision" in the hearth is "plan_revision"
    And a hearth transition event for "tracks/20260707T0301_plan_review_revision" contains "to: plan_revision"
    And a hearth transition event for "tracks/20260707T0301_plan_review_revision" contains "role: review"

  Scenario: impl_phase_review reviewer satisfied advances to implementing
    Given a hearth directory with the following structure:
      | path                                              | state             |
      | proposals/20260411T2021_anvil_workflow_engine/    | active            |
      | tracks/20260707T0310_impl_phase_satisfied/        | impl_phase_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0310_impl_phase_satisfied |
      | actor_name     | Reviewer-310001                           |
      | actor_type     | agent                                     |
      | actor_model    | test-model                                |
      | actor_provider | test                                      |
      | satisfaction   | satisfied                                 |
    Then the complete RPC response new_state is "implementing"
    And the resolved state of "tracks/20260707T0310_impl_phase_satisfied" in the hearth is "implementing"
    And a hearth transition event for "tracks/20260707T0310_impl_phase_satisfied" contains "to: implementing"
    And a hearth transition event for "tracks/20260707T0310_impl_phase_satisfied" contains "role: review"

  Scenario: impl_phase_review reviewer full_revision routes back to implementing (Option B)
    Given a hearth directory with the following structure:
      | path                                              | state             |
      | proposals/20260411T2021_anvil_workflow_engine/    | active            |
      | tracks/20260707T0311_impl_phase_revision/         | impl_phase_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0311_impl_phase_revision |
      | actor_name     | Reviewer-310002                          |
      | actor_type     | agent                                    |
      | actor_model    | test-model                               |
      | actor_provider | test                                     |
      | satisfaction   | full_revision                            |
    Then the complete RPC response new_state is "implementing"
    And the resolved state of "tracks/20260707T0311_impl_phase_revision" in the hearth is "implementing"
    And a hearth transition event for "tracks/20260707T0311_impl_phase_revision" contains "to: implementing"
    And a hearth transition event for "tracks/20260707T0311_impl_phase_revision" contains "role: review"

  Scenario: impl_review reviewer satisfied advances to reflecting
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0320_impl_review_satisfied/    | impl_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0320_impl_review_satisfied |
      | actor_name     | Reviewer-320001                            |
      | actor_type     | agent                                      |
      | actor_model    | test-model                                 |
      | actor_provider | test                                       |
      | satisfaction   | satisfied                                  |
    Then the complete RPC response new_state is "reflecting"
    And the resolved state of "tracks/20260707T0320_impl_review_satisfied" in the hearth is "reflecting"
    And a hearth transition event for "tracks/20260707T0320_impl_review_satisfied" contains "to: reflecting"
    And a hearth transition event for "tracks/20260707T0320_impl_review_satisfied" contains "role: review"

  Scenario: impl_review reviewer full_revision routes to impl_revision
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0321_impl_review_revision/     | impl_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0321_impl_review_revision |
      | actor_name     | Reviewer-320002                           |
      | actor_type     | agent                                     |
      | actor_model    | test-model                                |
      | actor_provider | test                                      |
      | satisfaction   | full_revision                             |
    Then the complete RPC response new_state is "impl_revision"
    And the resolved state of "tracks/20260707T0321_impl_review_revision" in the hearth is "impl_revision"
    And a hearth transition event for "tracks/20260707T0321_impl_review_revision" contains "to: impl_revision"
    And a hearth transition event for "tracks/20260707T0321_impl_review_revision" contains "role: review"

  Scenario: reflection_review reviewer satisfied advances to completed
    Given a hearth directory with the following structure:
      | path                                              | state             |
      | proposals/20260411T2021_anvil_workflow_engine/    | active            |
      | tracks/20260707T0330_reflection_review_satisfied/ | reflection_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0330_reflection_review_satisfied |
      | actor_name     | Reviewer-330001                                  |
      | actor_type     | agent                                            |
      | actor_model    | test-model                                       |
      | actor_provider | test                                             |
      | satisfaction   | satisfied                                        |
    Then the complete RPC response new_state is "completed"
    And the resolved state of "tracks/20260707T0330_reflection_review_satisfied" in the hearth is "completed"
    And a hearth transition event for "tracks/20260707T0330_reflection_review_satisfied" contains "to: completed"
    And a hearth transition event for "tracks/20260707T0330_reflection_review_satisfied" contains "role: review"

  Scenario: reflection_review reviewer full_revision routes to reflection_revision
    Given a hearth directory with the following structure:
      | path                                              | state             |
      | proposals/20260411T2021_anvil_workflow_engine/    | active            |
      | tracks/20260707T0331_reflection_review_revision/  | reflection_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0331_reflection_review_revision |
      | actor_name     | Reviewer-330002                                 |
      | actor_type     | agent                                           |
      | actor_model    | test-model                                      |
      | actor_provider | test                                            |
      | satisfaction   | full_revision                                   |
    Then the complete RPC response new_state is "reflection_revision"
    And the resolved state of "tracks/20260707T0331_reflection_review_revision" in the hearth is "reflection_revision"
    And a hearth transition event for "tracks/20260707T0331_reflection_review_revision" contains "to: reflection_revision"
    And a hearth transition event for "tracks/20260707T0331_reflection_review_revision" contains "role: review"

  Scenario: amend_review reviewer satisfied advances to completed
    Given a hearth directory with the following structure:
      | path                                           | state        |
      | proposals/20260411T2021_anvil_workflow_engine/ | active       |
      | tracks/20260707T0340_amend_review_satisfied/   | amend_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0340_amend_review_satisfied |
      | actor_name     | Reviewer-340001                             |
      | actor_type     | agent                                       |
      | actor_model    | test-model                                  |
      | actor_provider | test                                        |
      | satisfaction   | satisfied                                   |
    Then the complete RPC response new_state is "completed"
    And the resolved state of "tracks/20260707T0340_amend_review_satisfied" in the hearth is "completed"
    And a hearth transition event for "tracks/20260707T0340_amend_review_satisfied" contains "to: completed"
    And a hearth transition event for "tracks/20260707T0340_amend_review_satisfied" contains "role: review"

  Scenario: amend_review reviewer full_revision routes to amend_revision
    Given a hearth directory with the following structure:
      | path                                           | state        |
      | proposals/20260411T2021_anvil_workflow_engine/ | active       |
      | tracks/20260707T0341_amend_review_revision/    | amend_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0341_amend_review_revision |
      | actor_name     | Reviewer-340002                            |
      | actor_type     | agent                                      |
      | actor_model    | test-model                                 |
      | actor_provider | test                                       |
      | satisfaction   | full_revision                              |
    Then the complete RPC response new_state is "amend_revision"
    And the resolved state of "tracks/20260707T0341_amend_review_revision" in the hearth is "amend_revision"
    And a hearth transition event for "tracks/20260707T0341_amend_review_revision" contains "to: amend_revision"
    And a hearth transition event for "tracks/20260707T0341_amend_review_revision" contains "role: review"

  # --- Snapshot-safety: the escape hatch survives the gate flip. Nothing else
  # exercises snapshot from an is_review_gate:true track state now that the
  # flagship lifecycle advances its gates via complete(satisfied). ---

  Scenario: snapshot still fires from a flipped gate (plan_review -> implementing)
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0350_snapshot_from_plan_gate/  | plan_review |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/20260707T0350_snapshot_from_plan_gate |
      | to_state       | implementing                                 |
      | actor_name     | Reviewer-350001                              |
      | actor_role     | implement                                    |
      | actor_type     | agent                                        |
      | actor_model    | test-model                                   |
      | actor_provider | test                                         |
    Then the snapshot RPC response success is "true"
    And the resolved state of "tracks/20260707T0350_snapshot_from_plan_gate" in the hearth is "implementing"

  Scenario: snapshot still fires from a flipped gate (reflection_review -> completed)
    Given a hearth directory with the following structure:
      | path                                                | state             |
      | proposals/20260411T2021_anvil_workflow_engine/      | active            |
      | tracks/20260707T0351_snapshot_from_reflection_gate/ | reflection_review |
    And the engine is started with that hearth
    When the snapshot RPC is called with:
      | artifact_path  | tracks/20260707T0351_snapshot_from_reflection_gate |
      | to_state       | completed                                          |
      | actor_name     | Reviewer-351001                                    |
      | actor_role     | complete                                           |
      | actor_type     | agent                                              |
      | actor_model    | test-model                                         |
      | actor_provider | test                                               |
    Then the snapshot RPC response success is "true"
    And the resolved state of "tracks/20260707T0351_snapshot_from_reflection_gate" in the hearth is "completed"

  # --- Park stays snapshot-only: `abandoned` is a machine satisfaction on the
  # park edges but is deliberately excluded from the track complete whitelist, so
  # complete(satisfaction: "abandoned") from a gate is rejected. ---

  Scenario: complete(abandoned) from a flipped gate is rejected — park stays snapshot-only
    Given a hearth directory with the following structure:
      | path                                           | state       |
      | proposals/20260411T2021_anvil_workflow_engine/ | active      |
      | tracks/20260707T0360_complete_abandoned/       | plan_review |
    And the engine is started with that hearth
    When the complete RPC is called with:
      | artifact_path  | tracks/20260707T0360_complete_abandoned |
      | actor_name     | Reviewer-360001                         |
      | actor_type     | agent                                   |
      | actor_model    | test-model                              |
      | actor_provider | test                                    |
      | satisfaction   | abandoned                               |
    Then the complete RPC returns gRPC status "INVALID_ARGUMENT"
    And the complete RPC error message contains "satisfaction_unknown"
    And the complete RPC error message contains "abandoned"
