Feature: artifact_of_record — one authority for the artifact and its placeholder
  The transition assessment and the open-step sweep both need to know, for a
  given kind and state, WHICH file is the artifact of record and what its
  freshly-scaffolded bytes look like. Today that knowledge lives only inside
  `initial_scaffold_files`, which bootstrap consumes and nobody else can.

  This seam exposes it, so the placeholder identity is DERIVED from the writer
  rather than copied beside it. A copy is how a byte comparison silently stops
  matching after someone edits the bootstrap format — the check would keep
  passing while seeing nothing, which is the failure mode this whole track
  exists to detect.

  `display_name` is a parameter because the placeholder cannot be produced
  without it: every scaffold body embeds it. A two-argument version of this
  function promised byte-identical output while withholding the only input that
  determines those bytes.

  Scenario: a track in its initial state declares spec.md and the exact bootstrap bytes
    When the artifact of record is resolved for kind "track" state "spec" display name "Alpha Track"
    Then the artifact of record path is "spec.md"
    And the artifact of record placeholder is "# Alpha Track\n"

  # The placeholder must track the display name, because that is what makes the
  # byte comparison meaningful per-artifact rather than per-kind.
  Scenario: the placeholder carries the display name
    When the artifact of record is resolved for kind "track" state "spec" display name "Beta"
    Then the artifact of record placeholder is "# Beta\n"

  # Multi-file kinds are OUT OF SCOPE for this track, and say so rather than
  # having an implementer pick one during a task. Which of research.md,
  # proposal.md, plan.md or draft.md is "the" artifact for a generation workflow
  # is a policy decision, and inventing it inside an implementation is how
  # undocumented conventions get born.
  Scenario Outline: multi-file kinds decline
    When the artifact of record is resolved for kind "<kind>" state "<state>" display name "X"
    Then there is no artifact of record

    Examples:
      | kind                | state     |
      | playbook_generation | gathering |
      | decision            | draft     |

  # Only the INITIAL state is covered. Every other state records
  # not_applicable, which is honest about the limit instead of guessing.
  Scenario: a non-initial state declines
    When the artifact of record is resolved for kind "track" state "spec_review" display name "X"
    Then there is no artifact of record

  # NOT TESTED HERE, deliberately. An earlier draft of this file carried a
  # scenario calling itself "the mutation test": it resolved the seam and
  # compared the result to... the seam. A tautology that passes unconditionally,
  # which is worse than no test, because it reports coverage of the one property
  # this design depends on.
  #
  # Non-drift between the seam and the bootstrap writer is guaranteed BY
  # CONSTRUCTION — `artifact_of_record` derives from `initial_scaffold_files`
  # rather than copying it, so there is no second value to drift. The property a
  # test CAN falsify is different and stronger: that the seam's bytes match what
  # a real `begin` actually writes to disk. That needs a written file, so it
  # lands with the sweep in a later phase, against real hearth artifacts.
