Feature: Playbook generation authoring artifacts
  playbook_generation authoring is auditable through playbook-shaped files and
  append-only amendments rather than a track spec.

  Scenario: playbook_generation creation scaffolds authoring artifacts
    Given a builder hearth with parent track "20260419T1336_parent"
    When begin is called for playbook_generation "daily_standup" target_owner "Foundation"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with kind "playbook_generation" state "gathering" directory "workflow_generations" registry "workflow_generations.md"
    And the ArtifactCreation scaffold files include "research.md,proposal.md,plan.md,draft.md,amendments.md,machine.yaml,reflection.md,authoring.md"

  Scenario: playbook amendments are accepted for append-only authoring decisions
    Given an in-memory amend query adapter with artifact "workflow_generations/wg1" kind "playbook_generation" state "gathering"
    When amend Add is called for artifact "workflow_generations/wg1" kind "playbook" document "amendments" target_id "decision-1" new_kind "authoring_decision" body "Require a review gate before publishing" at "2026-06-04T21:15:00Z"
    Then the amend outcome is successful
    And the amend outcome event 0 is OpRecorded
    And the amend outcome OpRecorded target_document is "amendments"
