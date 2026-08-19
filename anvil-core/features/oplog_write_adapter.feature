Feature: OpLogWritePort append-and-readback fidelity (BP1, D-4)

  # A narrow append-only persistence seam for the structured per-document op log.
  # Modeled on ActivityWritePort: atomic, never touches sibling files. The op-log
  # file `<artifact>/<target_document>.amendments.yaml` is WHOLLY owned by this
  # port, so the fs adapter re-serializes the whole OpLog (#[serde(transparent)]).

  Scenario: append one op log entry to the fs adapter then read it back equal
    Given an op-log write fs hearth at "tracks/20260604T2114_amend"
    When append_op on fs is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-20260604T211500Z-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "a new AC"
    Then the fs op-log file at "tracks/20260604T2114_amend" document "spec" deserializes to a log of length 1
    And the fs op-log file at "tracks/20260604T2114_amend" document "spec" entry 0 has op_id "op-20260604T211500Z-0"
    And the fs op-log file at "tracks/20260604T2114_amend" document "spec" entry 0 has target_id "AC9"

  Scenario: a second append to the same document extends the same file in FIFO order
    Given an op-log write fs hearth at "tracks/20260604T2114_amend"
    When append_op on fs is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-20260604T211500Z-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "first"
    And append_op on fs is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-20260604T211600Z-1" accepted_at "2026-06-04T21:16:00Z" seq 1 target_id "AC9" op_kind "revise" new_kind "" body "second"
    Then the fs op-log file at "tracks/20260604T2114_amend" document "spec" deserializes to a log of length 2
    And the fs op-log file at "tracks/20260604T2114_amend" document "spec" ordered op_id sequence is "op-20260604T211500Z-0,op-20260604T211600Z-1"

  Scenario: a different target_document writes a separate file, never cross-contaminating
    Given an op-log write fs hearth at "tracks/20260604T2114_amend"
    When append_op on fs is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-spec-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "spec body"
    And append_op on fs is called for "tracks/20260604T2114_amend" document "plan" with op_id "op-plan-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "phase-1" op_kind "add" new_kind "phase" body "plan body"
    Then the fs op-log file at "tracks/20260604T2114_amend" document "spec" deserializes to a log of length 1
    And the fs op-log file at "tracks/20260604T2114_amend" document "plan" deserializes to a log of length 1
    And the fs op-log file at "tracks/20260604T2114_amend" document "spec" entry 0 has op_id "op-spec-0"
    And the fs op-log file at "tracks/20260604T2114_amend" document "plan" entry 0 has op_id "op-plan-0"

  Scenario: the in-memory op-log adapter has the same append-and-readback contract
    Given an in-memory op-log adapter
    When append_op in memory is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-mem-0" accepted_at "2026-06-04T21:15:00Z" seq 0 target_id "AC9" op_kind "add" new_kind "acceptance_criterion" body "mem body"
    And append_op in memory is called for "tracks/20260604T2114_amend" document "spec" with op_id "op-mem-1" accepted_at "2026-06-04T21:16:00Z" seq 1 target_id "AC9" op_kind "revise" new_kind "" body "mem second"
    Then the in-memory op-log for "tracks/20260604T2114_amend" document "spec" has length 2
    And the in-memory op-log for "tracks/20260604T2114_amend" document "spec" ordered op_id sequence is "op-mem-0,op-mem-1"
