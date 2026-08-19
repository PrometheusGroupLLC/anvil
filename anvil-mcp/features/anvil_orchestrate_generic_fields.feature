Feature: anvil_orchestrate carries generic required fields through the missing-fields loop
  Kit-action playbooks declare required fields outside the builtin set (e.g.
  lore_query: question, requester). When a begin-mode handoff omits them, the
  shim returns a structured missing_required_fields outcome whose next_call
  pre-keys a `fields` object with exactly the missing generic field names (empty
  values) so the calling agent sees what to fill. When the fields are supplied
  in the `fields` argument, the kit-action playbook begins.

  Scenario: begin-mode kit-action with no generic fields returns a pre-keyed fields object
    When a surface begins a kit_action through anvil_orchestrate with no generic fields
    Then the handoff is not an error
    And the handoff outcome is "missing_required_fields"
    And the handoff missing fields are "question,requester"
    And the handoff next_call carries a fields object pre-keyed with "question,requester"

  Scenario: begin-mode kit-action supplying the generic fields begins the playbook
    When a surface begins a kit_action through anvil_orchestrate with fields question "what is anvil" requester "Nick"
    Then the handoff is not an error
    And the handoff outcome is not "missing_required_fields"
