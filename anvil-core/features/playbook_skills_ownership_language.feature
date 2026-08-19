# Documentation-correctness test — not an engine behavioral contract.
# These scenarios assert that ownership language in skill files and docs
# has been corrected per the playbook-skills-naming track. They use grep-based
# file-content assertions, not engine state. See: tracks/20260426T1515_workflow_skills_naming/

Feature: Playbook skills ownership language is correct in documentation
  The forge-build dev-playbook lifecycle skills must not claim Forge-app ownership
  in their descriptions or documentation. Slash commands are playbook skills, not
  "forge skills." A bash automation tool ("forge CLI") does not currently exist
  and must not be conflated with these slash commands.

  Scenario: AGENTS.md Skills section uses playbook-skill language, not forge-skills language
    Given the file at "anvil/AGENTS.md" exists
    When the Skills section is scanned for "forge skills" ownership claims
    Then no lines in the Skills section contain the phrase "forge skills"
    And no lines in the Skills section contain the phrase "Forge skills"

  Scenario: AGENTS.md Skills section contains no forge CLI conflation
    Given the file at "anvil/AGENTS.md" exists
    When the Skills section is scanned for "forge CLI" conflation language
    Then no lines in the Skills section contain the phrase "forge CLI"
    And no lines in the Skills section contain the phrase "Forge CLI"

  Scenario: The forge:spec skill is retired and points to the engine
    Given the forge skill directory is accessible at "anvil/.claude/commands/forge"
    When the file "spec.md" is read from the forge skill directory
    Then the first H1 heading in the file is "# RETIRED — the spec phase is engine-driven"
