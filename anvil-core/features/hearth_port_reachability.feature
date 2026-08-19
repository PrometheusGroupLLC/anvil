Feature: Every port method answers "I could not look" as itself, at every address form

  # ── C-d.1 round 7, H-1: the closure criterion is this table, not a module list ──
  #
  # Six rounds each closed a hand-written set of modules and each was defeated by
  # a site nobody named. `anvil-core/src/hearth/fs_query_adapter.rs` — the
  # `QueryPort` `anvil-engine` constructs ELEVEN times — carried all four
  # historical spellings of the swallow and appears in NO section of the
  # implementation record. Inside the sibling adapter that round 6 did fix,
  # `read_artifact_kind` answered NOT_FOUND for an artifact on disk while
  # `read_artifact_state` — same file, same artifact, same mode, thirty-five
  # lines away — answered `IoError`.
  #
  # So the criterion here is behavioural and it is enumerated from the PORT
  # TRAITS: every method x every address form it accepts x every failure mode,
  # and the class is closed when the table is green. A module list cannot see two
  # methods of one port disagreeing; a table with both of them in it cannot miss
  # it.
  #
  # WHAT EVERY ROW HOLDS CONSTANT: the same hearth, seeded with the same two real
  # artifacts of two different kinds, a registry, a projection, an op log, a hook
  # file and a context file. Only the MODE of one named directory or file varies.
  # The step module refuses to evaluate any row whose fixture did not seed both
  # artifacts, so no row can pass over an empty hearth.
  #
  # WHAT THE ANSWERS MEAN:
  #   resolved      — the method returned its WHOLE correct value. Not "Ok": the
  #                   value, compared against a literal declared in the step
  #                   module. A SHORT enumeration fails a `resolved` row, which
  #                   is the H-1 consequence asserting itself.
  #   uninspectable — the method refused with the I/O class. `NotFound` fails
  #                   here (gRPC NOT_FOUND tells an operator an artifact on disk
  #                   is not there), and so does any `Ok`.
  #
  # WHY `0300` IS IN EVERY TABLE. A directory at `0300` is traversable and not
  # listable: `stat` succeeds, `read_dir` fails. So the per-artifact lookups must
  # RESOLVE at `0300` and the enumerations must REFUSE. Without that row, "every
  # mode that is not 0755 refuses" would satisfy this feature — a check that has
  # stopped discriminating between the fix and a blanket refusal.

  # ── A: artifact-addressed reads, x BOTH address forms ────────────────────
  #
  # The address axis is the one that caught the disagreement. A bare id is
  # resolved by probing the filesystem; a relative path whose first component
  # names a known kind directory is resolved by mapping the PREFIX, with no
  # filesystem access at all. That is why every `read_artifact_kind` row in the
  # `relative path` column is declared `resolved` at every mode: it is correct
  # there, and it was correct there while the same method by BARE ID answered
  # NOT_FOUND for the same artifact. Declaring the cell is the point — an omitted
  # cell is what a module list is made of.
  Scenario Outline: The "<port>" port method "<method>" by "<address>" over a per-kind directory at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "the per-kind directory" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "<address>"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                    | address       | mode | answer        |
      | query    | read_artifact_kind        | bare id       | 0755 | resolved      |
      | query    | read_artifact_kind        | bare id       | 0300 | resolved      |
      | query    | read_artifact_kind        | bare id       | 0600 | uninspectable |
      | query    | read_artifact_kind        | bare id       | 0000 | uninspectable |
      | query    | read_artifact_kind        | relative path | 0755 | resolved      |
      | query    | read_artifact_kind        | relative path | 0300 | resolved      |
      | query    | read_artifact_kind        | relative path | 0600 | resolved      |
      | query    | read_artifact_kind        | relative path | 0000 | resolved      |
      | query    | read_artifact_state       | bare id       | 0755 | resolved      |
      | query    | read_artifact_state       | bare id       | 0300 | resolved      |
      | query    | read_artifact_state       | bare id       | 0600 | uninspectable |
      | query    | read_artifact_state       | bare id       | 0000 | uninspectable |
      | query    | read_artifact_state       | relative path | 0755 | resolved      |
      | query    | read_artifact_state       | relative path | 0300 | resolved      |
      | query    | read_artifact_state       | relative path | 0600 | uninspectable |
      | query    | read_artifact_state       | relative path | 0000 | uninspectable |
      | query    | read_artifact_status      | bare id       | 0755 | resolved      |
      | query    | read_artifact_status      | bare id       | 0300 | resolved      |
      | query    | read_artifact_status      | bare id       | 0600 | uninspectable |
      | query    | read_artifact_status      | bare id       | 0000 | uninspectable |
      | query    | read_artifact_status      | relative path | 0755 | resolved      |
      | query    | read_artifact_status      | relative path | 0300 | resolved      |
      | query    | read_artifact_status      | relative path | 0600 | uninspectable |
      | query    | read_artifact_status      | relative path | 0000 | uninspectable |
      | query    | read_activity_entries     | bare id       | 0755 | resolved      |
      | query    | read_activity_entries     | bare id       | 0300 | resolved      |
      | query    | read_activity_entries     | bare id       | 0600 | uninspectable |
      | query    | read_activity_entries     | bare id       | 0000 | uninspectable |
      | query    | read_activity_entries     | relative path | 0755 | resolved      |
      | query    | read_activity_entries     | relative path | 0300 | resolved      |
      | query    | read_activity_entries     | relative path | 0600 | uninspectable |
      | query    | read_activity_entries     | relative path | 0000 | uninspectable |
      | query    | read_transitions          | bare id       | 0755 | resolved      |
      | query    | read_transitions          | bare id       | 0300 | resolved      |
      | query    | read_transitions          | bare id       | 0600 | uninspectable |
      | query    | read_transitions          | bare id       | 0000 | uninspectable |
      | query    | read_transitions          | relative path | 0755 | resolved      |
      | query    | read_transitions          | relative path | 0300 | resolved      |
      | query    | read_transitions          | relative path | 0600 | uninspectable |
      | query    | read_transitions          | relative path | 0000 | uninspectable |
      | query    | read_transitions_strict   | bare id       | 0755 | resolved      |
      | query    | read_transitions_strict   | bare id       | 0300 | resolved      |
      | query    | read_transitions_strict   | bare id       | 0600 | uninspectable |
      | query    | read_transitions_strict   | bare id       | 0000 | uninspectable |
      | query    | read_transitions_strict   | relative path | 0755 | resolved      |
      | query    | read_transitions_strict   | relative path | 0300 | resolved      |
      | query    | read_transitions_strict   | relative path | 0600 | uninspectable |
      | query    | read_transitions_strict   | relative path | 0000 | uninspectable |
      | snapshot | read_artifact_kind        | bare id       | 0755 | resolved      |
      | snapshot | read_artifact_kind        | bare id       | 0300 | resolved      |
      | snapshot | read_artifact_kind        | bare id       | 0600 | uninspectable |
      | snapshot | read_artifact_kind        | bare id       | 0000 | uninspectable |
      | snapshot | read_artifact_kind        | relative path | 0755 | resolved      |
      | snapshot | read_artifact_kind        | relative path | 0300 | resolved      |
      | snapshot | read_artifact_kind        | relative path | 0600 | resolved      |
      | snapshot | read_artifact_kind        | relative path | 0000 | resolved      |
      | snapshot | read_artifact_state       | bare id       | 0755 | resolved      |
      | snapshot | read_artifact_state       | bare id       | 0300 | resolved      |
      | snapshot | read_artifact_state       | bare id       | 0600 | uninspectable |
      | snapshot | read_artifact_state       | bare id       | 0000 | uninspectable |
      | snapshot | read_artifact_state       | relative path | 0755 | resolved      |
      | snapshot | read_artifact_state       | relative path | 0300 | resolved      |
      | snapshot | read_artifact_state       | relative path | 0600 | uninspectable |
      | snapshot | read_artifact_state       | relative path | 0000 | uninspectable |
      | snapshot | read_artifact_actor_names | bare id       | 0755 | resolved      |
      | snapshot | read_artifact_actor_names | bare id       | 0300 | resolved      |
      | snapshot | read_artifact_actor_names | bare id       | 0600 | uninspectable |
      | snapshot | read_artifact_actor_names | bare id       | 0000 | uninspectable |
      | snapshot | read_artifact_actor_names | relative path | 0755 | resolved      |
      | snapshot | read_artifact_actor_names | relative path | 0300 | resolved      |
      | snapshot | read_artifact_actor_names | relative path | 0600 | uninspectable |
      | snapshot | read_artifact_actor_names | relative path | 0000 | uninspectable |
      | snapshot | read_activity_entries     | bare id       | 0755 | resolved      |
      | snapshot | read_activity_entries     | bare id       | 0300 | resolved      |
      | snapshot | read_activity_entries     | bare id       | 0600 | uninspectable |
      | snapshot | read_activity_entries     | bare id       | 0000 | uninspectable |
      | snapshot | read_activity_entries     | relative path | 0755 | resolved      |
      | snapshot | read_activity_entries     | relative path | 0300 | resolved      |
      | snapshot | read_activity_entries     | relative path | 0600 | uninspectable |
      | snapshot | read_activity_entries     | relative path | 0000 | uninspectable |
      | snapshot | read_transitions          | bare id       | 0755 | resolved      |
      | snapshot | read_transitions          | bare id       | 0300 | resolved      |
      | snapshot | read_transitions          | bare id       | 0600 | uninspectable |
      | snapshot | read_transitions          | bare id       | 0000 | uninspectable |
      | snapshot | read_transitions          | relative path | 0755 | resolved      |
      | snapshot | read_transitions          | relative path | 0300 | resolved      |
      | snapshot | read_transitions          | relative path | 0600 | uninspectable |
      | snapshot | read_transitions          | relative path | 0000 | uninspectable |

  # ── B: reads addressed by a hearth-relative path, over the directory that
  #       holds what they read ─────────────────────────────────────────────
  #
  # These methods take a path and join it; they never go through the artifact
  # lookup, so the bare-id form is not an address they accept and there is no
  # cell for it. `read_carry_forward_if_present` and `read_artifact_text` were
  # ALREADY correct before this round — they are in the table because a matrix
  # that only contains broken cells is a list of known defects wearing a table's
  # clothes, and because they are the controls that prove the refusing rows are
  # not an artifact of the fixture.
  Scenario Outline: The "<port>" port method "<method>" over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "relative path"
    Then that port method answers "<answer>"

    Examples:
      | port  | method                        | unreadable             | mode | answer        |
      | query | read_op_log                   | the artifact directory | 0755 | resolved      |
      | query | read_op_log                   | the artifact directory | 0300 | resolved      |
      | query | read_op_log                   | the artifact directory | 0600 | uninspectable |
      | query | read_op_log                   | the artifact directory | 0000 | uninspectable |
      | query | read_carry_forward_if_present | the artifact directory | 0755 | resolved      |
      | query | read_carry_forward_if_present | the artifact directory | 0300 | resolved      |
      | query | read_carry_forward_if_present | the artifact directory | 0600 | uninspectable |
      | query | read_carry_forward_if_present | the artifact directory | 0000 | uninspectable |
      | query | read_artifact_text            | the artifact directory | 0755 | resolved      |
      | query | read_artifact_text            | the artifact directory | 0300 | resolved      |
      | query | read_artifact_text            | the artifact directory | 0600 | uninspectable |
      | query | read_artifact_text            | the artifact directory | 0000 | uninspectable |
      | query | read_context_file             | the context directory  | 0755 | resolved      |
      | query | read_context_file             | the context directory  | 0300 | resolved      |
      | query | read_context_file             | the context directory  | 0600 | uninspectable |
      | query | read_context_file             | the context directory  | 0000 | uninspectable |
      | query | read_playbook_hook_body       | the hooks directory    | 0755 | resolved      |
      | query | read_playbook_hook_body       | the hooks directory    | 0300 | resolved      |
      | query | read_playbook_hook_body       | the hooks directory    | 0600 | uninspectable |
      | query | read_playbook_hook_body       | the hooks directory    | 0000 | uninspectable |

  # ── C: the ENUMERATIONS, over the hearth root and over one per-kind dir ──
  #
  # This is where the finding is at its purest. On unmutated `579c7b3`, with the
  # hearth root at `0600` and two artifacts on disk, `list_artifacts` answered
  # `Ok([])` — an unreadable root reading as an EMPTY REGISTRY, with no error and
  # no diagnostic — and `find_artifact_by_kind_origin_turn` answered `Ok(None)`,
  # which is `begin`'s idempotency gate saying "no artifact exists for this turn"
  # over one that does, and clearing a DUPLICATE WRITE. With only the per-kind
  # directory unreadable, `list_artifacts` answered `Ok` with the OTHER artifact
  # and silently dropped the track.
  #
  # `0000` refusing while `0600` did not is what proved `0600` was a swallow and
  # not a uniform failure: at `0600` `read_dir` SUCCEEDS and the per-entry `stat`
  # is what fails. Both rows are here so that stays visible.
  Scenario Outline: The "<port>" enumeration "<method>" over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port  | method                            | unreadable             | mode | answer        |
      | query | list_artifacts                    | the hearth root        | 0755 | resolved      |
      | query | list_artifacts                    | the hearth root        | 0500 | resolved      |
      | query | list_artifacts                    | the hearth root        | 0300 | uninspectable |
      | query | list_artifacts                    | the hearth root        | 0600 | uninspectable |
      | query | list_artifacts                    | the hearth root        | 0000 | uninspectable |
      | query | list_artifacts                    | the per-kind directory | 0755 | resolved      |
      | query | list_artifacts                    | the per-kind directory | 0500 | resolved      |
      | query | list_artifacts                    | the per-kind directory | 0300 | uninspectable |
      | query | list_artifacts                    | the per-kind directory | 0600 | uninspectable |
      | query | list_artifacts                    | the per-kind directory | 0000 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the hearth root        | 0755 | resolved      |
      | query | find_artifact_by_kind_origin_turn | the hearth root        | 0500 | resolved      |
      | query | find_artifact_by_kind_origin_turn | the hearth root        | 0300 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the hearth root        | 0600 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the hearth root        | 0000 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the per-kind directory | 0755 | resolved      |
      | query | find_artifact_by_kind_origin_turn | the per-kind directory | 0500 | resolved      |
      | query | find_artifact_by_kind_origin_turn | the per-kind directory | 0300 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the per-kind directory | 0600 | uninspectable |
      | query | find_artifact_by_kind_origin_turn | the per-kind directory | 0000 | uninspectable |

  # ── D: the reads whose result is WRITTEN ────────────────────────────────
  #
  # Four `SnapshotPort` methods read a source file and then write something
  # derived from it. Their reads were `read_to_string(p).unwrap_or_default()` and
  # `read_to_string(p).ok()?` — so an unreadable registry rebuilt the projection
  # as though the hearth held nothing, and the empty result was written over the
  # real one. This is the class's own consequence (an unreadable input read as an
  # empty one, ending in a permitted write) and the only place on this track
  # where the write is not merely permitted but destructive.
  #
  # The FILE is mode-varied here, not its directory, so the write target stays
  # writable and the clobber is reachable. `0200` (write-only) is the sharp row:
  # the file is right there and the process may not read it.
  Scenario Outline: The "<port>" port method "<method>" whose source "<unreadable>" is at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                                      | unreadable             | mode | answer        |
      | snapshot | registry_entry_exists                       | the registry file      | 0644 | resolved      |
      | snapshot | registry_entry_exists                       | the registry file      | 0000 | uninspectable |
      | snapshot | registry_entry_exists                       | the registry file      | 0200 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the registry file      | 0644 | resolved      |
      | snapshot | resolve_display_name_via_move_execution_row | the registry file      | 0000 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the registry file      | 0200 | uninspectable |
      | snapshot | rebuild_decisions_projection                | the decisions registry | 0644 | resolved      |
      | snapshot | rebuild_decisions_projection                | the decisions registry | 0000 | uninspectable |
      | snapshot | rebuild_decisions_projection                | the decisions registry | 0200 | uninspectable |
      | snapshot | rebuild_sparks_projection                   | the sparks source      | 0644 | resolved      |
      | snapshot | rebuild_sparks_projection                   | the sparks source      | 0000 | uninspectable |
      | snapshot | rebuild_sparks_projection                   | the sparks source      | 0200 | uninspectable |

  # ── E: the PER-ENTRY failure mode — every directory readable, one FILE not ──
  #
  # Added by round 7's OWN unfailability audit, which is the part of this feature
  # worth reading. With only the directory-level modes in the table, restoring
  # `status_path.exists()` and the swallowing `continue` beneath it — the exact
  # round-4 spelling, at begin's idempotency gate — left all 140 cells GREEN.
  # Not because the site was fixed: because the LISTING EDGE above it refused
  # first and masked it. A green table over a restored defect is the thing this
  # track has been caught by three times, and it was about to happen inside the
  # matrix written to prevent it.
  #
  # These rows leave every directory at 0755 and take the mode off ONE FILE, so
  # the enumeration succeeds, `stat` succeeds, and the READ is what fails. That
  # is the only fixture that can tell a swallow at the per-entry read from a
  # refusal above it. `0200` is the sharp row: the file is present, `stat`
  # answers, and the process may not read it.
  Scenario Outline: The "<port>" port method "<method>" whose artifact status file is at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "the artifact status file" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                            | mode | answer                  |
      | query    | list_artifacts                    | 0644 | resolved                |
      | query    | list_artifacts                    | 0200 | uninspectable           |
      | query    | list_artifacts                    | 0000 | uninspectable           |
      | query    | find_artifact_by_kind_origin_turn | 0644 | resolved                |
      | query    | find_artifact_by_kind_origin_turn | 0200 | uninspectable           |
      | query    | find_artifact_by_kind_origin_turn | 0000 | uninspectable           |
      | query    | read_artifact_state               | 0644 | resolved                |
      | query    | read_artifact_state               | 0200 | uninspectable           |
      | query    | read_artifact_state               | 0000 | uninspectable           |
      | query    | read_artifact_status              | 0644 | resolved                |
      | query    | read_artifact_status              | 0200 | uninspectable           |
      | query    | read_artifact_status              | 0000 | uninspectable           |
      | query    | read_activity_entries             | 0644 | resolved                |
      | query    | read_activity_entries             | 0200 | uninspectable           |
      | query    | read_activity_entries             | 0000 | uninspectable           |
      | snapshot | read_artifact_state               | 0644 | resolved                |
      | snapshot | read_artifact_state               | 0200 | uninspectable           |
      | snapshot | read_artifact_state               | 0000 | uninspectable           |
      | snapshot | read_artifact_kind                | 0644 | kind from the directory |
      | snapshot | read_artifact_kind                | 0200 | kind from the directory |
      | snapshot | read_artifact_kind                | 0000 | kind from the directory |
      | snapshot | read_activity_entries             | 0644 | resolved                |
      | snapshot | read_activity_entries             | 0200 | uninspectable           |
      | snapshot | read_activity_entries             | 0000 | uninspectable           |

  # ── F: the THIRD address form — a bare id under a kind dir not in KIND_DIRS ──
  #
  # Also added by the audit. `scan_subdirs_for_artifact` is the function that
  # carried `read_dir(hearth).ok()?`, `entries.flatten()`, `subdir.is_dir()` and
  # `candidate.join("status.yaml").exists()` — the round-3, round-4 and round-5
  # spellings, in eleven lines — and it is reached ONLY when the literal join and
  # the per-kind loop have both answered "absent". A track lives under `tracks/`,
  # which is in `KIND_DIRS`, so no row addressing the track ever reaches it:
  # restoring all four spellings left the table GREEN.
  #
  # The knowledge artifact lives under `knowledge/`, which is not, so these rows
  # are the only ones in the feature that exercise the scan. An address form the
  # matrix does not carry is a hole with the same shape as a module a list does
  # not name.
  Scenario Outline: The "<port>" port method "<method>" for a non-legacy kind over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id, non-legacy kind"
    Then that port method answers "<answer>"

    Examples:
      | port     | method               | unreadable                    | mode | answer               |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0755 | resolved             |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0300 | resolved             |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0600 | uninspectable        |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0000 | uninspectable        |
      | query    | read_artifact_kind   | the non-legacy status file    | 0644 | resolved             |
      | query    | read_artifact_kind   | the non-legacy status file    | 0200 | uninspectable        |
      | query    | read_artifact_kind   | the non-legacy status file    | 0000 | uninspectable        |
      | query    | read_artifact_state  | the non-legacy kind directory | 0755 | resolved             |
      | query    | read_artifact_state  | the non-legacy kind directory | 0300 | resolved             |
      | query    | read_artifact_state  | the non-legacy kind directory | 0600 | uninspectable        |
      | query    | read_artifact_state  | the non-legacy kind directory | 0000 | uninspectable        |
      | query    | read_artifact_state  | the non-legacy status file    | 0644 | resolved             |
      | query    | read_artifact_state  | the non-legacy status file    | 0200 | uninspectable        |
      | query    | read_artifact_state  | the non-legacy status file    | 0000 | uninspectable        |
      | query    | read_artifact_status | the non-legacy kind directory | 0755 | resolved             |
      | query    | read_artifact_status | the non-legacy kind directory | 0300 | resolved             |
      | query    | read_artifact_status | the non-legacy kind directory | 0600 | uninspectable        |
      | query    | read_artifact_status | the non-legacy kind directory | 0000 | uninspectable        |
      | query    | read_artifact_status | the non-legacy status file    | 0644 | resolved             |
      | query    | read_artifact_status | the non-legacy status file    | 0200 | uninspectable        |
      | query    | read_artifact_status | the non-legacy status file    | 0000 | uninspectable        |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0755 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0300 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0600 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0000 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0644 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0200 | address not accepted |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0000 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0755 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0300 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0600 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0000 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0644 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0200 | address not accepted |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0000 | address not accepted |

  # ── G: the registry gates over an unreadable hearth ROOT ────────────────
  #
  # Third audit finding. `registry_entry_exists` opens with `path.exists()`, and
  # `Ok(false)` from it means "this artifact has no entry in this registry" —
  # which is what the caller checks before CREATING one. Taking the mode off the
  # registry FILE cannot reach that branch: `stat` succeeds on an unreadable
  # file, so the swallow is skipped and the read below it refuses honestly. The
  # branch is only reachable with the DIRECTORY unreadable, and measured on
  # unmutated `579c7b3` with the hearth root at 0600 and a real `tracks.md`
  # naming the artifact, it answered `Ok(false)`.
  #
  # `0500` is a control for `registry_entry_exists` and not for the move: a
  # read-only hearth can still be read, and a move must still fail to WRITE.
  Scenario Outline: The "<port>" registry gate "<method>" over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                                      | unreadable      | mode | answer        |
      | snapshot | registry_entry_exists                       | the hearth root | 0755 | resolved      |
      | snapshot | registry_entry_exists                       | the hearth root | 0500 | resolved      |
      | snapshot | registry_entry_exists                       | the hearth root | 0600 | uninspectable |
      | snapshot | registry_entry_exists                       | the hearth root | 0000 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the hearth root | 0755 | resolved      |
      | snapshot | resolve_display_name_via_move_execution_row | the hearth root | 0500 | resolved      |
      | snapshot | resolve_display_name_via_move_execution_row | the hearth root | 0600 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the hearth root | 0000 | uninspectable |

  # ── H: the FOURTH address form — a non-legacy kind by RELATIVE PATH ──────
  #
  # Second pass of the audit. `fs_snapshot_adapter::kind_from_status_yaml` is the
  # resolver that OPENS a status.yaml, and it is reached only when
  # `kind_from_path` has answered `None` — which the six legacy kinds never do,
  # because their kind is carried by the directory name. So no row addressing a
  # track, by either form, ever executed its error arm: restoring
  # `locate_artifact_dir(..).unwrap_or(None)` there left the whole table GREEN.
  #
  # These rows address the knowledge artifact by its hearth-relative path, which
  # the snapshot lookup DOES accept (unlike its bare id — see table F), so they
  # reach the arm and pin it. Two address forms of one artifact disagreeing about
  # whether it exists is the shape round 7 was sent to find; a matrix that
  # carries only one of them cannot see it.
  Scenario Outline: The "<port>" port method "<method>" for a non-legacy kind by relative path over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "relative path, non-legacy kind"
    Then that port method answers "<answer>"

    Examples:
      | port     | method               | unreadable                    | mode | answer        |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0755 | resolved      |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0300 | resolved      |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0600 | uninspectable |
      | query    | read_artifact_kind   | the non-legacy kind directory | 0000 | uninspectable |
      | query    | read_artifact_kind   | the non-legacy status file    | 0644 | resolved      |
      | query    | read_artifact_kind   | the non-legacy status file    | 0200 | uninspectable |
      | query    | read_artifact_kind   | the non-legacy status file    | 0000 | uninspectable |
      | query    | read_artifact_state  | the non-legacy kind directory | 0755 | resolved      |
      | query    | read_artifact_state  | the non-legacy kind directory | 0300 | resolved      |
      | query    | read_artifact_state  | the non-legacy kind directory | 0600 | uninspectable |
      | query    | read_artifact_state  | the non-legacy kind directory | 0000 | uninspectable |
      | query    | read_artifact_state  | the non-legacy status file    | 0644 | resolved      |
      | query    | read_artifact_state  | the non-legacy status file    | 0200 | uninspectable |
      | query    | read_artifact_state  | the non-legacy status file    | 0000 | uninspectable |
      | query    | read_artifact_status | the non-legacy kind directory | 0755 | resolved      |
      | query    | read_artifact_status | the non-legacy kind directory | 0300 | resolved      |
      | query    | read_artifact_status | the non-legacy kind directory | 0600 | uninspectable |
      | query    | read_artifact_status | the non-legacy kind directory | 0000 | uninspectable |
      | query    | read_artifact_status | the non-legacy status file    | 0644 | resolved      |
      | query    | read_artifact_status | the non-legacy status file    | 0200 | uninspectable |
      | query    | read_artifact_status | the non-legacy status file    | 0000 | uninspectable |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0755 | resolved      |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0300 | resolved      |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0600 | uninspectable |
      | snapshot | read_artifact_kind   | the non-legacy kind directory | 0000 | uninspectable |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0644 | resolved      |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0200 | uninspectable |
      | snapshot | read_artifact_kind   | the non-legacy status file    | 0000 | uninspectable |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0755 | resolved      |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0300 | resolved      |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0600 | uninspectable |
      | snapshot | read_artifact_state  | the non-legacy kind directory | 0000 | uninspectable |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0644 | resolved      |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0200 | uninspectable |
      | snapshot | read_artifact_state  | the non-legacy status file    | 0000 | uninspectable |
      | snapshot | read_transitions     | the non-legacy kind directory | 0755 | resolved      |
      | snapshot | read_transitions     | the non-legacy kind directory | 0300 | resolved      |
      | snapshot | read_transitions     | the non-legacy kind directory | 0600 | uninspectable |
      | snapshot | read_transitions     | the non-legacy kind directory | 0000 | uninspectable |
      | snapshot | read_transitions     | the non-legacy status file    | 0644 | resolved      |
      | snapshot | read_transitions     | the non-legacy status file    | 0200 | uninspectable |
      | snapshot | read_transitions     | the non-legacy status file    | 0000 | uninspectable |

  # ── I: the per-file TRANSITION EVENT STORE ────────────────────────────────
  #
  # C-d.1 round 8. **Not one person named this node.** The syscall-coverage
  # instrument (`hearth_port_syscall_coverage.feature`) recorded every path the
  # ports open during the green control and failed on the ones no row varies;
  # `tracks/<id>/transitions` came out of a `read_dir` the code makes on every
  # single fold, in a fixture that never created the directory.
  #
  # That is why no AXIS could have reached it. An axis is a column and a column
  # varies a node; the fixture is the universe the columns range over. With the
  # directory absent, `read_event_files`'s `Err(_) => Vec::new()` arm executed in
  # ALL 249 cells — on a `NotFound`, where emptying is the CORRECT answer — and
  # passed 249 times. Gutting BOTH event readers to return empty unconditionally
  # left the whole matrix GREEN.
  #
  # The two events on disk ADVANCE the state past the legacy array's, so an
  # unreadable store is not a shorter history: it is a WRONG ANSWER. The control
  # resolves `reviewing`; the defect resolved `implementing`, confidently, with
  # no error, for an artifact that is `reviewing`.
  #
  # TWO DECLARED NON-UNIFORM ANSWERS in this table, both measured:
  #   `read_artifact_status` reports the STATUS FILE and not the fold (see
  #   `domain::status`, whose own doc names `resolve_state_with_events` as the
  #   authoritative state), so the event store's mode does not reach it;
  #   `list_artifacts` answers `(id, kind)` and reads `kind:` out of status.yaml,
  #   so it never opens the event store either. Both stay `resolved` at 0600 and
  #   both are TRIPWIRES: if either ever starts folding events, its cell reds and
  #   the declaration has to be redone. The round-7 reviewer read `list_artifacts`
  #   as folding the event store; it does not, and the cell is how we know.
  #
  # 0300 and 0500 are the discriminating controls here. At 0500 `read_dir`
  # succeeds AND the per-entry `stat` succeeds, so the fold must RESOLVE; at
  # 0300 `read_dir` itself fails; at 0600 `read_dir` succeeds and the per-entry
  # `stat` is what fails — the mode at which the fail-closed adoption reader was
  # measured answering `Ok(0 transitions)` over two events on disk.
  Scenario Outline: The "<port>" port method "<method>" over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                  | unreadable                | mode | answer        |
      | query    | read_transitions        | the transitions directory | 0755 | resolved      |
      | query    | read_transitions        | the transitions directory | 0500 | resolved      |
      | query    | read_transitions        | the transitions directory | 0300 | uninspectable |
      | query    | read_transitions        | the transitions directory | 0600 | uninspectable |
      | query    | read_transitions        | the transitions directory | 0000 | uninspectable |
      | query    | read_artifact_state     | the transitions directory | 0755 | resolved      |
      | query    | read_artifact_state     | the transitions directory | 0500 | resolved      |
      | query    | read_artifact_state     | the transitions directory | 0300 | uninspectable |
      | query    | read_artifact_state     | the transitions directory | 0600 | uninspectable |
      | query    | read_artifact_state     | the transitions directory | 0000 | uninspectable |
      | query    | read_artifact_status    | the transitions directory | 0755 | resolved      |
      | query    | read_artifact_status    | the transitions directory | 0600 | resolved      |
      | query    | read_transitions_strict | the transitions directory | 0755 | resolved      |
      | query    | read_transitions_strict | the transitions directory | 0500 | resolved      |
      | query    | read_transitions_strict | the transitions directory | 0300 | adoption evidence unreadable |
      | query    | read_transitions_strict | the transitions directory | 0600 | adoption evidence unreadable |
      | query    | read_transitions_strict | the transitions directory | 0000 | adoption evidence unreadable |
      | query    | list_artifacts          | the transitions directory | 0755 | resolved      |
      | query    | list_artifacts          | the transitions directory | 0600 | resolved      |
      | query    | find_artifact_by_kind_origin_turn | the transitions directory | 0755 | resolved |
      | query    | find_artifact_by_kind_origin_turn | the transitions directory | 0600 | uninspectable |
      | snapshot | read_transitions        | the transitions directory | 0755 | resolved      |
      | snapshot | read_transitions        | the transitions directory | 0500 | resolved      |
      | snapshot | read_transitions        | the transitions directory | 0300 | uninspectable |
      | snapshot | read_transitions        | the transitions directory | 0600 | uninspectable |
      | snapshot | read_transitions        | the transitions directory | 0000 | uninspectable |
      | snapshot | read_artifact_state     | the transitions directory | 0755 | resolved      |
      | snapshot | read_artifact_state     | the transitions directory | 0500 | resolved      |
      | snapshot | read_artifact_state     | the transitions directory | 0300 | uninspectable |
      | snapshot | read_artifact_state     | the transitions directory | 0600 | uninspectable |
      | snapshot | read_artifact_state     | the transitions directory | 0000 | uninspectable |

  # ── J: the per-ENTRY event file — a WRONG ANSWER, not a refusal ───────────
  #
  # Every directory readable, the NEWEST event file unreadable. The listing edge
  # succeeds and the read beneath it fails, so this isolates the per-entry
  # swallow from any refusal above it — and it is the only fixture in which the
  # defect's sharpest consequence is visible: the fold resolves the artifact to
  # the PREVIOUS event's state and reports it as fact.
  #
  # Measured on unmutated 63df2ff, both ports: `Ok(planning)` for an artifact
  # that is `reviewing`. Not a swallow that refuses, not a swallow that empties.
  Scenario Outline: The "<port>" port method "<method>" whose "<unreadable>" is at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                  | unreadable                        | mode | answer        |
      | query    | read_transitions        | the newest transition event file  | 0644 | resolved      |
      | query    | read_transitions        | the newest transition event file  | 0200 | uninspectable |
      | query    | read_transitions        | the newest transition event file  | 0000 | uninspectable |
      | query    | read_artifact_state     | the newest transition event file  | 0644 | resolved      |
      | query    | read_artifact_state     | the newest transition event file  | 0200 | uninspectable |
      | query    | read_artifact_state     | the newest transition event file  | 0000 | uninspectable |
      | query    | read_transitions_strict | the newest transition event file  | 0644 | resolved      |
      | query    | read_transitions_strict | the newest transition event file  | 0200 | adoption evidence unreadable |
      | query    | read_transitions_strict | the newest transition event file  | 0000 | adoption evidence unreadable |
      | snapshot | read_transitions        | the newest transition event file  | 0644 | resolved      |
      | snapshot | read_transitions        | the newest transition event file  | 0200 | uninspectable |
      | snapshot | read_transitions        | the newest transition event file  | 0000 | uninspectable |
      | snapshot | read_artifact_state     | the newest transition event file  | 0644 | resolved      |
      | snapshot | read_artifact_state     | the newest transition event file  | 0200 | uninspectable |
      | snapshot | read_artifact_state     | the newest transition event file  | 0000 | uninspectable |

  # ── K: the NON-LEGACY artifact's own directory and event store ────────────
  #
  # The third address form reaches the fold through `scan_subdirs_for_artifact`
  # rather than the per-kind loop. A node only one address form creates is a
  # fixture bound one address form down — which is round 7's own §40.4 finding
  # applied to the fixture instead of to the columns.
  Scenario Outline: The "<port>" port method "<method>" for a non-legacy kind over "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "bare id, non-legacy kind"
    Then that port method answers "<answer>"

    Examples:
      | port  | method              | unreadable                           | mode | answer        |
      | query | read_transitions    | the non-legacy transitions directory | 0755 | resolved      |
      | query | read_transitions    | the non-legacy transitions directory | 0300 | uninspectable |
      | query | read_transitions    | the non-legacy transitions directory | 0600 | uninspectable |
      | query | read_transitions    | the non-legacy transitions directory | 0000 | uninspectable |
      | query | read_artifact_state | the non-legacy transitions directory | 0755 | resolved      |
      | query | read_artifact_state | the non-legacy transitions directory | 0300 | uninspectable |
      | query | read_artifact_state | the non-legacy transitions directory | 0600 | uninspectable |
      | query | read_artifact_state | the non-legacy transitions directory | 0000 | uninspectable |
      | query | read_transitions    | the non-legacy transition event file | 0644 | resolved      |
      | query | read_transitions    | the non-legacy transition event file | 0200 | uninspectable |
      | query | read_transitions    | the non-legacy transition event file | 0000 | uninspectable |
      | query | read_artifact_state | the non-legacy transition event file | 0644 | resolved      |
      | query | read_artifact_state | the non-legacy transition event file | 0200 | uninspectable |
      | query | read_artifact_state | the non-legacy transition event file | 0000 | uninspectable |
      | query | read_artifact_kind  | the non-legacy artifact directory    | 0755 | resolved      |
      | query | read_artifact_kind  | the non-legacy artifact directory    | 0300 | resolved      |
      | query | read_artifact_kind  | the non-legacy artifact directory    | 0600 | uninspectable |
      | query | read_artifact_kind  | the non-legacy artifact directory    | 0000 | uninspectable |
      | query | read_artifact_state | the non-legacy artifact directory    | 0755 | resolved      |
      | query | read_artifact_state | the non-legacy artifact directory    | 0300 | resolved      |
      | query | read_artifact_state | the non-legacy artifact directory    | 0600 | uninspectable |
      | query | read_artifact_state | the non-legacy artifact directory    | 0000 | uninspectable |

  # ── L: the FILES each read-through method actually opens ──────────────────
  #
  # Every one of these came out of the syscall recorder, and not one of them was
  # varied by the 249. A method whose whole job is to return a file's contents,
  # with no row asserting what it does when that file cannot be read, is a
  # method whose failure behaviour nothing on this track had ever asked about.
  Scenario Outline: The "<port>" port method "<method>" whose "<unreadable>" is at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "relative path"
    Then that port method answers "<answer>"

    Examples:
      | port  | method                       | unreadable            | mode | answer        |
      | query | read_artifact_text           | the artifact text file | 0644 | resolved      |
      | query | read_artifact_text           | the artifact text file | 0200 | uninspectable |
      | query | read_artifact_text           | the artifact text file | 0000 | uninspectable |
      | query | read_carry_forward_if_present | the carry-forward file | 0644 | resolved      |
      | query | read_carry_forward_if_present | the carry-forward file | 0200 | uninspectable |
      | query | read_carry_forward_if_present | the carry-forward file | 0000 | uninspectable |
      | query | read_op_log                  | the op log file       | 0644 | resolved      |
      | query | read_op_log                  | the op log file       | 0200 | uninspectable |
      | query | read_op_log                  | the op log file       | 0000 | uninspectable |
      | query | read_playbook_hook_body      | the hook file         | 0644 | resolved      |
      | query | read_playbook_hook_body      | the hook file         | 0200 | uninspectable |
      | query | read_playbook_hook_body      | the hook file         | 0000 | uninspectable |
      | query | read_context_file            | the context file      | 0644 | resolved      |
      | query | read_context_file            | the context file      | 0200 | uninspectable |
      | query | read_context_file            | the context file      | 0000 | uninspectable |

  # ── M: the DIRECTORIES those reads traverse ───────────────────────────────
  Scenario Outline: The "<port>" port method "<method>" traversing "<unreadable>" at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "relative path"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                                    | unreadable                | mode | answer        |
      | query    | read_playbook_hook_body                   | the playbooks directory   | 0755 | resolved      |
      | query    | read_playbook_hook_body                   | the playbooks directory   | 0300 | resolved      |
      | query    | read_playbook_hook_body                   | the playbooks directory   | 0600 | uninspectable |
      | query    | read_playbook_hook_body                   | the playbooks directory   | 0000 | uninspectable |
      | query    | read_playbook_hook_body                   | the playbook directory    | 0755 | resolved      |
      | query    | read_playbook_hook_body                   | the playbook directory    | 0300 | resolved      |
      | query    | read_playbook_hook_body                   | the playbook directory    | 0600 | uninspectable |
      | query    | read_playbook_hook_body                   | the playbook directory    | 0000 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the projections directory | 0755 | resolved    |
      | snapshot | resolve_display_name_via_move_execution_row | the projections directory | 0300 | resolved    |
      | snapshot | resolve_display_name_via_move_execution_row | the projections directory | 0600 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the projections directory | 0000 | uninspectable |
      | snapshot | rebuild_sparks_projection                 | the sparks directory      | 0755 | resolved      |
      | snapshot | rebuild_sparks_projection                 | the sparks directory      | 0300 | resolved      |
      | snapshot | rebuild_sparks_projection                 | the sparks directory      | 0600 | uninspectable |
      | snapshot | rebuild_sparks_projection                 | the sparks directory      | 0000 | uninspectable |

  # ── N: the PROJECTION FILES the rebuilds read before they write ───────────
  #
  # The destructive half of the class, at the node the instrument found. Round 7
  # closed the seven read-then-write sites and verified them behaviourally — but
  # against a fixture with no projection on disk, so the branch that reads an
  # EXISTING projection was never once exercised by the 249 cells over the very
  # methods whose risk is reading a projection as empty and writing the empty
  # result over the real one.
  Scenario Outline: The "<port>" read-then-write method "<method>" whose "<unreadable>" is at mode "<mode>" answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" at mode "<mode>"
    When the "<port>" port method "<method>" is asked for the artifact by "relative path"
    Then that port method answers "<answer>"

    Examples:
      | port     | method                                      | unreadable              | mode | answer        |
      | snapshot | resolve_display_name_via_move_execution_row | the execution projection | 0644 | resolved      |
      | snapshot | resolve_display_name_via_move_execution_row | the execution projection | 0200 | uninspectable |
      | snapshot | resolve_display_name_via_move_execution_row | the execution projection | 0000 | uninspectable |
      | snapshot | rebuild_decisions_projection                | the decisions projection | 0644 | resolved      |
      | snapshot | rebuild_decisions_projection                | the decisions projection | 0200 | uninspectable |
      | snapshot | rebuild_decisions_projection                | the decisions projection | 0000 | uninspectable |
      | snapshot | rebuild_sparks_projection                   | the sparks projection    | 0644 | resolved      |
      | snapshot | rebuild_sparks_projection                   | the sparks projection    | 0200 | uninspectable |
      | snapshot | rebuild_sparks_projection                   | the sparks projection    | 0000 | uninspectable |

  # ── O: ELOOP — the failure lever that is NOT a mode ───────────────────────
  #
  # C-d.1 round 8, M-2. Round 7 declared MUT-Q1, MUT-Q7 and MUT-S1 MASKED, on
  # the argument that `stat` of `<hearth>/<kind>/<id>` can only fail when
  # `<kind>` itself is unreadable and that same mode makes the scan below refuse.
  # The argument is scoped to MODE as the only failure lever. The round-7
  # reviewer falsified it in four lines: a symlink loop is deterministic, needs
  # no chmod, and `fs_probe`'s own header names ELOOP among the errors it exists
  # to refuse on. Measured: unmutated `IoError`, MUT-Q1 alone `NotFound`.
  #
  # `IoError -> NotFound` is the defect itself — gRPC INTERNAL becomes
  # gRPC NOT_FOUND and an operator is told an artifact on disk is not there.
  #
  # The masked verdicts were unfalsifiable BY THE SHAPE OF THE FIXTURE, which is
  # the missing-node disease one level up: in the AXIS rather than the subject.
  # These rows make MUT-Q1's red a cell.
  Scenario Outline: The "<port>" port method "<method>" over "<unreadable>" replaced by a symlink loop answers "<answer>"
    Given a hearth holding a track and a knowledge artifact with "<unreadable>" replaced by a symlink loop
    When the "<port>" port method "<method>" is asked for the artifact by "<address>"
    Then that port method answers "<answer>"

    Examples:
      | port     | method               | unreadable                        | address                       | answer        |
      | query    | read_artifact_state  | the artifact directory            | relative path                 | uninspectable |
      | query    | read_artifact_status | the artifact directory            | relative path                 | uninspectable |
      | query    | read_transitions     | the artifact directory            | relative path                 | uninspectable |
      | query    | read_artifact_kind   | the artifact directory            | relative path                 | resolved      |
      | query    | read_artifact_state  | the artifact directory            | bare id                       | uninspectable |
      | query    | read_transitions     | the artifact directory            | bare id                       | uninspectable |
      | query    | read_artifact_state  | the non-legacy artifact directory | bare id, non-legacy kind      | uninspectable |
      | query    | read_artifact_state  | the non-legacy artifact directory | relative path, non-legacy kind | uninspectable |
      | query    | list_artifacts       | the artifact directory            | bare id                       | uninspectable |
      | snapshot | read_artifact_state  | the artifact directory            | relative path                 | uninspectable |
      | snapshot | read_transitions     | the artifact directory            | relative path                 | uninspectable |
      | query    | read_artifact_state  | the transitions directory         | bare id                       | uninspectable |
      | query    | read_transitions     | the transitions directory         | bare id                       | uninspectable |
      | query    | read_transitions_strict | the transitions directory      | bare id                       | adoption evidence unreadable |
