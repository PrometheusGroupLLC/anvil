Feature: Crate size cannot silently grow into a build-memory hazard

  rustc compiles a crate as ONE unit — the frontend holds the whole crate's IR in memory
  at once — so peak build memory tracks the LARGEST CRATE, not the workspace. A crate
  that grows past the machine's tolerance stops being a code-organisation preference and
  becomes an operational hazard.

  Measured across the fleet on 2026-07-30: accumulate-test-support reached 169,616 lines
  and OOM'd this machine four times in one day at 20-30 GB of rustc, froze every
  application, killed the OrbStack VM, and took Concourse and its Cloudflare tunnel down
  with it. anvil-test-support is on the same curve at 83,812.

  Nothing announced the crossing. accumulate-test-support tripled in a single month, one
  track at a time, and the first signal anyone received was the machine freezing two
  months later. This guard exists so the next crossing is loud while it is still cheap.

  It is a RATCHET, not a cap: a crate may not grow past its recorded baseline, shrinking
  lowers that baseline permanently, and raising one takes an explicit committed decision.
  A flat cap set where it belongs would be red on arrival for crates already over it, and
  a gate that fails the day it lands gets bypassed rather than obeyed.

  Scenario: no crate has grown beyond its recorded baseline
    Given the workspace crate size baseline
    When the crate sizes are measured
    Then no crate exceeds its baseline

  Scenario: the ratchet actually fails when a crate grows
    Given the workspace crate size baseline
    When a crate is measured against a baseline lowered below its current size
    Then the ratchet reports a violation
