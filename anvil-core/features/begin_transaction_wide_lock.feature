Feature: A begin-style transaction holds one hearth guard across read and writes
  begin MUST acquire its hearth lock transaction-wide — before its initial read
  — and hold it across all event writes, so a multi-event begin cannot lose an
  update (spec Req 5 / F3, AC "begin transaction-wide lock"). At the anvil-core
  seam this is the guarantee that a single HearthLocks guard, acquired before
  the read, is held across multiple subsequent writes; an interleaving
  same-hearth writer cannot slip between the read and the writes.

  Scenario: One guard held across read + multiple writes blocks an interleaving writer
    Given a HearthLocks primitive
    When one writer holds a single hearth guard across a read and multiple writes while another same-hearth writer races
    Then the final committed value is 3
