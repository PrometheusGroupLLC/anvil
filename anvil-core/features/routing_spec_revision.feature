Feature: Routing — spec_revision is engine-supported for doer and reviewer
  Slice B (R4): the execution_route discriminator returns "engine" for the
  spec_revision routing entries — the doer re-entry (filtered_artifact, creator),
  the engine-handled reviewer rejection (filtered_artifact, reviewer), and the
  doer's complete action (available_action, spec). The resumer-fallback row for
  spec_revision is unchanged (negative-space assertion).

  Scenario: filtered_artifact for (track, spec_revision, creator) is engine
    When compute_execution_route is called for filtered_artifact kind "track" state "spec_revision" role "creator"
    Then the supported playbook result is "engine"

  Scenario: filtered_artifact for (track, spec_revision, reviewer) is engine
    When compute_execution_route is called for filtered_artifact kind "track" state "spec_revision" role "reviewer"
    Then the supported playbook result is "engine"

  Scenario: available_action for (track, spec_revision, spec) is engine
    When compute_execution_route is called for available_action kind "track" state "spec_revision" role "spec"
    Then the supported playbook result is "engine"

  # spec_revision is a doer-resume state, so resumer was already engine-routed
  # before Slice B; Slice B does not regress it.
  Scenario: resumer routing for spec_revision is unchanged (no regression)
    When compute_execution_route is called for filtered_artifact kind "track" state "spec_revision" role "resumer"
    Then the supported playbook result is "engine"
