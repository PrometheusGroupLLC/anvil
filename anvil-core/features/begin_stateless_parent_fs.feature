Feature: Begin resolves a stateless-but-transitioned parent via the fallback
  When a parent artifact's status.yaml has no top-level `state` but does carry
  transitions, begin must resolve the parent state from the last transition's
  `to` rather than hard-erroring. This exercises the real
  FileSystemQueryAdapter::read_artifact_state read site (not the in-memory
  adapter), so the fallback is genuinely covered.

  Scenario: Begin a child of a stateless-but-active parent succeeds
    # The create flow serves the (spec, doer) hook from the track playbook's
    # hooks/ dir (P5 migration), resolved via the SeedPlaybookRegistry's doer
    # hook declaration. Seed the hook body on disk so the body read succeeds;
    # this scenario's focus is the stateless-parent state fallback.
    Given a begin fs hearth with:
      | path                                                                        | content                                                                  |
      | proposals/20260403T1500_parent/status.yaml                                  | version: 1\nkind: proposal\ntransitions:\n  - to: vision\n  - to: active |
      | workflows/20260422T0000_track_lifecycle/hooks/spec-writing.md               | # Spec Writing\n\n## Overview\n\nWrite the spec.                          |
    When begin fs is executed with parent "20260403T1500_parent"
    Then the begin outcome is successful
