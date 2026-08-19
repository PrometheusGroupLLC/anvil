Feature: anvil_orchestrate preflights required fields before beginning

  # route_response_mirrors_begin H1: a single route with MISSING required fields is
  # advisory, not gated. It returns the begin-equivalent guidance AND the begin call
  # AND required_fields AND the missing_fields list — and performs NO begin. The
  # model supplies the fields when it issues the begin call.
  Scenario: route single with missing required fields returns advisory guidance plus the begin call
    When a surface ships "start a track" into anvil_orchestrate in route-mode with no creation fields
    Then the handoff is not an error
    And the handoff is advisory single for kind "track"
    And the handoff carries a non-empty route guidance
    And the handoff missing fields are "name,parent_id,approver"
    And the handoff next_call begins kind "track"
    And the handoff next_call re-invokes "anvil_orchestrate" carrying required field placeholders
    And no track artifact was created by the handoff

  Scenario: begin-mode with supplied required fields creates the track and serves the spec hook
    When a surface begins a track through anvil_orchestrate with track_name "Router C3 readiness" parent_id "seedprop" approver "Nick"
    Then the handoff is not an error
    And the handoff routes to playbook "track_lifecycle" at state "spec" role "doer"
    And the handoff carries a non-empty hook context_text
    And the handoff carries the step intent and expected_output

  # route_response_mirrors_begin H4: a directory-less run playbook with no required
  # fields is now ADVISORY — it returns the begin call instead of auto-beginning.
  Scenario: directory-less run playbook with no required fields returns advisory begin call
    When a surface ships "ask lore about plans" into anvil_orchestrate in route-mode with conversation_id "turn-c3-lore-preserved" and ctx org "Foundation"
    Then the handoff is not an error
    And the handoff is advisory single for kind "lore_query"
    And the handoff next_call begins kind "lore_query"

