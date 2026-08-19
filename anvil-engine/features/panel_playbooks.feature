Feature: GET /panel/playbooks — the Playbooks column the host pulls

  The host draws the Playbooks column and this engine is the only thing that
  knows what playbooks exist. The kit HAS published it since 2026-08-15 —
  `kit/app/frontend/src/lib/panelPublish.ts`, with a real state derivation, and
  production carries 32 rows of it — but it publishes from the FRONTEND through a
  Tauri invoke, so the only thing that can fire it is a person opening anvil's
  surface. That is the same defect kiln's column had, and the same one that left
  temper's Measures column an absent key while its store held 161 evals.

  Foundry pulls a kit's panel at kit start from a route the kit declares. An
  engine route is reachable without a person; a frontend publisher is not.

  I ALREADY GOT THIS WRONG ONCE, and it is why this is a careful PORT rather than
  a fresh idea. On 2026-08-16 I shipped a `/panel/playbooks` route whose state was
  the literal "now" for every row and whose title was the machine's entire
  description — one of them about a thousand characters — and reverted it
  (9fd803e2), because the host pull would have written mine OVER the better rows
  already in production. Every rule here is transcribed from `panelPublish.ts`
  and `registry.ts`; where they disagree with this route, THEY are right.

  THE THREE READS ARE THE FRONTEND'S THREE READS, through the same `compute_*`
  functions the gRPC RPCs and the `/ws` methods use, so this surface cannot drift
  from the pane beside it: the atlas says whether a definition loads, the live
  instances say whether a run is going right now, the activity says whether it
  has ever been called.

  Scenario: The route answers the document shape the host reads
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner     | description       |
      | track_review | anvil-kit | Review a track    |
      | lore_query   | lore-kit  | Answer a question |
    And the engine is started with that hearth
    When an HTTP GET /panel/playbooks is sent to the engine port
    Then the /panel/playbooks response status is 200
    And the playbooks document declares version 1
    And the playbooks document carries 2 playbooks

  # A playbook the engine has never recorded a call for. The design's own words
  # (`j2_expert.py:444`) and the reason it is not a zero.
  Scenario: A playbook nothing has ever called is hollow
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner     | description    |
      | track_review | anvil-kit | Review a track |
    And the engine is started with that hearth
    When an HTTP GET /panel/playbooks is sent to the engine port
    Then the playbook "Track review" is in state "hollow"
    And the playbook "Track review" carries the meta "nothing has run"

  # The customer's own grouping. A playbook this kit owns is theirs; one that
  # arrived from another kit is shared with them, and the column says which.
  #
  # THE GROUPING IS DERIVED FROM status.yaml's `owner_kit:` — NOT from the
  # fixture's `owner` column, which writes `contributed_by:` and feeds the
  # activity roll-up instead. My first draft asserted "Yours" against a fixture
  # that never wrote owner_kit, so every row came back "Shared with you": the
  # scenario was asking for something the fixture could not produce, and the
  # failure was mine rather than the route's.
  Scenario: A playbook from another kit is grouped as shared
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner     | description       |
      | track_review | anvil-kit | Review a track    |
      | lore_query   | lore-kit  | Answer a question |
    And the playbook "track_review" declares owner_kit "anvil-kit"
    And the playbook "lore_query" declares owner_kit "lore-kit"
    And the engine is started with that hearth
    When an HTTP GET /panel/playbooks is sent to the engine port
    Then the playbook "Track review" is grouped under "Yours"
    And the playbook "Lore query" is grouped under "Shared with you"

  # THE TITLE IS A NAME, NEVER AN IDENTIFIER, and never a description. The route
  # I reverted put the machine's whole description in this slot — about a
  # thousand characters for one row — so this asserts the re-spelling rule
  # rather than trusting it: `track_review` becomes `Track review`.
  Scenario: A title is the kind re-spelled, not the identifier and not the description
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner     | description                                    |
      | track_review | anvil-kit | A very long description that is not the title |
    And the engine is started with that hearth
    When an HTTP GET /panel/playbooks is sent to the engine port
    Then the playbook "Track review" is in state "hollow"
    And no playbook title contains an underscore
    And no playbook title is longer than 80 characters

  # A call was recorded and the engine records THAT it ran, never whether the run
  # was clean. Printing "clean" from a call count would be an invented outcome,
  # so the row says plainly that no outcome was recorded.
  Scenario: A playbook that has run carries no invented outcome
    Given a playbook activity engine hearth with playbooks:
      | kind         | owner     | description    |
      | track_review | anvil-kit | Review a track |
    And the engine is started with that hearth
    When the route RPC is called with message "track_review", signal "", conversation_id "panel-turn-1", ctx org "Foundation" role "read" clearance "internal"
    And an HTTP GET /panel/playbooks is sent to the engine port
    Then the playbook "Track review" is in state "this engine records that it ran, not how it went"
    And the playbook "Track review" carries no meta

  # ALL HEARTHS, and this is the scenario the shipped route needed and did not
  # have. `PlaybookRegistryView.tsx` reads live instances scoped to ONE hearth
  # and activity across EVERY hearth. The asymmetry is load-bearing: a call
  # recorded against another hearth still means this playbook HAS run, and
  # scoping it away turns "ran, outcome unknown" into "never run".
  #
  # MEASURED, not imagined: with activity scoped to one hearth, 5 of anvil's 32
  # production rows derived `hollow` that the shipped frontend derives as having
  # run. The grouping matched the frontend exactly (Yours 7 / Shared 25) while
  # the states did not, which is what made the narrower read visible at all.
  Scenario: A call recorded in another hearth still means the playbook has run
    Given a permitted parent root with two sub-hearths each seeded with playbook "shared_kind" owned by "lore-kit"
    And an activity log record for kind "shared_kind" is appended to the SECOND sub-hearth
    And the engine is started with the first sub-hearth and the parent as a permitted root
    When an HTTP GET /panel/playbooks is sent to the engine port
    Then the playbook "Shared kind" is in state "this engine records that it ran, not how it went"
    And the playbook "Shared kind" carries no meta
