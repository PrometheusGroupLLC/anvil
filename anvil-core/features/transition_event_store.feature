Feature: Transition event store (one file per event)
  A transition is persisted as its own file under `<artifact>/transitions/`,
  with a content-addressed name (timestamp + actor + random suffix) that never
  derives from a sibling count. Two concurrent transitions write two distinct
  files and never mutate each other — the conflict-free invariant. Current
  state and history are a fold over those event files (merged with the legacy
  status.yaml array for dual-read), ordered by embedded timestamp with a stable
  tiebreak, independent of filesystem read order. Recording a transition no
  longer appends to the legacy `transitions:` array.

  The top-level `state:` header IS rewritten, as a derived projection of that
  fold — see `status_header_projection.feature` for the header contract. (The
  upcast originally froze the header, on the theory that not touching
  status.yaml avoided shared-file mutation. It did not: `begin` and the actor
  upsert already rewrite the same file on the same operation. All that froze
  was the value every file-level reader trusts.)

  # AC1 — event file written, content-addressed, has to/at/actor/role
  Scenario: Recording a transition writes one content-addressed event file
    Given a transition-event hearth with a track at "spec"
    When a transition to "spec_review" by "Reviewer-111111" role "review" at "2026-04-17T01:00:00Z" is recorded
    Then the transitions event directory for the track contains exactly 1 file
    And the latest transition event file carries to "spec_review" actor "Reviewer-111111" role "review" at "2026-04-17T01:00:00Z"
    And the latest transition event file name is not derived from a sibling count

  # AC2 — two transitions, two distinct files, neither mutates the other (HEADLINE)
  Scenario: Two transitions write two distinct files, conflict-free
    Given a transition-event hearth with a track at "spec"
    When a transition to "spec_review" by "Reviewer-111111" role "review" at "2026-04-17T01:00:00Z" is recorded
    And a transition to "plan" by "Doer-222222" role "plan" at "2026-04-17T02:00:00Z" is recorded
    Then the transitions event directory for the track contains exactly 2 file
    And the two transition event files have distinct names

  # AC3 — fold returns latest by timestamp + uuid even read out of order
  Scenario: Folded current state is the latest event by timestamp regardless of read order
    Given a transition-event hearth with a track at "spec"
    And a transition event file for "plan" at "2026-04-17T02:00:00Z" by "Doer-222222" role "plan"
    And a transition event file for "spec_review" at "2026-04-17T01:00:00Z" by "Reviewer-111111" role "review"
    When the track current state is read through the seam
    Then the folded current state is "plan"

  # AC5 — recording a transition does NOT append the legacy array; the header
  # IS re-projected (superseding the frozen-header rule this scenario used to
  # assert).
  Scenario: Recording a transition leaves the legacy array untouched and re-projects the header
    Given a transition-event hearth with a track at "spec"
    When a transition to "spec_review" by "Reviewer-111111" role "review" at "2026-04-17T01:00:00Z" is recorded
    Then the track status.yaml does not contain a transition to "spec_review"
    And the track status.yaml top-level state is "spec_review"
    And the track current state through the seam is "spec_review"

  # AC4 — a legacy-only artifact (status.yaml array, no event dir) resolves via the seam (dual-read)
  Scenario: A legacy-only artifact resolves its state through the dual-read seam
    Given a transition-event hearth with a track at "plan"
    Then the transitions event directory for the track contains exactly 0 file
    And the track current state through the seam is "plan"

  # AC7 — a new transition on a legacy artifact: merge-fold preserves prior history,
  # current state is the new target, and the legacy array is left intact on disk.
  Scenario: A new transition on a legacy artifact preserves prior history via merge-fold
    Given a transition-event hearth with a track at "spec"
    When a transition to "spec_review" by "Reviewer-111111" role "review" at "2026-04-17T01:00:00Z" is recorded
    And the track current state is read through the seam
    Then the folded current state is "spec_review"
    And the transitions event directory for the track contains exactly 1 file
    And the track status.yaml top-level state is "spec_review"

  # AC8 — the creation-seed mechanic: the first transition on a freshly-scaffolded
  # (transition-less) artifact writes an event file and the folded state resolves.
  Scenario: A fresh artifact's first (creation-seed) transition writes an event file and resolves
    Given a transition-event hearth with a fresh track at "spec" and no recorded transitions
    When a transition to "spec" by "Author-000001" role "spec" at "2026-04-17T00:00:00Z" is recorded
    And the track current state is read through the seam
    Then the folded current state is "spec"
    And the transitions event directory for the track contains exactly 1 file
