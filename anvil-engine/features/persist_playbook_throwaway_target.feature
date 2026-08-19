Feature: PersistPlaybook writes the THROWAWAY target home, leaving anvil-hearth untouched (track 1a, BP4, A4/A7)
  The headline boundary: persisting a throwaway test machine to a SEPARATE temp
  owner-home writes there and adds NO entry under the engine's own hearth
  (the cache is not the target — A4 / MEDIUM-2). The machine is built from the
  conformant fixture (no coupling to any real content) — an outcome_predicate plus
  a begin hook on its non-terminal state, so it clears the enforcing WRITE boundary —
  and is registry-resolvable from the owner-home but not from the engine hearth.

  Scenario: persist to a temp owner-home leaves the engine hearth playbooks untouched
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    And the engine hearth playbooks entry count is recorded
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700020 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC response kind is "throwaway_kind"
    And a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the persist owner-home
    And the engine hearth playbooks entry count is unchanged
    And a fresh registry from the persist owner-home resolves kind "throwaway_kind"
    And a fresh registry from the engine hearth does not resolve kind "throwaway_kind"

  # C-d.1 round 4. The `HearthRegistrationBlocked` -> FAILED_PRECONDITION mapping
  # was introduced with NO falsifiable coverage anywhere: the variant appeared in
  # exactly one production line and the code string in exactly one feature, at the
  # DOMAIN seam, so the RPC's status code rested on reading. INVALID_ARGUMENT is
  # the tempting mapping and the wrong one — the request is fine; the target
  # hearth is not in a state that may be registered into — and nothing could tell
  # the two apart. The scenario above is this one's control: the same RPC into an
  # unblocked owner-home returns a kind, not a status.
  Scenario: a persist into a hearth whose registration is blocked is FAILED_PRECONDITION
    Given a hearth directory with the following structure:
      | path                                          | state  |
      | proposals/20260411T2021_anvil_workflow_engine/ | active |
    And the engine is started with that hearth
    And a separate temp owner-home directory
    And that owner-home carries definitions under BOTH hearth roots
    When the PersistPlaybook RPC is called for kind "throwaway_kind" under that owner-home with:
      | actor_name     | Persist-Doer-700021 |
      | actor_type     | agent           |
      | actor_model    | claude-opus-4-8 |
      | actor_provider | anthropic       |
    Then the PersistPlaybook RPC returns gRPC status "FAILED_PRECONDITION"
    And the PersistPlaybook RPC error message contains "hearth_registration_blocked"
