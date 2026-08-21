# Contributing to Anvil

Thank you for looking at this. Before you spend time on a patch, please read
the two sections below — they describe how this repository actually works and
what it can and cannot do. Both are unusual enough that finding out after you
have cloned would waste your time.

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
   upstream change and linking the commit that landed it.

A closed pull request here is not a rejection. If it was rejected, we will say
so plainly in the review.

#### What attribution you actually get today — and what you do not

This document previously promised that "your commit appears in this
repository's history authored by you." **That is not true, and we would rather
say so than let you find out.** Every export lands as a single batch commit
whose author and committer are both pinned to the publisher, so an individual
contributor's authorship does not survive the export boundary. Concretely, if
your change ships today you get:

- your change, in the code, in the next export;
- a maintainer's comment linking the landing commit;
- the "proposed a pull request" event on your profile, which you get for
  opening the pull request regardless of outcome.

You do **not** get a Merged badge on your pull request, an entry in this
repository's contributor graph, or `git blame` credit.

We consider that a defect rather than a policy, and intend to fix it — git
carries author and committer as two separate fields, and the export currently
pins both when it only needs to pin the committer. Until that ships, treat the
list above as the whole of what is on offer, and weigh a large patch
accordingly.

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

**GitHub Actions is disabled on this repository**, at the repository level. No
workflow can run here — not ours, and not one that arrives in a pull request.

This document used to justify that differently, and the justification was
wrong. It said that because the repository ships no workflow files, a fork's
pull request has "nothing to execute." That inference does not hold. Under the
`pull_request` trigger GitHub builds a merge ref from the base branch and the
pull request's head, and it executes the workflow definition found *there* — so
a pull request that adds `.github/workflows/…` supplies the workflow itself.
Shipping zero workflow files never prevented execution. It is the disabled
setting that does, and the setting is checkable where the absence of a file was
only reassuring.

We still ship no workflow files, and a pull request that adds one will have it
removed — but that is housekeeping now, not the protection.

The consequence for you is unchanged and worth stating plainly: **no automated
check will ever run on your pull request here.** Nothing on GitHub can produce
a green tick on this repository. Review is done by a human, and validation is
done upstream, against the private repository where the full suite runs.

---

## 2. Building, and the one suite you cannot run

**This repository builds from a bare clone.** Clone it, install `protoc`, and
`cargo test --workspace --locked` goes green. Nothing else — no private
sibling repository, no credential, no vendored blob — is required.

> **This section used to say the opposite.** Until 2026-08-20 it stated that an
> outside contributor could not build the repository at all, because the
> workspace pulled path dependencies from private siblings and Cargo failed
> during manifest resolution. That was true at the initial public release and
> is no longer true: the test-runner crates were moved out of the default
> workspace, and the private broker dependency was removed. The description
> below was re-measured against the current tree rather than edited in place.

### Prerequisites

Two, and the second one is easy to miss:

1. **A Rust toolchain.** Anything current; the measurement below used
   1.97.1.
2. **`protoc`, the Protocol Buffers compiler.** This is a **hard**
   prerequisite, not an optional extra. `anvil-engine/build.rs` compiles
   `proto/anvil.proto` through `tonic-build`, which shells out to `protoc`.
   Without it the build dies inside a build script and cargo exits 101:

   ```
   error: failed to run custom build command for `anvil-engine`
     Could not find `protoc`. If `protoc` is installed, try setting the
     `PROTOC` environment variable to the path of the `protoc` binary.
   ```

   Install it with `brew install protobuf` on macOS, or
   `apt install protobuf-compiler` on Debian/Ubuntu.

### Build and test

```bash
git clone https://github.com/PrometheusGroupLLC/anvil.git
cd anvil
cargo test --workspace --locked
```

### What was actually measured

Against commit `4d7890e2` of this repository, on a clone in a directory with no
private sibling repositories anywhere above it:

- `cargo metadata --no-deps` — exits 0. The workspace enumerates.
- `cargo test --workspace --locked` — **exits 0**, 104 tests passed across 18
  test binaries, 0 failed.
- With `protoc` removed from `PATH` and nothing else changed,
  `cargo check -p anvil-engine` — **exits 101** with the error quoted above.
  That is the whole reason `protoc` is called out as a prerequisite.

### The workspace

Exactly four members:

| Crate | What it is |
| --- | --- |
| `anvil-core` | Library: ports, domain types, playbook loader, hearth reader |
| `anvil-core-hearth` | Hearth storage adapters and maintenance binaries |
| `anvil-engine` | The long-running engine process, exposing a gRPC API |
| `anvil-mcp` | An MCP shim (JSON-RPC over stdio) that fronts the engine for agents |

`proto/anvil.proto` defines the gRPC service.

### What you cannot run: the `.feature` suite

This is the real limitation, and it is narrower than "nothing builds".

The 504 `.feature` files in this repository are anvil's specification — 260
under `anvil-core/features/`, 166 under `anvil-engine/features/`, 78 under
`anvil-mcp/features/`. **They all ship here. None of them execute here.**

`cargo test --workspace` does not run them. The Gherkin runner they bind to
(Brine) is not public, and the step-definition crates that bind each scenario to
real execution are not part of this export. So the 104 tests reported above are
the Rust unit and integration tests — they are not the behavioural suite, and
you should not read a green `cargo test` as "the specification passes."

Publishing the runner, so that the suite the project's correctness claim rests
on can be executed by anyone, is still an open intention. No date is committed
to it.

### What you can usefully do in the meantime

- **Read the `.feature` files.** They are the specification. Every behavioural
  claim anvil makes is written as a Gherkin scenario under each crate's
  `features/` directory. Reviewing them for gaps, ambiguity, or wrong behaviour
  is genuinely valuable.
- **Report bugs and design problems** as issues.
- **Documentation and prose fixes** need no toolchain.
- **Source changes** can be built and unit-tested with the commands above; say
  in the PR that you could not run the `.feature` suite, which nobody outside
  the project can. We will not hold it against you.

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

You will not be able to execute the scenario you write — see [what you cannot
run](#what-you-cannot-run-the-feature-suite). Write it anyway; a reviewer runs
it upstream. A behavioural change that arrives without a scenario is incomplete
even though no check here will say so.

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
