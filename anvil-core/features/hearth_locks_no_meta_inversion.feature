Feature: HearthLocks releases the meta-mutex before holding a per-hearth lock
  The meta-mutex guarding the lazy-init map is acquired ONLY to look up / insert
  the per-hearth lock and is released before the per-hearth lock is held (spec
  Req 5 lock-ordering, plan N2). There must be no nested-lock inversion: while a
  writer holds hearth X's lock (parked), a fresh lock_for on hearth Y still
  completes — proving the meta-mutex is not held across the per-hearth critical
  section.

  Scenario: A fresh lock_for(Y) completes while a writer parks holding X's lock
    Given a HearthLocks primitive
    When a writer parks holding hearth X's lock and a fresh lock_for on hearth Y is requested
    Then the second writer completes while the first is still parked
