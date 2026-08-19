Feature: Workflow activity query groups playbooks by owner with call counts
  The ArtifactActivityQuery is the read-side that backs the UI's "which
  playbooks are registered and how often each is called, grouped by the
  kit/author that owns them" view. It enumerates every registered playbook
  from a PlaybookRegistry, derives each playbook's owner from the
  `contributed_by` field its status.yaml carries (resolved via the registry's
  workflow_id), and folds a redacted routing-activity record stream into a
  per-kind call count. Playbooks whose status.yaml lacks `contributed_by`
  (or carry no status.yaml at all) are attributed to owner "anvil". The
  result is grouped by owner so the surface can render one section per kit.

  Scenario: playbooks of differing owners are grouped by owner with their descriptions
    Given a playbook activity registry with:
      | kind                | owner       | description                  |
      | lore_query          | lore-kit    | Answer a question from lore  |
      | lore_digest         | lore-kit    | Summarize lore topics        |
      | measurement_cycle   | temper-kit  | Run a measurement cycle      |
      | track               | anvil-kit   | The dev-workflow track       |
    And no routing activity has been recorded
    When the playbook activity query is executed
    Then the playbook activity result groups owner "lore-kit" with kinds "lore_digest,lore_query"
    And the playbook activity result groups owner "temper-kit" with kinds "measurement_cycle"
    And the playbook activity result groups owner "anvil-kit" with kinds "track"
    And the playbook activity entry for kind "lore_query" has description "Answer a question from lore"
    And the playbook activity entry for kind "lore_query" has call count 0

  Scenario: a playbook lacking contributed_by is attributed to owner anvil
    Given a playbook activity registry with:
      | kind        | owner    | description           |
      | spark       |          | Capture a spark       |
      | track       | anvil-kit | The dev-workflow track |
    And no routing activity has been recorded
    When the playbook activity query is executed
    Then the playbook activity entry for kind "spark" has owner "anvil"
    And the playbook activity result groups owner "anvil" with kinds "spark"

  Scenario: call counts reflect recorded routing activity per selected kind
    Given a playbook activity registry with:
      | kind        | owner    | description           |
      | lore_query  | lore-kit | Answer a question     |
      | lore_digest | lore-kit | Summarize topics      |
      | track       | anvil-kit | The dev-workflow track |
    And routing activity has been recorded:
      | kind        | outcome | at                   |
      | lore_query  | single  | 2026-06-15T09:00:00Z |
      | lore_query  | single  | 2026-06-15T09:05:00Z |
      | lore_digest | single  | 2026-06-15T10:00:00Z |
    When the playbook activity query is executed
    Then the playbook activity entry for kind "lore_query" has call count 2
    And the playbook activity entry for kind "lore_digest" has call count 1
    And the playbook activity entry for kind "track" has call count 0

  Scenario: a recorded kind that is no longer registered is ignored
    Given a playbook activity registry with:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And routing activity has been recorded:
      | kind          | outcome | at                   |
      | lore_query    | single  | 2026-06-15T09:00:00Z |
      | retired_kind  | single  | 2026-06-15T09:05:00Z |
    When the playbook activity query is executed
    Then the playbook activity entry for kind "lore_query" has call count 1
    And the playbook activity result has no entry for kind "retired_kind"
