Feature: The coverage criterion is derived from the code's syscalls, not from anyone's list

  # ── C-d.1 round 8: the instrument that ends the pattern ──────────────────
  #
  # Seven rounds closed this class against an enumeration and seven rounds were
  # defeated the SAME way — by a noun nobody wrote down.
  #
  #   rounds 3-6  a MODULE a list did not name (`fs_query_adapter.rs`: the
  #               QueryPort the engine constructs eleven times, carrying all four
  #               historical spellings, in no section of the record)
  #   round 7     an ADDRESS FORM a matrix did not carry — the matrix's own
  #               self-audit found this, and it is the best moment on the track
  #   round 7     a filesystem NODE the fixture never created
  #
  # The third one is why this feature exists. `hearth_port_reachability.feature`
  # is a real criterion: 249 cells, enumerated from the port TRAITS, every answer
  # compared against a declared literal, every declared answer carrying a
  # measured red. It still could not see `transitions/`, because five of its own
  # methods on both of its own ports fold
  # `domain::transition_log::read_event_files`, that function `read_dir`s
  # `<artifact_dir>/transitions/` and answers `Vec::new()` on ANY error, and the
  # fixture writes history as a legacy array inside `status.yaml` and never
  # creates the directory at all.
  #
  # So the swallowing arm was not merely unexercised. It EXECUTED in all 249
  # cells, on a NotFound where emptying is correct, and passed 249 times.
  # Gutting BOTH event readers to return empty unconditionally left the matrix
  # 249/249 GREEN — while the same mutation reds 21 scenarios elsewhere.
  #
  # An axis is a column. A FIXTURE is the universe the columns range over. No
  # column can vary a node that is not there.
  #
  # ── What this asserts, and why it cannot inherit the disease ─────────────
  #
  # Every path the ports actually LOOK AT — on the readable control AND on every
  # degraded fixture the matrix builds — must be a path some matrix row takes the
  # GOVERNING modes off.
  #
  #   the left side  — recorded by interposing stat/lstat/open/opendir/access in
  #                    a child process (`anvil-test-support/fstrace/fstrace.c`),
  #                    so it is the code's own syscalls and not a reading of them
  #   the right side — parsed out of `hearth_port_reachability.feature`, so a
  #                    subject counts as covered only when rows on disk vary it;
  #                    "declare it covered" is not available without writing them
  #
  # It records LOOKUPS, not successful reads. A `stat` of a path that is not
  # there is still a dependency on that path, and `transitions/` is precisely
  # that shape. An instrument that recorded only successful opens would rebuild
  # the hole it exists to close.
  #
  # ── C-d.1 round 9: the three misses the round-8 reviewer constructed ─────
  #
  # An instrument is itself a criterion, and a criterion that has not been shown
  # a defect it MISSES has not been characterised. Round 8 shipped two mutations
  # this instrument CATCHES and none it misses. The reviewer found three in an
  # afternoon by asking the instrument's own question about the instrument —
  # what does this predicate EXCLUDE? — and all three are closed here.
  #
  #   M-2  it recorded only the READABLE CONTROL, so a swallow behind
  #        `if <a readable file>.is_err()` was invisible; measured GREEN. The
  #        degraded path is this class's home address — every fallback and every
  #        recovery branch runs precisely when something is already unreadable.
  #        The probe now rebuilds one fixture per distinct (subject, mode) the
  #        matrix carries, through the MATRIX'S OWN degradation helper, and the
  #        tapes are unioned.
  #
  #   M-1  coverage was per-NODE, not per-(node x mode). Deleting the eight
  #        `the transitions directory | 0600` rows left the instrument GREEN and
  #        left round 7's H-2 — an adoption reset clobbering a governed artifact
  #        — pinned by nothing. A directory now needs 0300, 0600 and 0000; a file
  #        needs 0200 and 0000. Those are distinct POSIX failure sites and the
  #        code answers differently at each.
  #
  #   M-3  the anti-vacuity guard was calibrated to catch a DEAD recorder, not a
  #        partly deaf one: a tape missing an entire op class still cleared a
  #        count of nodes. Every op class the ports use must now be ON the tape.
  #
  # ── Its own falsifiability ───────────────────────────────────────────────
  #
  # The check refuses the vacuous shapes before it compares anything: a recorder
  # that wrote no trace, a trace with nothing inside the fixtures, a tape missing
  # an op class, a lookup silently dropped by the prefix filter, fewer than five
  # governing nodes, and a matrix that varies no node at all. Each of those would
  # satisfy a naive subset test, and a check that cannot fail is how this class
  # survived seven rounds.

  Scenario: No port method reads a node the matrix leaves unvaried, on any arm
    Given the fixtures the port-reachability matrix builds — the readable control and every degraded arm
    When every port method the matrix exercises is invoked under a syscall recorder
    Then every path they looked at is a path the matrix varies
