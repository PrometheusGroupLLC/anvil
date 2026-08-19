Feature: Begin serves the (spec, doer) hook from the on-disk hearth, hearth-first
  AC-4: editing the on-disk track playbook's hook declaration + hook body
  changes what begin serves with NO engine rebuild. This proves the
  hearth-first, registry-consulted resolution path: begin resolves the hook
  declaration via CompositePlaybookRegistry (HearthPlaybookRegistry first,
  SeedPlaybookRegistry fallback) and reads the body via the real
  FileSystemQueryAdapter from {hearth}/workflows/{id}/hooks/.

  This is the only Phase-5 scenario that exercises the genuine on-disk path
  end to end with a fresh registry per call. Both reads (the declaration and
  the body) come from disk, so a same-process edit to the hooks/ file is
  reflected on the next begin with no rebuild — the literal AC-4 claim.

  Scenario: begin serves the on-disk doer hook body, and an in-process edit changes it
    Given a begin fs hearth declaring a spec doer hook with body:
      """
      # On-Disk Spec Writing Hook

      DISTINCTIVE-HEARTH-FIRST-MARKER-ALPHA
      """
    When begin fs hearth-first is executed with parent "20260601T0000_parent"
    Then the begin outcome is successful
    And the begin outcome result context_text contains "DISTINCTIVE-HEARTH-FIRST-MARKER-ALPHA"
    When the on-disk doer hook body is rewritten to:
      """
      # On-Disk Spec Writing Hook (edited)

      DISTINCTIVE-HEARTH-FIRST-MARKER-BETA
      """
    And begin fs hearth-first is executed with parent "20260601T0000_parent"
    Then the begin outcome is successful
    And the begin outcome result context_text contains "DISTINCTIVE-HEARTH-FIRST-MARKER-BETA"
    And the begin outcome result context_text does not contain "DISTINCTIVE-HEARTH-FIRST-MARKER-ALPHA"
