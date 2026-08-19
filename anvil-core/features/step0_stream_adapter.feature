Feature: The full §0 step-measurement stream adapter is append-only, partitioned, and idempotent
  The FileSystemStep0StreamAdapter writes the temper-consumed §0 stream to
  `<temper_home>/.temper/step-measurements/<kind>/events.jsonl` — one JSON object
  per line, append-only, partitioned by artifact_kind. A line whose stable
  event_id already exists is a no-op (idempotent replay). Two distinct logical
  steps in the same second are NOT false-deduped. The redaction policy omits the
  tokens field even when usage is supplied.

  Scenario: a replayed (identical) event is deduped to a single line
    Given a temper home for the §0 stream
    Then appending the same §0 event twice for kind "track" writes one line

  Scenario: two distinct steps in the same second are not false-deduped
    Given a temper home for the §0 stream
    Then appending two distinct §0 events in the same second for kind "track" writes two lines

  # H3: two GENUINELY-DISTINCT transitions that collapse on
  # (workflow_id, from_state, to_state, role, at) — e.g. a loop re-entering the
  # same transition in the same second — must NOT false-dedupe. The uniqueness
  # component (event_seq) keeps them distinct. A role-only seq would collapse
  # them to one line (the bug).
  Scenario: two distinct same-second same-role same-transition events with distinct seq are both kept
    Given a temper home for the §0 stream
    Then appending two §0 events identical except seq for kind "track" writes two lines

  Scenario: a None-tokens event omits the tokens key from the line
    Given a temper home for the §0 stream
    Then appending a §0 event with no tokens for kind "track" omits the tokens field

  Scenario: events are partitioned per playbook kind
    Given a temper home for the §0 stream
    Then the §0 stream partitions kind "lore_query" and kind "track" into separate files
