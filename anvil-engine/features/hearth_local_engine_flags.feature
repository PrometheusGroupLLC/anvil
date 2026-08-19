Feature: Hearth-local engine flags survive kit updates (durable local opt-ins)
  The router opt-ins (ANVIL_ENFORCE_MEASUREMENT_DEFINITION, ANVIL_SEMANTIC_ROUTE_RPC,
  ANVIL_ABSTENTION_LEDGER) are LOCAL, opt-in knobs — enabled only on a backfilled
  hearth, never baked into the published kit. Until now they lived only in the
  installed kit's cached foundry-manifest.json engine.env, so every kit auto-update
  silently reverted them. At engine startup, after process env is read, the engine
  merges OPTIONAL overrides from a hearth-local flags file at
  <global_playbooks_hearth>/engine-flags.env (simple KEY=VALUE lines, # comments).
  Precedence: real process env > the file > built-in defaults. Only ANVIL_-prefixed
  keys are honored (others ignored + warned once). A missing/unreadable file is a
  silent no-op (fail-open). This makes the opt-ins survive every kit update with zero
  watchers.

  # ── File present, keys absent from env → the ANVIL_ overrides are applied ──
  # This is the survives-kit-update case: the three router opt-ins live in the
  # hearth-local file and are merged into the engine's env at startup.
  Scenario: a flags file's ANVIL_ overrides are applied when absent from the process env
    Given a hearth-local engine-flags file with:
      """
      # router opt-ins — durable local config
      ANVIL_ENFORCE_MEASUREMENT_DEFINITION=1
      ANVIL_ENFORCE_EVIDENCE_OBLIGATION=1
      ANVIL_SEMANTIC_ROUTE_RPC=on
      ANVIL_ABSTENTION_LEDGER=on
      """
    And the process environment sets nothing
    When the engine resolves the hearth-local flags
    Then the merge applies "ANVIL_ABSTENTION_LEDGER" with value "on"
    And the merge applies "ANVIL_SEMANTIC_ROUTE_RPC" with value "on"
    And the merge applies "ANVIL_ENFORCE_MEASUREMENT_DEFINITION" with value "1"
    # T-EEC-1 P4 (F4): the new obligation dark-gate rides the same ANVIL_-prefix
    # merge rule — no allow-list edit needed, it is picked up automatically.
    And the merge applies "ANVIL_ENFORCE_EVIDENCE_OBLIGATION" with value "1"
    And the merged ANVIL_ABSTENTION_LEDGER value resolves the abstention ledger enabled

  # ── Process env overrides the file → explicit operator intent wins ──
  # A var already set in the process env is NEVER overridden by the file, so an
  # operator who explicitly set the flag off keeps it off.
  Scenario: the real process env overrides the file
    Given a hearth-local engine-flags file with:
      """
      ANVIL_ABSTENTION_LEDGER=on
      """
    And the process environment sets ANVIL_ABSTENTION_LEDGER to "off"
    When the engine resolves the hearth-local flags
    Then the merge does not apply ANVIL_ABSTENTION_LEDGER
    And the effective ANVIL_ABSTENTION_LEDGER value resolves the abstention ledger disabled

  # ── The merged flag actually drives the REAL consumer, end-to-end ──
  # Blocker-2 regression guard + real-I/O coverage: ANVIL_ABSTENTION_LEDGER is read
  # by the anvil-hooks route-turn PROCESS (which owns the ledger append), NOT the
  # engine — a set_var in the engine can never reach it, so that process merges its
  # OWN startup. This exercises the REAL install wrapper (real temp hearth + real
  # engine-flags.env + real set_var), then the REAL consumer read
  # (abstention_ledger_enabled) and a REAL ledger append against that hearth.
  #
  # HONEST LIMIT — why this is in-process rather than a spawned anvil-hooks binary:
  # the production topology is `anvil-hooks route-turn` merging the flag at its own
  # single-threaded startup (bin/anvil-hooks.rs calls the SAME
  # install_hearth_local_flags), then appending to the ledger. But that binary only
  # reaches the ledger-append branch AFTER call_engine_route returns a NoMatch
  # abstention — i.e. it requires a live engine on a port plus a real routing
  # decision. Spawning it here would drag a running engine + router into a unit-seam
  # step (slow + flaky, and hazardous on a live box). So this step drives the exact
  # same three real seams the spawned process would — install (real fs + set_var),
  # abstention_ledger_enabled (the identical consumer call), record_abstention (real
  # append) — in-process. That is the honest limit: everything the merge is
  # responsible for is covered with real I/O; only the engine round-trip that
  # PRECEDES the ledger branch (unrelated to the flag) is not re-spawned here.
  Scenario: the file's ledger opt-in drives the real abstention-ledger consumer end-to-end
    Given a hearth-local engine-flags file with:
      """
      ANVIL_ABSTENTION_LEDGER=on
      """
    Then installing the hearth-local flags enables the real abstention ledger and appends one record

  # ── Absent file → silent no-op (fail-open) ──
  # No engine-flags.env in the hearth is not an error; nothing is applied.
  Scenario: an absent flags file is a silent no-op
    Given a hearth with no engine-flags file
    And the process environment sets nothing
    When the engine resolves the hearth-local flags
    Then the merge applies no keys

  # ── Non-ANVIL_ keys are ignored (and surfaced for the warn-once) ──
  # Only ANVIL_-prefixed keys are honored; any attempt to smuggle PATH/HOME/etc.
  # through the file is ignored, never applied.
  Scenario: non-ANVIL_ keys in the file are ignored
    Given a hearth-local engine-flags file with:
      """
      ANVIL_ABSTENTION_LEDGER=on
      PATH=/somewhere/evil
      HOME=/tmp/hijack
      """
    And the process environment sets nothing
    When the engine resolves the hearth-local flags
    Then the merge applies "ANVIL_ABSTENTION_LEDGER" with value "on"
    And the merge applies exactly 1 key
    And the merge ignores the non-ANVIL_ key "PATH"
    And the merge ignores the non-ANVIL_ key "HOME"

  # T-EEC-2 P4: claimed-evidence enforcement is a distinct, per-request-lane
  # dark gate. Only the explicit affirmative values enable it; the T-EEC-1
  # authoring flag cannot do so, and startup must not globalize the lane key.
  Scenario Outline: claimed-evidence gate values are dark by default
    Then claimed-evidence gate value "<value>" resolves "<outcome>"

    Examples:
      | value   | outcome  |
      | unset   | disabled |
      | empty   | disabled |
      | 0       | disabled |
      | garbage | disabled |
      | 1       | enabled  |
      | true    | enabled  |
      | on      | enabled  |

  Scenario: the authoring obligation flag cannot enable the claimed-evidence gate
    Given two request lanes where only the first opts into claimed-evidence enforcement
    And the process environment enables only the authoring evidence-obligation flag
    Then claimed-evidence enforcement is enabled only for the opted-in request lane
    And startup installation does not globalize claimed-evidence enforcement

  Scenario: one engine keeps claimed-evidence enforcement isolated by request hearth
    Given two hearth directories X and Y each with the standard structure
    And request lane X opts into claimed-evidence enforcement while lane Y remains default off
    When the same unsatisfied evidence transition is sent to both request lanes
    Then request lane X is refused before mutation with failed precondition "playbook_evidence_obligation_unsatisfied"
    And request lane Y succeeds and records evidence status "absent"
