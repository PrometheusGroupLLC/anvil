Feature: The hearth-root guard path may not contain a bool-returning filesystem predicate

  # C-d.1 round 5. This gate exists because the SAME defect was closed twice at
  # the instance and came back at the next spelling:
  #
  #   round 3 — `list_dirs` swallowed `read_dir`'s Err into `Vec::new()`
  #   round 4 — fixed THAT syscall, published a three-row contract, and shipped
  #             code implementing two of the three rows; three per-ENTRY `stat`
  #             swallows were still standing behind the fixed `read_dir`
  #   round 5 — this
  #
  # `Path::is_dir()`, `Path::is_file()` and `Path::exists()` return `bool` and
  # map EVERY error to `false`. On the hearth-root guard path `false` reads as
  # "there is nothing here", which is the one answer on which every guard
  # concludes nothing would be lost and CLEARS A WRITER. Patching call sites
  # leaves the next call site free to reintroduce it — which is exactly what
  # happened, three times.
  #
  # C-d.1 ROUND 6 CORRECTION — READ THIS BEFORE TRUSTING THIS FILE.
  #
  # Round 5 shipped this gate claiming it made the swallow UNREPRESENTABLE. It
  # does not. It is `line.contains(needle)` over the non-comment lines of two
  # hand-named files, and an independent review wrote the same swallow, at the
  # same site, with the same behaviour, TEN other ways — UFCS, a type alias, the
  # receiver and the method on DIFFERENT SOURCE LINES, a `matches!` macro,
  # `.ok().is_some()`, `.is_err()`, `.unwrap_or_default()`, a `-> bool` helper in
  # a third module, a `pub(crate) -> bool` helper inside `fs_probe` itself, and
  # the split-line trick at a real site. All ten compiled and all ten left this
  # file GREEN. One differs from a caught spelling only by where a newline falls.
  #
  # WHAT THIS FILE IS: a tripwire for the three spellings this defect has
  # actually shipped as. It catches a developer who writes it back the way it was
  # written before. It catches nobody who writes it any other way, and no
  # substring scan would.
  #
  # WHAT ACTUALLY CLOSES THE CLASS is structural and is not here. The hearth-root
  # DECISION (`registry_projection::decide_hearth_roots`) now receives an owned,
  # path-free `HearthRootFacts` produced by one edge. The loader's only
  # filesystem access likewise moved to `fs_probe`, leaving that module with no
  # path type in scope at all. See `HearthRootFacts`.
  #
  # ROUND 7, M-1 — THIS COMMENT USED TO SAY "all ten bypasses FAIL TO COMPILE
  # there rather than going undetected", AND THAT IS FALSE. The round-6 reviewer
  # falsified it by construction: `std::path` and `std::fs` are reachable by
  # absolute path from every module in Rust and `String` is `AsRef<Path>`, so
  # ATK-1, ATK-2 and ATK-3 written INSIDE `decide_hearth_roots` — with the
  # binding created on the line above — compile, leave this file GREEN, and leave
  # the projection suite GREEN. The three mutations round 6 reported as "fails to
  # compile" failed on a BINDING NAME (`hearth`, `legacy`, `p` — identifiers that
  # had been removed), not on the bypass being unavailable. Correcting round 5's
  # overclaim introduced a smaller one, and it shipped in this file.
  #
  # THE PROPERTY THAT IS TRUE, AND IT IS WORTH HAVING: **the decision holds no
  # path naming the hearth**, because `HearthRootFacts` carries only `Vec<String>`
  # basenames and unit variants. A predicate written in the decision can only
  # probe a path the mutation constructs out of thin air, which names a place
  # rather than THIS hearth's roots. To make the decision answer wrongly about
  # this hearth you must also change the edge or the type. That is narrower than
  # "unwritable" and it is real.
  #
  # SUBJECT LIST: two files. The guard path is wider — see `GUARD_MODULES` for
  # the two modules it does not scan and what was done about them instead.
  #
  # `fs_probe` is the one module allowed to call the raw predicates, because it
  # is the module that converts them into a `Result`. It is held to that by the
  # second scenario: no NON-PRIVATE function in it may return a bare `bool`, so
  # it cannot hand a swallowed answer back out. Round 5 wrote that check to match
  # only lines beginning `pub fn `, so `pub(crate) fn .. -> bool` and any
  # two-line signature passed it; round 6 repaired it.

  Scenario: No guard module names a bool-returning filesystem predicate
    Given the hearth-root guard modules
    Then every guard module is present and non-empty
    And no guard module calls a bool-returning filesystem predicate

  Scenario: The fallible probe cannot hand a swallowed answer back out
    Given the fallible filesystem probe module
    Then every guard module is present and non-empty
    And no non-private function in the probe returns a bare bool
