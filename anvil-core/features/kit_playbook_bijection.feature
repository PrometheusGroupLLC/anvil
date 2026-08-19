Feature: Canonical playbooks survive packaging

  C1's packaging gate (spec.md:1017-1027). After packaging, the build asserts a
  four-set bijection:

      source dirs with machine.yaml
        = staged playbooks.definitions paths
        = staged dirs with machine.yaml
        = staged dirs with canonical legal status.yaml

  The sets are DERIVED FROM THAT BUILD. No numeric constant and no checked-in
  template population is an oracle — `kit/foundry-manifest.json`'s own definition
  block is template INPUT that the build overwrites from the source machines, and
  it is deliberately not kept in sync with the source tree.

  The staging runs through `scripts/stage-kit-content.sh` +
  `scripts/declare-kit-playbooks.sh` + `scripts/assert-kit-playbook-bijection.py`
  — the same three scripts `scripts/build-kit.sh` invokes, in the same order.
  Running the whole of build-kit.sh here is not viable: it cross-compiles three
  Rust targets and runs `npm ci` against kit/app/frontend, mutating shared build
  state on every suite run. The rules this feature depends on live in the three
  delegated scripts precisely so this seam cannot drift from the published kit.

  BEHAVIORAL MATRIX closed here: source directory shape x gate verdict —
  (machine + canonical status) / (machine, no canonical status) / (no machine).
  DECLARED EXCLUSION: the binary assembly, cross-compilation, frontend build and
  enforcement-load gate of build-kit.sh are out of this seam's scope; they carry
  their own assertions inside that script and are exercised by a real build.

  Scenario: The Anvil package derives its complete playbook set
    Given an anvil-kit staged through the production packaging scripts
    Then every machine-backed source is declared exactly once in the staged manifest
    And every declared staged definition has a canonical legal playbook status
    And no declared path is missing from the staged package

  # What the packaging predicate EXCLUDES, asserted rather than assumed. A
  # `playbooks/<id>/` directory with no machine.yaml is not staged, not declared
  # and not registered. This is not hypothetical: `20260528T2321_workflow_generation`
  # is such a directory in this repo today.
  Scenario: A source directory without a machine is excluded from the package
    Given an anvil-kit staged through the production packaging scripts
    Then a source playbook directory with no machine is absent from the staged manifest
    And the excluded directory is still present in the source tree

  # The one non-derived datum in the gate is the playbook kind's own lifecycle.
  # It is mirrored into the asserter in Python, so it is pinned against the
  # compiled seed here: adding a state to one side without the other reds.
  Scenario: The gate's legal state list is the playbook seed's own lifecycle
    Then the packaging gate's legal playbook states equal the playbook seed's states
