Feature: Begin create drives a parent-less domain machine
  begin(artifact_type: "knowledge_lifecycle", no parent) resolves the machine
  from the registry and emits a machine-derived ArtifactCreation event:
  kind/initial-state/directory/registry all sourced from the resolved
  PlaybookMachine, with NO parent required. The track create flow stays
  registry-driven too (behavior-preservation oracle). (S1 / AC1, pure handler.)

  Scenario: knowledge_lifecycle create emits an ArtifactCreation with machine-derived placement
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine
    When begin fs composite create is executed for artifact_type "knowledge_lifecycle" with no parent
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with kind "knowledge_lifecycle" state "ingesting" directory "knowledge" registry "knowledge.md"
    And the begin outcome result state is "ingesting"

  Scenario: an unknown kind with no machine is rejected as unsupported
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine
    When begin fs composite create is executed for artifact_type "bogus_kind" with no parent
    Then the begin outcome is an UnsupportedType error for "bogus_kind"

  Scenario: track create still requires a parent and emits kind track (behavior-preservation)
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine and an active proposal "20260601T0000_parent"
    When begin fs composite create is executed for artifact_type "track" with parent "20260601T0000_parent"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with kind "track" state "spec" directory "tracks" registry "tracks.md"

  Scenario: a create begin carrying a conversation_id records it on the ArtifactCreation event (resume-aware routing)
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine and an active proposal "20260601T0000_parent"
    When begin fs composite create is executed for artifact_type "track" with parent "20260601T0000_parent" and conversation_id "C1"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with conversation_id "C1"

  Scenario: a create begin without a conversation_id records an empty conversation_id (back-compat)
    Given a composite begin fs hearth seeded with the knowledge_lifecycle machine and an active proposal "20260601T0000_parent"
    When begin fs composite create is executed for artifact_type "track" with parent "20260601T0000_parent"
    Then the begin outcome is successful
    And the begin outcome emits an ArtifactCreation event with conversation_id ""
