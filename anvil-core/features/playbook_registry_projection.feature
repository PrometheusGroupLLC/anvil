Feature: The playbooks registry projection, generations dual-read, and the hearth move

  C9 (`spec.md:1212-1226`). Four rules, each replacing something that used to be
  silent, all exercised against a real TempDir hearth and the production loader.

  THE PROJECTION IS COMPARED TO AN ORACLE DECLARED HERE, IN THIS FILE, NEVER TO
  A VALUE RECOMPUTED FROM THE PRODUCTION LOADER. The first version of this
  feature asserted the projection against `loaded_set(&hearth)` — the same
  function that BUILDS the projection. Expected and actual shrank in lockstep,
  so a loader that silently dropped every definition whose basename contains
  `playbook` left this feature 7/7 GREEN: precisely the regression this rename
  track exists to prevent, invisible to the scenario named for it. The expected
  ids are now written out below as literals. If the loader stops resolving one,
  the literal does not move with it.

  BEHAVIORAL MATRIX closed here — directory state x operation:
  legacy-only / canonical-only / both-present, against load, project, and move.
  The both-present column was DECLARED closed here while both-present x move and
  both-present x load were in fact uncovered and silently dropped definitions;
  it is covered now, by the two scenarios under "both present".

  DECLARED EXCLUSIONS, stated so they are not mistaken for coverage:
    * Multi-process concurrent moves are OUT OF SCOPE for this track. Two engines
      racing on one hearth is a supervisor concern; nothing here serializes them.
    * Legacy directory READS are not removed. They go only after the
      fleet/cached-payload gate proves no supported owner depends on them
      (`spec.md:1212-1226`), which is not this task's to claim.
    * WRITING the rendered projection to a live `{hearth}/playbooks.md`, and
      retiring the live `workflows.md`, are hearth DATA changes bound by the
      cutover gate. Rendering and refusing are covered here; applying is not.
    * The live hearth is never touched. Every scenario builds its own TempDir.

  # ── project ───────────────────────────────────────────────────────────────
  Scenario: The projection lists exactly the definitions the loader resolved
    Given a temporary hearth seeded with definitions "20260101T0000_alpha_lifecycle,20260528T2321_workflow_generation" and unloadable directories "20260202T0000_no_machine"
    When the production registry renders the playbooks projection
    Then the projection's active section lists exactly "20260101T0000_alpha_lifecycle,20260528T2321_workflow_generation"
    And the projection's exclusions section lists exactly "20260202T0000_no_machine"
    And the projection states the exclusion reason for "20260202T0000_no_machine"
    And the projection's active section carries a governed kind for every id it lists

  Scenario: A definition the loader cannot resolve leaves the active section and is stated as excluded
    Given a temporary hearth seeded with definitions "20260101T0000_alpha_lifecycle,20260528T2321_workflow_generation" and unloadable directories "20260202T0000_no_machine"
    And the definition "20260528T2321_workflow_generation" is made unloadable
    When the production registry renders the playbooks projection
    Then the projection's active section lists exactly "20260101T0000_alpha_lifecycle"
    And the projection's exclusions section lists exactly "20260202T0000_no_machine,20260528T2321_workflow_generation"

  # ── retire ────────────────────────────────────────────────────────────────
  Scenario: Retiring the legacy registry file requires a receipt naming it
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: some-other-file.md"
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

  Scenario: A receipt naming a different file that merely embeds the name is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: backup_of_workflows.md.bak"
    Then the retirement is refused
    And the refusal names what the receipt actually said
    And the legacy registry file is byte-identical to its preimage

  Scenario: A receipted retirement removes the legacy registry file
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: workflows.md"
    Then the legacy registry file is gone

  Scenario: A receipt naming the file by absolute path is accepted
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with a receipt entry naming the file by its full path
    Then the legacy registry file is gone

  # Basename equality is NECESSARY and NOT SUFFICIENT. This gate DELETES, so a
  # false accept is data loss: before these two, any token whose basename read
  # `workflows.md` authorized deleting THIS hearth's `workflows.md`, including
  # evidence about a file in a different hearth entirely. The accepted-path
  # scenario above cannot distinguish that from a correct implementation,
  # because it constructs this hearth's own path.
  Scenario: A receipt naming another hearth's file of the same name is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with a receipt entry naming ANOTHER hearth's file of the same name
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

  Scenario: A receipt naming a same-named file under a subdirectory of this hearth is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: archive/workflows.md"
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

  # THE BARE-BASENAME RESIDUAL, bounded. The path-scope fix shipped the previous
  # review's own prescribed rule — "a bare basename, or a path whose parent
  # resolves to this hearth" — and three of its four probe lines then refused.
  # The fourth still deleted: the tokenizer split on `[ ] ( )`, so a markdown
  # link DEGENERATED to a bare basename, and an entry naming another hearth's
  # location in prose deleted this hearth's registry on a bare token beside it.
  # A bare basename is unambiguous only when the entry points nowhere else.
  Scenario: A receipt whose markdown link degenerates to a bare basename is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: [workflows.md](x)"
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

  Scenario: A receipt naming this file by basename beside ANOTHER hearth's location is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with a receipt entry naming the basename beside ANOTHER hearth's path
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

  # C-d.1 round 5, M-3. `/` WAS NOT THE ONLY WAY TO NAME A PLACE. The round-4
  # ambiguity test returned false for any token with no `MAIN_SEPARATOR` in it,
  # so on Unix a Windows path, a UNC path, and a bare relative directory naming
  # another hearth were all invisible to it — and the gate DELETED this hearth's
  # registry on every one of them. They were outside the declared residual, and
  # this gate deletes, so they are pinned rather than declared.
  Scenario Outline: A receipt naming another location as "<shape>" is refused
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with a receipt entry naming another location as "<shape>"
    Then the retirement is refused
    And the legacy registry file is byte-identical to its preimage

    # C-d.1 round 6, L-1. `percent_encoded` is the fourth shape. A
    # percent-encoded path is a real convention for writing a path, and
    # `token_is_location_shaped` tested `/`, `\`, `~`, `://` and a real-directory
    # probe — none of which sees `%2F`. So `%2Fother%2Fhearth%2Fworkflows.md` was
    # invisible to the ambiguity test and this DELETING gate accepted the bare
    # basename beside it. It sat outside all three declared residual bullets: it
    # IS a path-shaped token, and the doc claimed "path-shaped in ANY convention".
    Examples:
      | shape           |
      | windows_path    |
      | unc_path        |
      | sibling_dirname |
      | percent_encoded |

  # C-d.1 round 5, L-3. Two undeclared FALSE NEGATIVES in the safe direction,
  # taken: backticks are how this codebase's own prose writes a filename, and a
  # correctly-formed markdown link to THIS hearth's own file is the other way an
  # operator writes the one receipt this gate exists to accept. Both refused.
  # The markdown fix reads the link's TARGET and never its label, so the
  # degenerate-to-bare-basename hole above stays closed — `[workflows.md](x)`
  # still refuses, in its own scenario, unchanged.
  Scenario: A receipt naming this file in backticks is accepted
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with the receipt entry "retired: `workflows.md`"
    Then the legacy registry file is gone

  Scenario: A receipt whose markdown link targets THIS hearth's own file is accepted
    Given a temporary hearth carrying a legacy workflows registry file
    When retirement is attempted with a markdown link to this hearth's own file
    Then the legacy registry file is gone

  # ── generations: dual-read, canonical-write ───────────────────────────────
  # This runs BEFORE any migration of workflow_generations/, which is the
  # ordering the task requires: nothing is moved out from under a reader.
  Scenario: A hearth holding only the legacy generations directory still resolves every generation
    Given a temporary hearth whose generations live only under the legacy directory
    When the generations are resolved
    Then every legacy generation is resolved
    And the legacy generations directory is byte-identical to its preimage
    And a new generation is written under the canonical generations directory

  Scenario: The same generation identity under both directories is refused
    Given a temporary hearth with one identity present under both generations directories
    When the generations are resolved
    Then the resolution is refused naming both paths
    And an explicit merge decision is required
    And both generations directories are byte-identical to their preimages

  # THE REFUSAL IS SCOPED TO THE COLLIDING IDENTITY. The first wiring of the
  # dual read into `locate_artifact_dir` returned `None` for EVERY bare-id lookup
  # in the hearth the moment one generation identity collided — tracks,
  # proposals, decisions, sparks, everything not addressed by full relative path
  # — because the error is a property of the two generations ROOTS and was
  # answered as if it were a property of the artifact being asked for. The
  # control assertion is the last line: without it the scoping fix is itself
  # unfalsifiable, since the colliding id refuses under both the broad and the
  # narrow implementation.
  Scenario: The production artifact lookup reads both generation roots and refuses a collision
    Given a temporary hearth whose generations live only under the legacy directory
    And that hearth also holds an unrelated track "20260101T0000_unrelated_track"
    Then the production artifact lookup resolves "20260601T0000_gen_beta"
    And a canonical generation "20260701T0000_gen_new" is created
    And the production artifact lookup resolves "20260701T0000_gen_new"
    And the production artifact lookup refuses "20260601T0000_gen_beta" once it exists under both roots
    And the production artifact lookup still resolves "20260101T0000_unrelated_track" while that collision stands

  # ── move ──────────────────────────────────────────────────────────────────
  Scenario: A completed hearth move preserves every basename and artifact id
    Given a temporary hearth whose definitions live under the pre-migration definitions directory
    When the legacy hearth directory is migrated
    Then the canonical directory holds every definition under its exact source basename
    And the legacy hearth directory is gone

  Scenario: An incomplete hearth move fails loudly and blocks registration
    Given a temporary hearth whose definitions live under the pre-migration definitions directory
    And the hearth move is set to fail
    When the production loader scans that hearth
    Then the loader reports a hearth directory move failure naming both paths
    And the loader reports registration as blocked
    And no definition was written into the canonical directory
    And the legacy directory still holds every definition under its exact basename

  # ── both present ──────────────────────────────────────────────────────────
  # The cell the preamble declared closed and was not. Before this, both-present
  # returned Ok(false) — "nothing to do" — while the scan read canonical only, so
  # every definition under the legacy root vanished from the registry with no
  # rename, no error and no log line.
  Scenario: Both hearth directories present is refused, naming every shadowed definition
    Given a temporary hearth carrying definitions under BOTH the legacy and canonical directories
    When the legacy hearth directory is migrated
    Then the migration is refused naming both roots
    And the refusal names every definition the legacy root would have shadowed
    And the refusal names the identity present under both roots
    And both hearth directories are byte-identical to their preimages

  Scenario: The production loader reports the both-directories collision and blocks registration
    Given a temporary hearth carrying definitions under BOTH the legacy and canonical directories
    When the production loader scans that hearth
    Then the loader reports a hearth directory collision naming both roots
    And the loader reports registration as blocked
    And the legacy-only definition is absent from the loaded set
    And both hearth directories are byte-identical to their preimages

  # THE REFUSAL IS KEYED TO WHAT IS LOST, AND THESE THREE ARE WHAT MAKE THAT A
  # PROPERTY RATHER THAN A SENTENCE. The two scenarios above always seeded a
  # shared identity under both roots, so a refusal keyed to a NAME COLLISION
  # passed all of them while silently dropping the likelier hearth state: a
  # partially-migrated hearth whose legacy definitions have NO canonical
  # namesake. That subcase is the one the refusal exists for, and it is first
  # below. The two after it pin the other direction — a legacy root that would
  # lose no definition must NOT refuse — so the pair is a biconditional and
  # neither "refuse on collision" nor "refuse on existence" survives it.
  Scenario: Legacy definitions with no canonical namesake are refused, because they are still lost
    Given a temporary hearth whose legacy definitions have NO canonical namesake
    When the legacy hearth directory is migrated
    Then the migration is refused naming both roots
    And the refusal names every legacy-only definition it would have shadowed
    And the refusal names no identity as present under both roots
    And both hearth directories are byte-identical to their preimages

  Scenario: The production loader reports a shadow-only collision and blocks registration
    Given a temporary hearth whose legacy definitions have NO canonical namesake
    When the production loader scans that hearth
    Then the loader reports a hearth directory collision naming both roots
    And the loader reports registration as blocked

  # The declared boundary, made to match its own stated rule. `list_dirs` counted
  # ANY subdirectory, so an empty leftover subdirectory or a `.cache/` under the
  # legacy root produced a hard refusal plus a blocked registration on a hearth
  # that had already completed its migration — losing zero definitions, which is
  # the exact outcome the boundary was written to prevent.
  Scenario: A legacy root that would lose no definition is not a collision
    Given a temporary hearth whose legacy root holds no definition at all
    When the production loader scans that hearth
    Then the loader reports no hearth directory collision
    And the loader reports registration as not blocked
    And the canonical definition still loaded

  # ── the losing direction: a root that cannot be ENUMERATED ────────────────
  # The biconditional above — refuse when a definition would be lost, do not
  # refuse when none would be — held only where the roots could be LISTED.
  # `list_dirs` swallowed every `read_dir` failure into an empty vector, so on a
  # legacy root that holds a real definition and cannot be enumerated the
  # migrate seam answered Ok(false), `registration_blocked_detail()` answered
  # None, and the persist WRITE boundary ALLOWED the write. A silent fallback
  # inside the guard whose stated purpose is to fail loud, on an ordinary
  # trigger: any `read_dir` failure, which for a desktop app reading hearths in
  # user directories means protected locations, stale mounts and descriptor
  # exhaustion — no operator mistake required.
  #
  # "Cannot enumerate" and "enumerated, found nothing" are DIFFERENT ANSWERS and
  # the two scenarios below are what makes them different in the code: the
  # empty-legacy-root scenario above must still NOT refuse.
  # L-1, taken: "and the legacy hearth directory still exists" USED TO BE the
  # third line of this scenario and COULD NOT FAIL in it — this fixture has both
  # roots present, so `hearth_root_diagnosis` can only answer `NothingToMove` or
  # `Err` and `MovePending` is unreachable, which means no implementation can
  # rename anything here. It is still asserted in the plain-file scenario below,
  # where the fixture makes the rename reachable and the assertion can red.
  Scenario: A legacy root that cannot be enumerated is refused, not read as empty
    Given a temporary hearth whose legacy root holds a definition and cannot be enumerated
    When the legacy hearth directory is migrated
    Then the migration is refused because a root could not be enumerated
    And the refusal names the unreadable root

  Scenario: The production loader blocks registration on a canonical root it cannot enumerate
    Given a temporary hearth whose canonical root cannot be enumerated
    When the production loader scans that hearth
    Then the loader reports registration as blocked
    And the loader reports an unreadable hearth root

  # ── C-d.1 round 5: the swallow is a PROPERTY, not a syscall ───────────────
  #
  # Round 4 keyed its fix to `read_dir` returning `Err` — the syscall its own
  # `0300` probe happened to hit — and left three per-ENTRY `stat` swallows
  # standing behind it: `entry.path().is_dir()`, `…/machine.yaml.is_file()` and
  # `artifact_dir.is_dir()`. `Path::is_dir()`/`is_file()`/`exists()` return
  # `bool` and map EVERY error to `false`, and on this path `false` means
  # "nothing here", which is the one answer that clears a writer.
  #
  # The property is: NO UNREADABLE INPUT MAY BE READ AS AN EMPTY ONE. These
  # outlines are parameterised on the mode because a single mode is what let the
  # instance stand in for the class twice. `0300` is traversable-not-readable
  # (`read_dir` fails); `0600`/`0400` are readable-not-traversable (`read_dir`
  # SUCCEEDS, every entry `stat` fails — the shape a stale NFS mount takes);
  # `0000` on a definition DIRECTORY under a perfectly readable root is round 4's
  # own declared row 3, which the code did not implement. `0755`/`0500` are
  # CONTROLS in the same table: they prove the fixture holds real definitions, so
  # no refusal row can pass vacuously.
  Scenario Outline: A canonical root the loader cannot inspect at mode "<mode>" answers "<blocked>"
    Given a temporary hearth whose canonical root holds two definitions at mode "<mode>"
    When the production loader scans that hearth
    Then the loader answer carries "<blocked>"
    And the loader resolved the definitions "<loaded>"

    Examples:
      | mode | blocked                 | loaded                                                  |
      | 0755 |                         | 20260101T0000_alpha_lifecycle,20260505T0000_legacy_only_beta |
      | 0500 |                         | 20260101T0000_alpha_lifecycle,20260505T0000_legacy_only_beta |
      | 0600 | hearth_root_unreadable  |                                                         |
      | 0400 | hearth_root_unreadable  |                                                         |
      | 0300 | hearth_root_unreadable  |                                                         |

  Scenario Outline: A legacy root holding a shadowed definition at mode "<mode>" refuses with "<code>"
    Given a temporary hearth whose legacy root holds a shadowed definition at mode "<mode>"
    When the legacy hearth directory is migrated
    Then the migration answer carries "<code>"

    Examples:
      | mode | code                       |
      | 0755 | hearth_directory_collision |
      | 0500 | hearth_directory_collision |
      | 0600 | hearth_root_unreadable     |
      | 0400 | hearth_root_unreadable     |
      | 0300 | hearth_root_unreadable     |

  # ROW 3 OF ROUND 4'S OWN TABLE: the root is readable, ONE ENTRY is not. This is
  # the reproduction that still carried the round-3 signature verbatim —
  # `Ok(NothingToMove)`, `blocked = None`, and `persist = Ok(events=1)`. The
  # persist half is pinned at its own seam in
  # `anvil-core/features/persist_playbook_handler.feature`.
  Scenario Outline: A legacy definition directory unreadable at mode "<mode>" refuses with "<code>"
    Given a temporary hearth whose legacy definition directory is at mode "<mode>"
    When the legacy hearth directory is migrated
    Then the migration answer carries "<code>"

    Examples:
      | mode | code                       |
      | 0755 | hearth_directory_collision |
      | 0500 | hearth_directory_collision |
      | 0000 | hearth_root_unreadable     |

  # SELF-SHADOW IS NOT SHADOWING, and this round made the false positive
  # expensive: wiring the refusal to the persist WRITER turned a spurious REPORT
  # into a hard refusal of every write into a healthy hearth. Two paths to one
  # directory lose nothing — there is no second copy.
  Scenario: A legacy root that is a symlink to the canonical root shadows nothing
    Given a temporary hearth whose legacy root is a symlink to the canonical root
    When the legacy hearth directory is migrated
    Then the migration reports nothing to move
    And the canonical hearth directory still holds its definition

  Scenario: The production loader does not block on a legacy root symlinked to canonical
    Given a temporary hearth whose legacy root is a symlink to the canonical root
    When the production loader scans that hearth
    Then the loader reports no hearth directory collision
    And the loader reports registration as not blocked
    And the canonical definition still loaded

  # `{hearth}/playbooks` as a plain FILE with canonical absent was RENAMED to
  # `playbooks` and reported as a completed migration — after which the loader's
  # own read_dir failure was swallowed into an empty registry. A file is not a
  # root, and saying so is the whole diagnostic.
  Scenario: A legacy root that is a plain file is refused and is not renamed
    Given a temporary hearth whose legacy root is a plain file
    When the legacy hearth directory is migrated
    Then the migration is refused because a root is not a directory
    And the canonical hearth directory was not created
    And the legacy hearth directory still exists

  # THE GUARD IS PURE, AND THAT IS WHAT LETS IT RUN AHEAD OF THE MUTATION.
  # `migrate_legacy_hearth_dir` was both the question and the move, so every
  # caller that only wanted to ask "may this hearth be registered into?"
  # performed a top-level directory rename to find out — including
  # `HearthPlaybookRegistry::new()`, which both engine persist seams construct.
  # An implementation of the diagnosis in terms of the migration passes every
  # other assertion in this file and reds here.
  Scenario: The hearth-root guard answers a legacy-only hearth without moving it
    Given a temporary hearth whose definitions live under the pre-migration definitions directory
    When the hearth root guard is asked
    Then the guard reports a pending move
    And the legacy hearth directory still exists
    And the canonical hearth directory was not created

  # ── registration_blocked's precision ──────────────────────────────────────
  # `registration_blocked()` returns ONLY the two hearth-level variants. Replacing
  # its body with `self.errors.first()` — any load error blocks the hearth — left
  # the feature 14/14 green, so the claim lived in a doc comment and nothing else.
  # Under that mutation a hearth whose only fault is one bad machine.yaml reports
  # itself unwritable, which would block every registrar against a fine hearth.
  Scenario: A hearth whose only fault is a malformed definition is still writable
    Given a temporary hearth whose only fault is a malformed definition
    When the production loader scans that hearth
    Then the loader reports the malformed definition as invalid
    And the loader reports registration as not blocked

  # ── error attribution ─────────────────────────────────────────────────────
  # The engine's id extractor maps both hearth-directory failures to NO artifact
  # id. That arm lived in a private fn inside the engine binary and was reachable
  # from no feature: replacing it with a sentinel left the engine suite green.
  Scenario: A hearth directory failure attributes to no artifact id
    Given a temporary hearth carrying definitions under BOTH the legacy and canonical directories
    When the production loader scans that hearth
    Then every hearth directory error attributes to no artifact id
    And a malformed definition error still attributes to its own artifact id

  # ── C-d.1 round 6, M-1: the guard path is WIDER than the scanned modules ──
  #
  # The round-5 lint names two files by hand. `loader::list_hook_filenames` is
  # one frame below one of them — called from the scanned `hearth_registry`'s
  # `hook_file_names` — and on unmutated round-5 HEAD it carried BOTH prior
  # signatures verbatim:
  #
  #   let Ok(dir_iter) = std::fs::read_dir(hooks_dir) else { Vec::new() };  (round 3)
  #   if !path.is_file() { return None; }                                   (round 4)
  #
  # So an unreadable `hooks/` answered "this artifact has no hook files", the
  # loader then rejected the machine for referencing a hook that IS ON DISK
  # (`playbook_unknown_hook_reference`), the definition was DROPPED, and
  # `registration_blocked()` answered None. Milder than the root case — it
  # refuses rather than clearing the write boundary — and still the same class:
  # an unreadable input read as an empty one, producing a false diagnostic that
  # sends an operator to fix a `machine.yaml` that is correct.
  #
  # `0755`/`0500` are CONTROLS in the same table: the identical fixture,
  # readable, resolves the definition and blocks nothing. `0600` is
  # readable-not-traversable, where `read_dir` SUCCEEDS and the per-entry `stat`
  # is what fails — the round-4 half of the defect, at this new site.
  Scenario Outline: A definition whose hooks directory is at mode "<mode>" answers "<blocked>"
    Given a temporary hearth whose definition carries a hooks directory at mode "<mode>"
    When the production loader scans that hearth
    # ORDER IS DELIBERATE. The false-diagnostic check runs FIRST because it is
    # the only one of the three whose red state is reachable in THIS scenario:
    # every state that produces a false `playbook_unknown_hook_reference` also
    # empties LOADED and leaves BLOCKED empty, so behind the other two it could
    # never fail and would be an unfailable assertion — which is the defect this
    # track has already been caught by. The two behind it are reused steps with
    # their own measured red states elsewhere in this feature.
    Then no hook file on disk is reported as an unknown reference
    And the loader answer carries "<blocked>"
    And the loader resolved the definitions "<loaded>"

    Examples:
      | mode | blocked                | loaded                        |
      | 0755 |                        | 20260101T0000_alpha_lifecycle |
      | 0500 |                        | 20260101T0000_alpha_lifecycle |
      | 0600 | hearth_root_unreadable |                               |
      | 0300 | hearth_root_unreadable |                               |
      | 0000 | hearth_root_unreadable |                               |

  # ── C-d.1 round 6, M-1 second reach: the artifact LOOKUP ──────────────────
  #
  # `fs_snapshot_adapter::locate_artifact_dir` is the SOLE production consumer of
  # the `resolve_generation_identity` round 5 made fallible — and it bracketed
  # that fixed call with `literal.exists()` before it and `candidate.exists()`
  # after it, neither of them scanned. Both map EACCES onto `false`, and all
  # thirteen call sites turn the resulting `None` into `SnapshotError::NotFound`,
  # which the engine maps to gRPC NOT_FOUND. An artifact directory that exists
  # and cannot be inspected was reported to the operator as an artifact that is
  # not there.
  #
  # "I could not look" is now `IoError` (gRPC INTERNAL), which is a different
  # answer with a different operator action. `0755`/`0500` are controls: the same
  # track, inspectable, resolves. `0600` is readable-not-traversable, so the
  # directory listing succeeds and the `stat` beneath it does not.
  Scenario Outline: A track under a per-kind directory at mode "<mode>" answers "<answer>"
    Given a temporary hearth holding a track under a per-kind directory at mode "<mode>"
    When the production artifact lookup is asked for that track by its full path
    Then the artifact lookup answers "<answer>"

    Examples:
      | mode | answer        |
      | 0755 | resolved      |
      | 0500 | resolved      |
      | 0600 | uninspectable |
      | 0000 | uninspectable |
