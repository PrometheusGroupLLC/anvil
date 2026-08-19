Feature: the join coverage over /ws — the surface a person actually reaches

  The engine's user-facing surface is the /ws bridge, not the gRPC port: a human
  or a Playbooks-UI session talks JSON-RPC to /ws, and a number reachable only by
  a gRPC client is computable in a sense that does not reach the person who has to
  read it. What is proven here is REACHABILITY and NON-DIVERGENCE — the same fold,
  the same per-hearth report, the same privacy contract, at the other surface.
  The matching arithmetic is proven at the core seam and the multi-hearth
  resolution rules at the RPC seam; neither is re-proven here.

  Scenario: the /ws payload carries a report per hearth and no pooled total
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-b            | proposal      | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
    And the engine is started with both permitted hearths
    When a join coverage request is sent over /ws naming both hearths
    Then the JoinCoverage response has 2 per-hearth reports
    And the per-hearth report labels are "hearth-primary,hearth-secondary" in any order
    And the per-hearth report for "hearth-primary" counts "joined" as 1
    And the per-hearth report for "hearth-primary" buckets sum to its declared denominators
    And the serialized report exposes no pooled coverage key
    And the report filter version is the "JOIN_FILTER_VERSION" constant

  # THE NON-DIVERGENCE PROOF. The two surfaces are one fold or they are two
  # numbers, and two numbers is the failure this scenario exists to catch. Both
  # calls run against the SAME engine over the SAME hearths, so any difference is
  # the arm having derived its own answer.
  Scenario: the /ws payload is identical to the gRPC JoinCoverage response
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome    | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single     | true              |
      | 2026-08-01T01:00:00Z | conv-b            | proposal      | single     | true              |
      | 2026-08-01T02:00:00Z | conv-c            | -             | candidates | true              |
    And a delivery log secondary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-d            | track         | single  | true              |
    And the primary hearth activity log is seeded with turns:
      | command | artifact_kind | at                   | conversation_hash | playbook_run_id | to_state    |
      | begin   | track         | 2026-08-01T00:30:00Z | conv-a            | run-1           | in_progress |
      | begin   | proposal      | 2026-08-01T03:00:00Z | conv-z            | run-2           | in_progress |
    And the engine is started with both permitted hearths
    When a join coverage request is sent over /ws naming both hearths
    And the same join coverage question is asked over gRPC
    Then the two JoinCoverage responses are identical
    And the per-hearth report for "hearth-primary" counts "joined" as 1
    # Two kind-bearing rows, not three: the `candidates` row delivered a menu and
    # names no kind, so it is no episode.
    And the per-hearth report for "hearth-primary" counts "episode_denominator" as 2
    And the per-hearth report for "hearth-primary" counts "menu_delivered" as 1

  # D13 #4, instantiated at this surface. The privacy contract is asserted over
  # the SERIALIZED key set against a declared allowlist, never as a hand-listed
  # set of forbidden names: a raw path, a salt, a conversation id or a pooled
  # total all fail the same way, and so does any key a future change adds without
  # amending the list.
  Scenario: every key on the /ws join coverage payload is on the declared allowlist
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash        | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-raw-identity-marker | track         | single  | true              |
    And a telemetry salt file containing "brine-test-salt" in the "primary" hearth
    And the engine is started with that hearth
    When a join coverage request is sent over /ws
    Then every key on the /ws join coverage payload is on the declared allowlist
    And the serialized JoinCoverage response does not contain raw text "conv-raw-identity-marker"
    And the serialized JoinCoverage response does not contain raw text "brine-test-salt"
    And the serialized JoinCoverage response contains no hearth path

  # The fail-closed rule reaches this surface too. A partial one-hearth report
  # answers a different question than the one asked while looking like an answer
  # to this one.
  Scenario: a /ws request naming one permitted and one non-permitted hearth is refused with no report
    Given a delivery log primary hearth seeded with rows:
      | at                   | conversation_hash | guidance_kind | outcome | guidance_produced |
      | 2026-08-01T00:00:00Z | conv-a            | track         | single  | true              |
    And the engine is started with that hearth
    When a join coverage request is sent over /ws naming the primary hearth and the unpermitted hearth
    Then the /ws JSON-RPC response is an error envelope
    And the /ws JSON-RPC error.data.code is "permission_denied"
    And the /ws JSON-RPC response carries no result
    And no JoinCoverage report was returned
