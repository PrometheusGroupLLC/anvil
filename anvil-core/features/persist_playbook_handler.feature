Feature: PersistPlaybookCommandHandler validates before emitting a persist event (track 1a, BP2, A2/A5)
  The pure CQRS handler loader-validates the generated machine FIRST, then checks
  for a duplicate kind at the owner-home via the passed-in registry. If the
  existing machine is byte-identical to this generated machine, the handler
  treats the request as already persisted and succeeds without emitting a write
  event. Same-kind content that is valid but byte-different returns a duplicate
  error and emits NO event. This keeps terminal playbook-generation retries safe
  across the two side effects: persist first, then local completion writes, while
  matching the adapter's byte-exact write-boundary policy.

  Scenario: valid machine yields one persist event
    Given a registry built from an empty temp owner-home
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns one PlaybookPersisted event
    And the PlaybookPersisted event carries kind "throwaway_kind"

  Scenario: loader-invalid machine yields a typed error and no event
    Given a registry built from an empty temp owner-home
    And a loader-invalid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a LoaderInvalid error
    And the persist handler error code is "playbook_unknown_role_reference"
    And the persist handler emits no event

  Scenario: idempotent same-content persist succeeds without another write event
    Given a registry built from a temp owner-home already holding kind "throwaway_kind"
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler succeeds
    And the persist handler emits no event

  Scenario: semantically equivalent but byte-different existing content is not idempotent
    Given a registry built from a temp owner-home already holding semantically equivalent but byte-different content for kind "throwaway_kind"
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a DuplicateKind error
    And the persist handler error code is "playbook_duplicate_kind_registration"
    And the persist handler emits no event
    And the handler target machine.yaml bytes for kind "throwaway_kind" are unchanged

  Scenario: same kind with different content at the owner-home yields a typed error and no event
    Given a registry built from a temp owner-home already holding different content for kind "throwaway_kind"
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a DuplicateKind error
    And the persist handler error code is "playbook_duplicate_kind_registration"
    And the persist handler emits no event

  Scenario: same kind with invalid existing content at the owner-home fails closed and does not overwrite
    Given a registry built from a temp owner-home already holding invalid content for kind "throwaway_kind"
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns an ExistingMachineInvalid error
    And the persist handler error code is "playbook_existing_machine_invalid"
    And the persist handler emits no event
    And the existing machine.yaml for kind "throwaway_kind" still contains "not: [valid"

  Scenario: argument kind must match the machine.yaml kind before any persist event
    Given a registry built from an empty temp owner-home
    And a minimal valid machine.yaml for kind "declared_kind"
    When the persist handler executes for kind "argument_kind"
    Then the persist handler returns a KindMismatch error
    And the persist handler error code is "kind_mismatch"
    And the persist handler emits no event

  Scenario: a machine with no outcome_predicate is refused at the persist write boundary
    Given a registry built from an empty temp owner-home
    And a machine.yaml with no outcome_predicate for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a LoaderInvalid error
    And the persist handler error code is "playbook_measurement_definition_missing"
    And the persist handler error names kind "throwaway_kind"
    And the persist handler emits no event

  Scenario: a machine whose non-terminal state has no hook is refused at the persist write boundary
    Given a registry built from an empty temp owner-home
    And a machine.yaml with an outcome_predicate but a hookless non-terminal state for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a NonTerminalStateHookless error
    And the persist handler error code is "playbook_non_terminal_state_hookless"
    And the persist handler error names kind "throwaway_kind"
    And the persist handler error names state "active"
    And the persist handler emits no event

  Scenario: a transitionless event-driven machine's non-initial custom-role state is accepted at the persist write boundary
    Given a registry built from an empty temp owner-home
    And a transitionless event-driven machine.yaml whose non-initial state carries only a custom-role hook for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns one PlaybookPersisted event
    And the PlaybookPersisted event carries kind "throwaway_kind"

  Scenario: a projection-only machine with transitions whose non-initial state carries only a worker hook is accepted at the persist write boundary
    Given a registry built from an empty temp owner-home
    And a projection-only machine.yaml with transitions whose non-initial state carries only a worker hook for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns one PlaybookPersisted event
    And the PlaybookPersisted event carries kind "throwaway_kind"

  Scenario: a state serving only a reviewer hook where the doer is served is refused at the persist write boundary
    Given a registry built from an empty temp owner-home
    And a machine.yaml whose initial state carries only a reviewer hook for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a NonTerminalStateHookless error
    And the persist handler error code is "playbook_non_terminal_state_hookless"
    And the persist handler error names kind "throwaway_kind"
    And the persist handler error names state "active"
    And the persist handler error names role "doer"
    And the persist handler emits no event

  # C-d.1 / C9. `registration_blocked()` had a production HOME and no production
  # PATH: nothing consulted it, so "the loader reports registration as blocked"
  # was a REPORT, and reporting a failure is not blocking a writer. The persist
  # handler IS the writer — both engine persist seams reach it — and a hearth
  # whose canonical root is not the root its definitions will be read from cannot
  # be written into, because the write produces an artifact nothing serves. The
  # first scenario in this feature is this one's control: the same request into
  # the same fixture WITHOUT the collision persists one event.
  Scenario: a persist into a hearth whose registration is blocked is refused before any write
    Given a registry built from an empty temp owner-home
    And that owner-home carries definitions under BOTH the legacy and canonical hearth roots
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a HearthRegistrationBlocked error
    And the persist handler error code is "hearth_registration_blocked"
    And the persist handler error names both hearth roots
    And the persist handler error names the collision code exactly once
    And the persist handler emits no event

  # THE HEARTH QUESTION IS ASKED BEFORE THE REQUEST IS VALIDATED, and this is
  # what makes that ordering a property rather than a comment. The request below
  # carries a SECOND, independent fault — no machine.yaml at all — which a
  # request guard one line further down would answer first. An implementation
  # that asks the hearth after any request guard reds here and nowhere else.
  #
  # It replaces an assertion that COULD NOT FAIL: "no machine.yaml was written
  # under either hearth root" stat'd the filesystem, and this handler is pure —
  # it emits events and never writes, so no implementation of it could redden
  # that check. It was cited in the record as evidence.
  # C-d.1 round 5, HIGH-1 AT THE WRITE BOUNDARY. The scenario above builds its
  # collision out of two READABLE roots. This one builds it out of a legacy
  # definition directory that cannot be inspected — root readable, entry not —
  # which round 4 read as "the legacy root holds no definition", answered
  # `Ok(NothingToMove)` for, left `registration_blocked()` at `None` for, and
  # therefore ALLOWED THE WRITE on: `persist = Ok(events=1)`. The refusal code
  # is asserted, not just the refusal, so the write cannot be blocked here for
  # some unrelated reason and still pass.
  Scenario: a persist into a hearth whose definition directory cannot be inspected is refused
    Given a registry built from an empty temp owner-home
    And that owner-home carries a legacy definition directory that cannot be inspected
    And a minimal valid machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler returns a HearthRegistrationBlocked error
    And the persist handler error code is "hearth_registration_blocked"
    And the persist handler error names the unreadable root
    And the persist handler emits no event

  Scenario: the target hearth is refused before the request is validated
    Given a registry built from an empty temp owner-home
    And that owner-home carries definitions under BOTH the legacy and canonical hearth roots
    And a request carrying no machine.yaml for kind "throwaway_kind"
    When the persist handler executes for kind "throwaway_kind"
    Then the persist handler emits no event
    And the refusal names the hearth rather than the request
