Feature: InMemoryArtifactAdapter scaffolding
  InMemoryArtifactAdapter implements ArtifactPort for testing.
  It records created docs in a vector for assertion and supports
  idempotency simulation via pre-existing doc seeds.

  Scenario: create_review_doc records the creation
    Given an in-memory artifact adapter with no pre-existing docs
    When artifact_port.create_review_doc is called for track "tracks/my-track" doc "spec.review.md" header "# Review"
    Then the in-memory artifact adapter recorded 1 created doc
    And the in-memory artifact adapter's created doc has track "tracks/my-track" and doc "spec.review.md"

  Scenario: create_review_doc returns the doc path
    Given an in-memory artifact adapter with no pre-existing docs
    When artifact_port.create_review_doc is called for track "tracks/my-track" doc "spec.review.md" header "# Review"
    Then the artifact port returns a path containing "my-track"
    And the artifact port returns a path containing "spec.review.md"

  Scenario: create_review_doc is idempotent for pre-existing docs
    Given an in-memory artifact adapter with pre-existing doc at track "tracks/my-track" doc "spec.review.md"
    When artifact_port.create_review_doc is called for track "tracks/my-track" doc "spec.review.md" header "# Review"
    Then the in-memory artifact adapter recorded 0 created docs
    And the artifact port returns a path containing "spec.review.md"

  Scenario: scaffold_track_directory records the call
    Given an in-memory artifact adapter with no pre-existing docs
    When artifact_port.scaffold_track_directory is called with track_name "my-new-track" parent_id "20260411T2021_anvil_workflow_engine" display_name "My New Track" and status_yaml "version: 1\nstate: spec\n"
    Then the in-memory artifact adapter recorded 1 scaffolded track
    And the scaffolded track record has track_name "my-new-track" and parent_id "20260411T2021_anvil_workflow_engine" and display_name "My New Track"

  Scenario: scaffold_track_directory returns a path containing the track_name
    Given an in-memory artifact adapter with no pre-existing docs
    When artifact_port.scaffold_track_directory is called with track_name "my-new-track" parent_id "20260411T2021_anvil_workflow_engine" display_name "My New Track" and status_yaml "version: 1\nstate: spec\n"
    Then the artifact port returns a path containing "my-new-track"

  Scenario: scaffold_track_directory returns an error if the track already exists
    Given an in-memory artifact adapter with pre-existing scaffold for track_name "duplicate-track"
    When artifact_port.scaffold_track_directory is called with track_name "duplicate-track" parent_id "20260411T2021_anvil_workflow_engine" display_name "Duplicate Track" and status_yaml "version: 1\nstate: spec\n"
    Then the artifact port returns an IoError with message starting "track directory already exists"
