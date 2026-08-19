Feature: A machine that declares the pre-migration name field is still satisfiable
  A `machine.yaml` is persisted hearth data. This repo reads those definitions;
  it does not own them, and the live builder machine in anvil-hearth still
  declares the pre-migration spelling of the playbook-name field. So the read
  side must recognise that declaration.

  When it does not, the failure is silent and total: the shim reports the field
  missing, hands the caller a bag key to fill, reads the value from somewhere
  else entirely, and reports it missing again. The agent does exactly what it is
  told, forever. That loop is what these scenarios exist to make impossible —
  the last one issues the returned next_call literally and requires progress.

  There is exactly ONE client-facing argument for this value and it is
  `playbook_name`. Recognising a name a definition declares is not an alias on
  the tool surface.

  Scenario: the declared field is reported missing when nothing is supplied
    When a surface begins an action whose machine declares the pre-migration name field, supplying nothing
    Then the handoff is not an error
    And the handoff outcome is "missing_required_fields"

  Scenario: the caller is pointed at the canonical argument, never at a bag key
    When a surface begins an action whose machine declares the pre-migration name field, supplying nothing
    Then the handoff next_call carries no fields object
    And the handoff next_call arguments name "playbook_name"

  Scenario: obeying the handoff exactly makes progress
    When a surface begins an action whose machine declares the pre-migration name field and then obeys the handoff exactly
    Then the handoff is not an error
    And the handoff outcome is not "missing_required_fields"
