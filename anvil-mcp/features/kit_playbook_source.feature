Feature: Kit playbook source-of-truth directory
  The anvil/playbooks/track_lifecycle/ directory is the canonical source of truth
  for the track_lifecycle playbook definition. These scenarios assert that all
  required files exist and are structurally valid before the kit is assembled.

  Scenario: machine.yaml exists and is valid YAML
    Given the playbook file "playbooks/track_lifecycle/machine.yaml" is loaded
    Then the playbook file "playbooks/track_lifecycle/machine.yaml" exists in the workspace

  Scenario: definition.md exists
    Then the playbook file "playbooks/track_lifecycle/definition.md" exists in the workspace

  Scenario: status.yaml exists with state active
    Given the playbook file "playbooks/track_lifecycle/status.yaml" is loaded
    Then the playbook file has state "active"

  Scenario: evidence.md exists
    Then the playbook file "playbooks/track_lifecycle/evidence.md" exists in the workspace

  Scenario: amendments.md exists
    Then the playbook file "playbooks/track_lifecycle/amendments.md" exists in the workspace

  Scenario: review.md exists
    Then the playbook file "playbooks/track_lifecycle/review.md" exists in the workspace

  Scenario: skills/info/SKILL.md exists
    Then the playbook file "playbooks/track_lifecycle/skills/info/SKILL.md" exists in the workspace

  Scenario: skills/info/SKILL.md contains the word metadata
    Then the file "playbooks/track_lifecycle/skills/info/SKILL.md" contains the word "metadata"
