Feature: QueryPort::read_op_log returns the persisted op log (BP1, D-4)

  # read_op_log(artifact_path, target_document) returns the persisted OpLog,
  # mirroring read_activity_entries' empty-collection-when-absent convention.

  Scenario: reading an absent op log yields an empty log (fs adapter)
    Given an op-log read fs hearth at "tracks/20260604T2114_amend"
    When read_op_log on fs is called for "tracks/20260604T2114_amend" document "spec"
    Then the read op log is empty

  Scenario: reading a persisted op log returns the recorded entries (fs adapter)
    Given an op-log read fs hearth at "tracks/20260604T2114_amend"
    And the fs op-log for "tracks/20260604T2114_amend" document "spec" has been seeded with op_id "op-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "seeded"
    When read_op_log on fs is called for "tracks/20260604T2114_amend" document "spec"
    Then the read op log has length 1
    And the read op log entry 0 has op_id "op-0"

  Scenario: reading an absent op log yields an empty log (in-memory adapter)
    Given an in-memory query adapter seeded with artifact "tracks/20260604T2114_amend" kind "track" state "completed"
    When read_op_log in memory is called for "tracks/20260604T2114_amend" document "spec"
    Then the in-memory read op log is empty

  Scenario: an in-memory query adapter seeded with an op log returns it
    Given an in-memory query adapter seeded with op log for artifact "tracks/20260604T2114_amend" document "spec" with op_id "op-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "seeded"
    When read_op_log in memory is called for "tracks/20260604T2114_amend" document "spec"
    Then the in-memory read op log has length 1
    And the in-memory read op log entry 0 has op_id "op-0"
