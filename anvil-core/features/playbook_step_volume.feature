Feature: WorkflowStepVolume folds step measurements into per-step counts
  The WorkflowStepVolume read-side folds the durable, redacted step-measurement
  record stream, filtered to a single workflow kind, into per-step call counts
  grouped by (from_state, to_state, role). Steps are ordered by descending
  call_count, then ascending from_state. A record for a different kind is
  excluded. An unknown kind, or an empty record stream, folds to an empty step
  list (no error) — a fresh hearth simply has no step measurements yet.

  Scenario: step measurements group by from/to/role with counts
    Given a playbook step volume record stream:
      | kind       | from_state | to_state    | role     | at                   |
      | lore_query | active     | in_progress | doer     | 2026-06-15T09:00:00Z |
      | lore_query | active     | in_progress | doer     | 2026-06-15T10:00:00Z |
      | lore_query | in_progress| completed   | reviewer | 2026-06-15T11:00:00Z |
    When the playbook step volume is folded for kind "lore_query"
    Then the playbook step volume has 2 steps
    And the playbook step volume step from "active" to "in_progress" role "doer" has call count 2
    And the playbook step volume step from "in_progress" to "completed" role "reviewer" has call count 1
    And the playbook step volume steps are ordered by descending call count

  Scenario: only records matching the requested kind are folded
    Given a playbook step volume record stream:
      | kind        | from_state | to_state    | role | at                   |
      | lore_query  | active     | in_progress | doer | 2026-06-15T09:00:00Z |
      | measurement | active     | in_progress | doer | 2026-06-15T10:00:00Z |
    When the playbook step volume is folded for kind "lore_query"
    Then the playbook step volume has 1 steps
    And the playbook step volume step from "active" to "in_progress" role "doer" has call count 1

  Scenario: an unknown kind folds to no steps
    Given a playbook step volume record stream:
      | kind       | from_state | to_state    | role | at                   |
      | lore_query | active     | in_progress | doer | 2026-06-15T09:00:00Z |
    When the playbook step volume is folded for kind "no_such_kind"
    Then the playbook step volume has 0 steps

  Scenario: an empty record stream folds to no steps
    Given an empty playbook step volume record stream
    When the playbook step volume is folded for kind "lore_query"
    Then the playbook step volume has 0 steps

  Scenario: the fold filters on the real artifact_kind, not the redaction kind
    Given a playbook step volume record stream:
      | kind             | artifact_kind | from_state | to_state    | role | at                   |
      | step_measurement | lore_query    | active     | in_progress | doer | 2026-06-15T09:00:00Z |
      | step_measurement | track         | active     | in_progress | doer | 2026-06-15T10:00:00Z |
    When the playbook step volume is folded for kind "lore_query"
    Then the playbook step volume has 1 steps
    And the playbook step volume step from "active" to "in_progress" role "doer" has call count 1

  Scenario: legacy records lacking artifact_kind fall back to the kind field
    Given a playbook step volume record stream:
      | kind       | artifact_kind | from_state | to_state    | role | at                   |
      | lore_query |               | active     | in_progress | doer | 2026-06-15T09:00:00Z |
    When the playbook step volume is folded for kind "lore_query"
    Then the playbook step volume has 1 steps
    And the playbook step volume step from "active" to "in_progress" role "doer" has call count 1

  Scenario: cross-hearth merge sums per-step counts across hearths
    Given hearth A routing+actor stream:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T09:00:00Z | aaaa1111   |
    And hearth B routing+actor stream:
      | kind       | outcome | at                   | actor_hash |
      | lore_query | single  | 2026-06-15T10:00:00Z | bbbb2222   |
    When the playbook step volume is folded across both hearths for kind "lore_query"
    Then the playbook step volume has 1 steps
    And the playbook step volume step from "" to "in_progress" role "doer" has call count 2
