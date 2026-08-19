Feature: The abstention ledger durably records conversation-aware abstentions (opt-in)
  When the route-turn hook's FINAL outcome for a user turn is an abstention
  (NoMatch — no playbook fit) AND the opt-in flag ANVIL_ABSTENTION_LEDGER is on,
  one JSON line is appended to <hearth>/abstentions/ledger.jsonl. The record is
  conversation-aware (message + recent_context + conversation_id + candidate_set)
  because routing decides on the conversation, not the bare message — later
  clustering counts DISTINCT conversations per theme. Default OFF is the privacy
  default and the byte-for-byte parity case. Only abstentions are recorded; a
  routed turn writes nothing. The append is append-only and FAIL-OPEN — the
  advisory hook must never fail a turn.

  # ── Flag OFF → nothing written (privacy default + parity) ──
  # With the ledger not opted into, an abstaining turn writes NO ledger file.
  Scenario: flag OFF writes no record for an abstaining turn
    Given a throwaway hearth for the abstention ledger
    And the abstention ledger flag is off
    When the route-turn final outcome is an abstention on message "some vague musing" in conversation "conv-off" with context "prior chit-chat" and candidates "daily_recap,weekly_recap"
    Then the abstention ledger file does not exist

  # ── Flag ON + abstention → exactly one conversation-aware record ──
  Scenario: flag ON records one conversation-aware abstention
    Given a throwaway hearth for the abstention ledger
    And the abstention ledger flag is on
    When the route-turn final outcome is an abstention on message "map our onboarding funnel" in conversation "conv-42" with context "user: we keep losing signups\nassistant: where in the flow" and candidates "daily_recap,weekly_recap"
    Then the abstention ledger record count is 1
    And the abstention ledger record 1 has message "map our onboarding funnel"
    And the abstention ledger record 1 has conversation_id "conv-42"
    And the abstention ledger record 1 has recent_context "user: we keep losing signups\nassistant: where in the flow"
    And the abstention ledger record 1 has candidate_set "daily_recap,weekly_recap"

  # ── Flag ON + a ROUTED turn → nothing written (only abstentions) ──
  Scenario: flag ON writes no record when the turn routes to a kind
    Given a throwaway hearth for the abstention ledger
    And the abstention ledger flag is on
    When the route-turn final outcome routes to kind "daily_recap" on message "daily recap please" in conversation "conv-routed"
    Then the abstention ledger file does not exist

  # ── Conversation-awareness: two abstentions in one conversation → two records ──
  # Proves the conversation_id rides on every record (no dedup — clustering is Phase 2).
  Scenario: two abstentions in the same conversation append two records sharing the id
    Given a throwaway hearth for the abstention ledger
    And the abstention ledger flag is on
    When the route-turn final outcome is an abstention on message "first wall we hit" in conversation "conv-77" with context "turn one" and candidates "daily_recap"
    And the route-turn final outcome is an abstention on message "still stuck here" in conversation "conv-77" with context "turn two" and candidates "daily_recap"
    Then the abstention ledger record count is 2
    And every abstention ledger record has conversation_id "conv-77"

  # ── Line-atomicity: every appended line is a standalone-parseable record ──
  # The ledger is line-delimited JSON, so each append must land as ONE physical
  # line carrying exactly ONE record — otherwise a downstream reader can't split
  # it back into records.
  Scenario: a written abstention line parses standalone
    Given a throwaway hearth for the abstention ledger
    And the abstention ledger flag is on
    When the route-turn final outcome is an abstention on message "solo line" in conversation "c1" with context "ctx" and candidates "daily_recap"
    Then the abstention ledger physical line count is 1
    And every abstention ledger line parses as exactly one record

  # ── Line-atomicity under concurrency (task #95) ──
  # Many `anvil-hooks route-turn` processes append to the SAME ledger at once.
  # Each append must be a SINGLE atomic write on the O_APPEND handle, so no two
  # records can glue onto one line and no record can be torn. With a
  # non-atomic record-then-newline pair of writes, a concurrent appender lands
  # BETWEEN them, gluing two JSON objects onto one physical line (observed 3/14
  # lines in the field). Every physical line must parse as exactly one record.
  Scenario: concurrent abstention appends each land as a standalone-parseable line
    Given a throwaway hearth for the abstention ledger
    When 8 concurrent abstaining turns each append 500 records to the ledger
    Then the abstention ledger physical line count is 4000
    And every abstention ledger line parses as exactly one record
    And the abstention ledger record count is 4000

  # ── The pure flag-resolution (ledger_flag_on) — on/off unit contract ──
  Scenario: the flag value "on" resolves enabled
    When the abstention ledger flag value is "on"
    Then the abstention ledger flag resolves enabled

  Scenario: the flag value "1" resolves enabled
    When the abstention ledger flag value is "1"
    Then the abstention ledger flag resolves enabled

  Scenario: the flag value "true" resolves enabled
    When the abstention ledger flag value is "true"
    Then the abstention ledger flag resolves enabled

  Scenario: the flag value "off" resolves disabled
    When the abstention ledger flag value is "off"
    Then the abstention ledger flag resolves disabled

  Scenario: an unset flag resolves disabled (the privacy default)
    When the abstention ledger flag is unset
    Then the abstention ledger flag resolves disabled

  # The ledger answers ONE question: what work do people want that no playbook
  # covers? A harness block is a system event, not a person wanting something.
  #
  # Measured on the live fleet before this filter: 1,217 of 3,206 rows (38.0%)
  # were `<task-notification>` / `<system-reminder>` blocks. Anyone mining the
  # ledger for new-playbook demand — which is what it is FOR — was mining a
  # corpus that was over a third machine chatter.
  #
  # This does not change routing. Every message still routes; only what counts
  # as EVIDENCE OF DEMAND is narrowed.
  Scenario Outline: harness-generated turns are not unmet demand
    When the abstention ledger considers the message "<message>"
    Then the abstention is recorded is "<recorded>"

    Examples:
      | message                                          | recorded |
      | can you add a deploy checklist playbook          | yes      |
      | <task-notification> task-id abc completed        | no       |
      | <system-reminder> the task tools were not used   | no       |
      | [SYSTEM NOTIFICATION - NOT USER INPUT] background | no      |
      | why is the router abstaining so much             | yes      |
