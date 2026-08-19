Feature: B-i.2's renamed identifier families mean what their new names say

  `spec.md:767`: "Each rename MUST have a type/field semantic assertion. A
  count-only rename is forbidden." `plan.md`'s B-i.2 names this file and names
  the mutation that must red: pasting `playbook_kind` over `workflow_kind`,
  rather than `artifact_kind`.

  Round 6 ran exactly that mutation against B-i.2b as landed — 102 sites
  retargeted to `playbook_kind`, same length so no column drift — and NOTHING
  RED. The workspace built, anvil-core stayed 1295/0, anvil-engine 521/0, the
  wire seam 4/0, and both verifiers passed, because `bi2-apply-rename.py
  --verify` only asserts the tree matches the artifact the implementer wrote and
  is therefore circular with respect to WHICH TARGET WAS CHOSEN. The semantic
  choice was defended in prose. Prose is not a check.

  THE DISTINCTION UNDER TEST. An ARTIFACT KIND is what an artifact IS — `track`,
  `decision`, `milestone`. A PLAYBOOK IDENTITY is the definition that governs
  that kind — `20260422T0000_track_lifecycle`. The registry maps one to the
  other, and they are disjoint namespaces. The renamed field holds the FIRST.
  A field called `playbook_kind` would claim it holds the second, which is false
  of every value the fold ever sees — so the name is wrong for a reason a test
  can state, not merely for a reason a reviewer can feel.

  The seam has THREE legs, because the mutation is a RENAME and a rename changes
  no value.
    LEG A  binds each renamed member by name in a pattern or type position — the
           fields by EXHAUSTIVE struct destructuring — so `playbook_kind` cannot
           compile.
    LEG A' requires the pinned set to equal the classification's `rename` targets,
           because Rust has no exhaustive construct for "every type this phase
           renamed" and a `vec![...]` of types would otherwise be a subset pin.
    LEG B  asserts the value domain that makes `artifact_kind` the RIGHT name
           rather than an arbitrary one, over real folds and a real registry —
           and is failable on its own terms: collapsing kind and identity reds
           it, and so does making fidelity measure the definition instead of the
           run.

  ONE ASSERTION PER FAMILY, as `plan.md`'s B-i.2 Verify requires. The four
  families it names, and the scenario that carries each one's meaning:

    | family (plan.md:799-804)                        | scenario                          |
    | `workflow_kind(s)` -> `artifact_kind(s)`        | "value domain is governed        |
    |                                                  |  artifact kinds" (+ disjointness) |
    | `WorkflowActivity` -> `ArtifactActivity`,        | "ArtifactActivity measures        |
    |   where it measures command/artifact activity    |  activity per governed kind"      |
    | `WorkflowFidelity` -> `PlaybookRunFidelity`,     | "PlaybookRunFidelity measures     |
    |   where it measures executions                   |  EXECUTIONS, one row per run"     |
    | `workflow_generation` -> `playbook_generation`,  | "keeps a bidirectional legacy     |
    |   with a legacy read alias                       |  read alias"                      |

  The last two scenarios are not per-family: they are LEG A (the pin itself) and
  LEG A' (the rule that the pin must grow when the classification does).

  Scenario: An artifact kind and the playbook that governs it are disjoint namespaces
    Given the real seed playbook registry
    Then every governed artifact kind resolves to a playbook identity that is not itself a kind
    And no governed artifact kind is equal to any playbook identity

  Scenario: The artifact-kind field's value domain is governed artifact kinds
    Given the real seed playbook registry
    And an activity-log stream whose artifact kinds are "track,decision,milestone"
    When the per-kind artifact activity is folded
    Then every folded key is a governed artifact kind
    And no folded key is a playbook identity

  Scenario: ArtifactActivity measures activity per governed artifact kind
    Given the real seed playbook registry
    And an activity-log stream whose artifact kinds are "track,track,decision"
    When the artifact activity is folded
    Then the entry for artifact kind "track" reports 2 calls
    And every entry names a governed artifact kind, never a playbook identity

  Scenario: PlaybookRunFidelity measures EXECUTIONS, one row per run
    Given the real seed playbook registry
    And two separate runs of artifact kind "track"
    When the playbook-run fidelity is folded
    Then the fidelity result carries 2 run instances
    And both run instances report artifact kind "track"

  Scenario: playbook_generation keeps a bidirectional legacy read alias
    Then the kind "playbook_generation" resolves the aliases "playbook_generation,workflow_generation"
    And the kind "workflow_generation" resolves the aliases "playbook_generation,workflow_generation"

  Scenario: The pin list is forced to grow with the classification
    Then the pinned members are exactly the classification artifact's rename targets

  Scenario: Every renamed family member is bound by name, not by count
    Then the renamed family members are exactly "ArtifactActivityEntry,ArtifactActivityQuery,ArtifactActivityResult,PlaybookRunFidelityResult,PlaybookRunInstanceFidelity,artifact_kind,artifact_kinds"
