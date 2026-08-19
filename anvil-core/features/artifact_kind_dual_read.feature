Feature: durable sinks fold `workflow_kind` and `artifact_kind` into one model

  NG-CORRELATION-KEY-DECISION-AMENDMENT, answered by Nick on 2026-07-30
  (decision 6, "Two fields are simply named wrong" -> "yes fix them"), narrowed
  to this one rename: the durable-sink field naming the governed artifact kind
  is canonically `artifact_kind`. Its values are governed artifact kinds
  (`track`, `lore_query`, ...), never definition kinds, so `workflow_kind` was
  wrong about its own contents.

  Existing rows are never rewritten and no sink file is ever renamed, so both
  spellings live on disk permanently. Every reader must therefore fold them
  into ONE internal model. A row carrying BOTH spellings is the dual-emission
  shape the amendment forbids at the log seam: a reader must refuse it rather
  than silently pick whichever it happened to check first.

  Driven against the REAL FileSystem*Adapter read paths over real byte-written
  JSONL rows in a real TempDir hearth. All five durable sinks that carry the
  field are exercised together, so the check cannot pass by closing four of five.

  Scenario: a legacy row and a canonical row read as the same kind, on every sink
    Given a durable sink hearth seeded with one legacy row and one canonical row per sink
    When every durable sink is read through its real adapter
    Then every sink reports the same governed kind for both rows

  Scenario: a row carrying both spellings is refused, on every sink
    Given a durable sink hearth seeded with one dual-spelling row per sink
    When every durable sink is read through its real adapter
    Then every sink refuses the row as malformed and names both field spellings

  # The adapters refuse an ambiguous row before they ever ask it for a value,
  # so the fold's own answer for that case is unobservable through them. It is
  # asserted directly here: an ambiguous row must yield NO value, so a future
  # caller that forgets the refusal gets nothing rather than a coin flip.
  Scenario: the fold itself answers all four shapes, and yields nothing when ambiguous
    When the artifact-kind fold is applied to the canonical, legacy, absent and dual-spelling shapes
    Then the fold reports canonical, legacy, missing and ambiguous, and the ambiguous shape carries no value

  Scenario: a row carrying neither spelling still reads as the empty kind
    Given a durable sink hearth seeded with one row carrying neither spelling per sink
    When every durable sink is read through its real adapter
    Then every sink reports the empty governed kind and no error
