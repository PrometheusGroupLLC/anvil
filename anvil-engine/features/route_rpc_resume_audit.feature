Feature: Route RPC durably records resumed workflow guidance
  Anvil's audit record must distinguish guidance that resumes an open workflow
  from a turn where no workflow matched or a new workflow could be started.
  A resume record identifies the open artifact so adoption measurements can join
  the guidance back to the workflow instance it continued.

  Scenario: a continuation token records a resume for the open artifact
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260722T2030_k5_bind_begin_seam" in state "spec" begun for conversation "Conv-resume-audit" at "2026-07-22T20:30:00Z"
    And the engine is started with that hearth
    When the route RPC is called with message "continue", signal "", and conversation_id "Conv-resume-audit"
    Then the route resolution outcome is "resume"
    And the activity log sink has 1 records
    And the activity log resume record has outcome "resume"
    And the activity log resume record has call state "resume"
    And the activity log resume record identifies artifact "20260722T2030_k5_bind_begin_seam" kind "track"
    And the activity log sink has no route record with outcome "no_match"

  Scenario: a genuine no-match turn remains a no-match audit record
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the engine is started with that hearth
    When the route RPC is called with message "zzz nothing here", signal "", and conversation_id "Conv-no-match-audit"
    Then the route resolution outcome is "no_match"
    And the activity log sink has 1 records
    And the activity log sink has a record command "route" outcome "no_match" artifact_kind ""
    And the activity log route record has call state "no_playbook_run"

  Scenario: a resumed turn does not create an abstention
    Given a route hearth seeded with the knowledge_lifecycle machine
    And the route hearth has an open "track" artifact "20260722T2030_k5_bind_begin_seam" in state "spec" begun for conversation "Conv-resume-ledger" at "2026-07-22T20:30:00Z"
    And the abstention ledger flag is on
    And the engine is started with that hearth
    When the route-turn hook receives continuation message "continue" for conversation "Conv-resume-ledger"
    Then the route-turn guidance resumes artifact "20260722T2030_k5_bind_begin_seam"
    And the abstention ledger file does not exist
