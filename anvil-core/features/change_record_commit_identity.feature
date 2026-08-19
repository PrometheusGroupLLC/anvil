Feature: Change record commit metadata and identity

  What the engine ADDS to a commit — author, committer, subject and trailers —
  is governed by the no-raw-identities contract. The tree is the transaction's
  own content and is governed separately.

  Every assertion below is made over the RENDERED commit, read back out of the
  object store, never over an in-memory struct: an absence assertion over a
  Rust type is a compile-time no-op, and the publication-log envelope (raw
  actor name, raw approver, raw conversation id) is one `impl` away from being
  inherited here.

  The seam is anvil-core and the user is the engine.

  Scenario: The commit author and committer are the fixed engine identity, not the ambient git config
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the hearth repository git config sets user.name "Hearth Owner" and user.email "owner@example.invalid"
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged,ArtifactWritten |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit author and committer are "anvil" and "anvil@localhost"
    And the commit metadata does not contain "Hearth Owner"
    And the commit metadata does not contain "owner@example.invalid"
    And HEAD, the index and the working tree are unchanged

  Scenario: The commit carries the salted actor hash and never the raw actor name
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged                 |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit has a trailer named "Anvil-Actor-Hash"
    And the commit trailer "Anvil-Actor-Hash" is the salted hash of "fable-the-orchestrator"
    And the commit metadata does not contain "fable-the-orchestrator"

  Scenario: The commit carries the salted conversation hash and never the raw conversation id
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | begin                        |
      | artifact_kind   | track                        |
      | event_kinds     | ArtifactCreated              |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit has a trailer named "Anvil-Conversation-Hash"
    And the commit trailer "Anvil-Conversation-Hash" is the salted hash of "6f1c2a48-0b3d-4f9a-9a11-77bd"
    And the commit metadata does not contain "6f1c2a48-0b3d-4f9a-9a11-77bd"

  Scenario: With no telemetry salt configured the actor hash is absent rather than raw
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record hearth has no telemetry salt
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged                 |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit has no trailer named "Anvil-Actor-Hash"
    And the commit has no trailer named "Anvil-Conversation-Hash"
    And the commit metadata does not contain "fable-the-orchestrator"
    And the commit metadata does not contain "6f1c2a48-0b3d-4f9a-9a11-77bd"

  Scenario: The commit message and trailers contain no absolute path, no approver name and no prose
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged                 |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    And the transaction carries reflection notes "the spec was too vague so I rewrote it" and approver "Nick"
    When the change-record commit is written
    Then the commit metadata does not contain "the spec was too vague so I rewrote it"
    And the commit metadata does not contain "Nick"
    And the commit metadata contains no absolute path

  Scenario: The trailer set is exactly the declared allowlist
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged                 |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | project_label   | anvil-hearth                 |
      | playbook_run_id | t11-engine-writes            |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit trailer key set is exactly the declared allowlist

  Scenario: The commit records the RPC command and the routed event variant names
    Given a hearth directory with the following structure:
      | path            | state |
      | tracks/t-record | spec  |
    And the change-record telemetry salt is "deployment-salt"
    And a change-record transaction with:
      | key             | value                        |
      | command         | complete                     |
      | artifact_kind   | track                        |
      | event_kinds     | StateChanged,ArtifactWritten |
      | actor           | fable-the-orchestrator       |
      | conversation_id | 6f1c2a48-0b3d-4f9a-9a11-77bd |
      | path            | tracks/t-record/status.yaml  |
    When the change-record commit is written
    Then the commit subject is "anvil: complete track"
    And the commit trailer "Anvil-Command" is "complete"
    And the commit trailer "Anvil-Artifact-Kind" is "track"
    And the commit trailer "Anvil-Event-Kinds" is "StateChanged,ArtifactWritten"
    And the commit trailer "Anvil-Paths-Recorded" is "1"
    And the commit is dated the declared at
