Feature: PlaybookMachine register field — driven vs free

  BP1 (workflow_routing_layer): a machine declares its register
  (driven | free). Driven machines are router candidates; free kinds
  (spark/decision/learning) are invocable out-of-band and excluded. The field
  is `#[serde(default)]` (default driven) so existing authored machines need no
  change, and the struct stays `deny_unknown_fields`.

  Scenario: a machine.yaml with register: free loads and reports free
    Given a playbook machine.yaml with content:
      """
      kind: spark
      directory: sparks
      registry: sparks.md
      description: A free generative kind.
      register: free
      required_fields: []
      roles: [doer]
      states:
        - name: active
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the register-field loader parses the machine
    Then the parse succeeds and the machine register is "free"

  Scenario: a machine.yaml without a register field defaults to driven
    Given a playbook machine.yaml with content:
      """
      kind: track
      directory: tracks
      registry: tracks.md
      description: A driven workflow.
      required_fields: []
      roles: [doer]
      states:
        - name: active
          role_filters: []
          registry_section: active
          projection_targets: []
          is_review_gate: false
          is_terminal: false
      transitions: []
      """
    When the register-field loader parses the machine
    Then the parse succeeds and the machine register is "driven"

  Scenario: enumerating driven candidates excludes free machines
    Given a registry with a driven "track" machine and a free "spark" machine
    When I select the driven candidates
    Then the driven candidate kinds include "track"
    And the driven candidate kinds do not include "spark"
