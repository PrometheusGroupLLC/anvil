Feature: ArtifactPort persist_generated_playbook writes to a given owner-home (track 1a, BP1, A1)
  The narrow write op persists an already-validated playbook machine into an
  explicit owner-home — `<owner_home>/playbooks/<kind>/machine.yaml` — writing NO
  status.yaml and ignoring the adapter's own hearth_path. The op is the
  authoritative write-boundary collision gate: it creates the target file
  exclusively, and if the target already exists it compares the existing bytes to
  the bytes this persist would write. Identical bytes are an idempotent no-op;
  different or invalid existing bytes fail closed without overwriting.

  Scenario: fs adapter writes machine.yaml under a given owner-home
    Given a temp directory used as an owner-home
    When the fs artifact adapter persists a playbook for kind "throwaway_kind" under that owner-home
    Then a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the owner-home
    And that machine.yaml contains "kind: throwaway_kind"
    And no status.yaml exists alongside the persisted machine.yaml
    And no exemplars directory exists alongside the persisted machine.yaml

  Scenario: fs adapter writes exemplar markdown beside the generated machine
    Given a temp directory used as an owner-home
    When the fs artifact adapter persists a playbook for kind "throwaway_kind" with exemplar "triage-good" under that owner-home
    Then a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under the owner-home
    And an exemplar markdown exists at "playbooks/throwaway_kind/exemplars/triage-good.md" under the owner-home
    And that exemplar markdown contains "band: good"
    And that exemplar markdown contains "A distilled good triage pattern."

  Scenario: fs adapter ignores its own hearth_path
    Given a fs artifact adapter constructed with a hearth_path of temp dir A
    And a separate temp dir B used as an owner-home
    When the fs artifact adapter persists a playbook for kind "throwaway_kind" under owner-home B
    Then a machine.yaml exists at "playbooks/throwaway_kind/machine.yaml" under owner-home B
    And owner-home A has no playbooks directory

  Scenario: fs adapter refuses to overwrite different content already at the write boundary
    Given a temp directory used as an owner-home
    And that owner-home already has different machine.yaml content for kind "throwaway_kind"
    When the fs artifact adapter attempts to persist a playbook for kind "throwaway_kind" under that owner-home
    Then the fs artifact adapter returns an error containing "playbook_duplicate_kind_registration"
    And the existing machine.yaml bytes for kind "throwaway_kind" are unchanged

  Scenario: fs adapter reports a byte-different hook-bearing duplicate as a duplicate, not existing-invalid
    Given a temp directory used as an owner-home
    And that owner-home already has byte-different hook-bearing machine.yaml content for kind "throwaway_kind"
    When the fs artifact adapter attempts to persist a playbook for kind "throwaway_kind" under that owner-home
    Then the fs artifact adapter returns an error containing "playbook_duplicate_kind_registration"
    And the existing machine.yaml bytes for kind "throwaway_kind" are unchanged

  Scenario: fs adapter treats identical content already at the write boundary as idempotent success
    Given a temp directory used as an owner-home
    And that owner-home already has identical machine.yaml content for kind "throwaway_kind"
    When the fs artifact adapter attempts to persist a playbook for kind "throwaway_kind" under that owner-home
    Then the fs artifact adapter succeeds
    And the existing machine.yaml bytes for kind "throwaway_kind" are unchanged

  Scenario: in-memory adapter records the persist call
    Given an in-memory artifact adapter
    When the in-memory adapter persists a playbook for owner-home "X" and kind "throwaway_kind"
    Then the in-memory adapter recorded 1 persisted playbook
    And the recorded persisted playbook has owner-home "X" and kind "throwaway_kind"
