# Contributing to Anvil

Thank you for looking at this. Before you spend time on a patch, please read
the two sections below — they describe how this repository actually works and
what currently does not work. Both are unusual enough that finding out after
you have cloned would waste your time.

---

## 1. This repository is a mirror

**Development does not happen here.** Anvil is developed in a private
repository. This public repository receives **curated exports** from it. There
is no automated two-way sync.

What that means in practice:

- The git history here is **not** the upstream history. Do not rely on commit
  SHAs, tags, or branch structure in this repository matching anything
  upstream.
- Between exports, this repository is **behind** the private one. Something you
  see missing may already exist upstream.
- Maintainers may not be able to merge your pull request with the merge button,
  because the change has to land upstream first.

### How your contribution actually lands

1. You fork `PrometheusGroupLLC/anvil` and open a pull request here.
2. A maintainer reviews it in the pull request. Discussion happens here, in the
   open.
3. If it is accepted, the maintainer **applies your patch into the private
   repository**, re-committing it with:

   ```
   git commit --author="Your Name <your@email>" --signoff
   ```

   so that **your authorship is preserved** on the commit that actually ships.
   Your name, not the maintainer's, is the commit author.
4. Your pull request is **closed, not merged**, with a comment referencing the
   upstream change. When the next export happens, your commit appears in this
   repository's history authored by you.

A closed pull request here is not a rejection. If it was rejected, we will say
so plainly in the review.

### Cadence — what to actually expect

We would rather under-promise:

- **First response to a pull request:** we aim for **within two weeks**. If a
  PR has had no response after that, please comment on it — it was missed.
- **Exports to this repository:** currently **irregular, roughly monthly**.
  This is best-effort. There is no published schedule and no guarantee.
- The gap between "your PR is accepted" and "your commit is visible here" can
  therefore be **several weeks**. That is expected, not a problem.

If this cadence does not work for you, please open an issue to discuss the
change before writing it, rather than investing in a large patch.

### Why there is no CI

This repository deliberately contains **no GitHub Actions workflows**. That is
a structural security decision, not an oversight: with no workflows present, a
pull request from a fork has nothing to execute. Please do not add workflow
files in a pull request — they will be removed.

The consequence is that **no automated checks will run on your PR**. Review is
done by a human, and validation is done upstream.

---

## 2. The hard part: you probably cannot build this yet

**Please read this before cloning.** As of this export, an outside contributor
**cannot build this repository and cannot run the test suite.** This is not a
misconfiguration on your machine.

The workspace declares path dependencies on **private sibling repositories that
are not published**:

- `anvil-test-support` — a workspace member — depends on `brine-core` and
  `brine-runner-rust` at `../../brine/…`. **Brine is the Gherkin test runner
  that executes every `.feature` file in this repository, and it is not
  currently public.**
- `anvil-engine` has an optional dependency on `foundry-kit-broker-client` at
  `../../foundry/…`.

Two other support crates that used to be pulled from a private repository —
`foundry-engine-addressing` and `foundry-kit-telemetry` — are now vendored in
source form under [`vendor/`](vendor/) and need nothing external.

Because `anvil-test-support` is a workspace member and its `brine` dependencies
are **not optional**, Cargo fails during manifest resolution — before any
compilation happens. `cargo build`, `cargo check`, and even
`cargo metadata` all fail with:

```
error: failed to load manifest for workspace member `…/anvil-core`
Caused by: failed to load manifest for dependency `anvil-test-support`
Caused by: failed to load manifest for dependency `brine-core`
Caused by: failed to read `…/brine/core/Cargo.toml`
Caused by: No such file or directory (os error 2)
```

So this is worse than "the tests do not run" — **nothing builds at all.**

### What is being done about it

This is a known, tracked defect in the public release, and it is the single
biggest barrier to outside contribution. The intended fixes, in rough order:

1. **Decouple the library crates from the test runner** so that
   `cargo build -p anvil-core`, `-p anvil-engine`, and `-p anvil-mcp` succeed
   with no sibling repositories present. The `brine` dependencies belong behind
   an optional feature or in a separate workspace, not in the default graph.
2. **Make the `foundry` broker dependency genuinely optional**, so it is absent
   from the default resolution graph.
3. **Publish or vendor the test runner** so that the `.feature` suite — which is
   the project's central correctness claim — can be executed by anyone.

No date is committed to any of these. Until at least item 1 lands, treat this
repository as **readable but not buildable**.

### What you can usefully do in the meantime

- **Read the `.feature` files.** They are the specification. Every behavioural
  claim anvil makes is written as a Gherkin scenario under each crate's
  `features/` directory. Reviewing them for gaps, ambiguity, or wrong behaviour
  is genuinely valuable and requires no build.
- **Report bugs and design problems** as issues.
- **Documentation and prose fixes** need no toolchain.
- **Small, self-evidently-correct source changes** can be reviewed by eye and
  validated upstream. Say in the PR that you were unable to build; we will not
  hold it against you.

Please do not open a pull request whose description claims tests pass unless
you actually ran them.

---

## 3. Developer Certificate of Origin (DCO)

All contributions to this project must be signed off under the **Developer
Certificate of Origin, version 1.1**. The full text is in
[`dco.txt`](dco.txt), reproduced verbatim from
<https://developercertificate.org/>.

### Why a DCO and not a CLA

Anvil is released under the Apache License, Version 2.0. Distributing the
project under that licence — and any future licensing decision — requires the
project to hold clear rights in every contribution. Without that, a single
contribution of uncertain provenance can encumber the whole codebase.

We use the DCO rather than a Contributor License Agreement deliberately. A CLA
means paperwork, an identity check, and often a corporate signature before a
one-line typo fix can be accepted. The DCO is a lightweight, well-understood,
industry-standard assertion that you have the right to submit what you are
submitting — and it is sufficient for our needs here. Lower friction, adequate
rights.

Note that the DCO is an assertion about *provenance and rights*, not a
copyright assignment. **You retain the copyright in your contribution.**

### How to sign off

Add a `Signed-off-by` line to every commit, matching the name and email you are
contributing under:

```
Signed-off-by: Jane Developer <jane@example.com>
```

Git will add it for you:

```bash
git commit --signoff -m "fix(engine): …"
```

To sign off a series of commits you already wrote:

```bash
git rebase --signoff main
```

Pull requests containing commits without a valid `Signed-off-by` line cannot be
accepted. Because there is no CI here, this is checked by hand at review time —
please save us both a round trip and sign off as you go. Anonymous or
pseudonymous contributions are fine as long as the sign-off is consistent and
you can stand behind the certification in `dco.txt`.

---

## 4. Making a good change

### Behaviour is specified in Gherkin, first

This project has a hard rule, and it is not negotiable for behavioural changes:

> **Nothing with behaviour is built without `.feature` files binding intent to
> execution.**

Tests are `.feature` files run through the Gherkin runner. There are no raw
unit tests for behavioural coverage. Each crate's `features/` directory tests
that crate from the perspective of *its* consumer:

- `anvil-mcp/features/` — the user is the coding agent.
- `anvil-engine/features/` — the user is the MCP shim.
- `anvil-core/features/` — the user is the engine.

If your change alters behaviour, it should add or modify a scenario, and the
scenario should be written from the user's perspective at that seam — no
implementation details, no internal type names.

### Style

- Prefer declarative over imperative, immutable over mutable.
- Follow the conventions already visible in the surrounding code rather than
  introducing a new style.
- Keep the change focused. One concern per pull request.

### Commit messages

Conventional commits: `<type>(<scope>): <description>`, for example
`fix(engine): reject a hearth path outside the permitted roots`.

---

## 5. Reporting bugs and security issues

- **Bugs and feature requests:** open an issue at
  <https://github.com/PrometheusGroupLLC/anvil/issues>. Include the commit you
  are on and what you expected versus what happened.
- **Security vulnerabilities:** do **not** open a public issue. Follow
  [SECURITY.md](SECURITY.md).

---

## 6. Licensing of your contribution

By contributing, you agree that your contribution is licensed under the same
terms as the project — the Apache License, Version 2.0 (see
[LICENSE](LICENSE)). Your `Signed-off-by` line is your certification of the
rights described in [`dco.txt`](dco.txt).
