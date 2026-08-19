# Change Record Schema (`refs/anvil/*`, `change-record.jsonl`)

The engine records each write transaction as a git commit on a dedicated,
anvil-owned ref lineage inside the hearth's own repository, and records the
outcome of that attempt as a row in the `change-record.jsonl` durable sink.

This file is the **consumed contract**: T12 (projections), T13 (check-in and
status served from derived history), T14 (the solidification term) and T17
(structural diffs) read the ref layout, the commit-message shape and the row
allowlists declared here. Changing anything below changes those readers.

Git means git the technology — the commit graph, diffs and the
content-addressed object store, on the user's own disk. **No remote is read or
written at any point, in any stage.** A hearth with no remote configured
behaves identically to one that has one.

---

## Ref layout

Two refs per repository, both **outside `refs/heads/`** so nothing this
mechanism does can be mistaken for a branch.

| Ref | Meaning |
|---|---|
| `refs/anvil/change-record` | The moving tip of the engine-authored lineage. |
| `refs/anvil/baseline` | The immutable first commit of that lineage. **The existence of this ref is the `has a baseline` predicate** consumed by the `no_baseline` outcome and by the authority flip's refusal (d). |

Consequence, stated because it is a real trade and not an oversight: `git log`
on the human's branch shows nothing. A reader names the ref —
`git log refs/anvil/change-record`.

### Establishing the lineage: baseline import

```text
anvil-change-record-import --hearth <path>
```

Replay from empty is unattainable for governance state that predates the
mechanism, so the honest starting point is a **baseline**: one commit whose tree
equals the recorded path set at import time, with no parent, on both refs.

- It works on a hearth that is not yet a git repository (initializing one) and
  on a hearth that already is one, **including one with a dirty working tree**,
  which it leaves exactly as it found it.
- `init` is allowed **here and nowhere else**, and never inside a repository
  anvil did not create. A hearth nested inside a foreign repository is a
  **refusal** that names what it found — committing a user's hearth into their
  home-directory dotfiles repo is the same class of error as answering a merge
  claim from the wrong checkout.
- It is **idempotent**: a second run establishes no second baseline and creates
  no duplicate commit. That is enforced by git's own compare-and-swap
  (`update-ref <ref> <new> ""`, create-only), not by a pre-check with a race
  window in it.
- **Replay from the baseline reproduces every recorded path byte-for-byte** —
  a byte comparison, never a parse-and-compare, because a trailing newline and
  a YAML key order are exactly the differences a parse would call equal.

Import is a thin client of `anvil_core::hearth::change_record_baseline`; no
consumer parses its stdout.

### What the engine never touches

The engine **never** moves `HEAD`, **never** stages into the repository's own
index, **never** modifies a file it did not itself write as part of the
transaction, and **never** runs a destructive git operation.

- **Allowed subcommands, and nothing else:** `init` (baseline import only, and
  never inside a foreign repository), `rev-parse --show-toplevel` /
  `rev-parse --verify --quiet <ref>`, `hash-object -w --stdin`, `read-tree`,
  `update-index --add --cacheinfo` / `--force-remove`, `write-tree`,
  `commit-tree`, `update-ref <ref> <new> <old>`, `ls-tree -r --name-only`,
  `cat-file`, `diff-tree --no-commit-id --name-only -r`.
- **Forbidden**, as a declared constant in the argv builder rather than by
  discipline: `add`, `commit`, `stash`, `reset`, `checkout`, `switch`,
  `restore`, `clean`, `branch`, `merge`, `rebase`, `rm`, `mv`, any `-f` /
  `--force`, and every network verb (`push`, `fetch`, `pull`, `clone`,
  `remote`).
- **`GIT_INDEX_FILE` is load-bearing.** Every `read-tree` / `update-index` /
  `write-tree` runs against a private index file under `<repo>/.git/anvil/`.
  The repository's own `.git/index` is never opened, so a human with a staged
  change is untouched.
- **The ref update is the only mutation of shared state, and it is a
  compare-and-swap.** `git update-ref <ref> <new> <old>`, where `<old>` is the
  tip observed when the parent was chosen, or the empty string to assert the
  ref must not yet exist. A nonzero exit is a lost race, never a force update.

### Losing the ref race

The in-process hearth mutex does not span processes, and a leg writing a second
repository (an `owner_home` target) holds no guard at all — so this
compare-and-swap is the whole of the serialization, not a belt-and-braces
measure on top of a lock.

A writer whose `update-ref` exits nonzero **retries against the new tip**:

1. Re-read the tip.
2. **Ask the lineage whether this operation is already on it.** A commit
   carrying this `Anvil-Operation-Id` between the tip and the journalled parent
   is this transaction, already recorded by whoever won the race; the retry
   adopts that commit and writes nothing. This is the same idempotency key the
   recovery path uses, and without it one transaction can be recorded twice.
3. Otherwise rebuild the tree **on the new tip**, re-commit with the unchanged
   operation id, message and declared `at`, and compare-and-swap again.

So the losing writer's commit is a **child** of the winning writer's, both are
reachable from the tip, and nothing either recorded is dropped. The retry is
**bounded** (8 attempts) and then refuses as a counted conflict: an unbounded
retry against a ref somebody is rewriting in a loop is a livelock, and a
refusal is counted where a hang is not.

A reader can tell a compare-and-swap from a forced update without trusting this
paragraph: under a forced update the losing writer's commit names the *stale*
tip as its parent and the winning commit becomes unreachable.

---

## Commit message

```
anvil: <command> <artifact_kind>

Anvil-Operation-Id: <id>
Anvil-Command: <command>
Anvil-Artifact-Kind: <kind>
Anvil-Event-Kinds: <comma-separated routed event variant names>
Anvil-Repository-Label: <basename-only label>
Anvil-Paths-Recorded: <count>
Anvil-At: <rfc3339>
Anvil-Actor-Hash: <16 hex>
Anvil-Conversation-Hash: <16 hex>
Anvil-Project-Label: <basename-only label>
Anvil-Playbook-Run-Id: <id>
```

The subject carries the command and the artifact kind only — no title, no
path, no prose. The body is a blank line followed by the trailer block.

**The trailer key set is exactly this eleven-key allowlist and no other key.**
The last four are **omitted when absent** — never emitted empty, never emitted
raw. With no telemetry salt configured there is no hash to emit, so the key is
absent (`anvil-core/src/domain/telemetry_salt.rs`). `Anvil-Paths-Recorded` is a
count, never a list.

### Identity

Author and committer are a **fixed engine identity**, constant across every
commit and every hearth, supplied through `GIT_AUTHOR_*` / `GIT_COMMITTER_*` on
every `commit-tree`. The ambient `user.name` / `user.email` from repository or
global git config is **never** read: a hearth owner's real name and email must
not become engine-authored record metadata.

| Field | Value |
|---|---|
| name | `anvil` |
| email | `anvil@localhost` |

### What may never appear in commit metadata

A raw actor name, a raw `conversation_id`, an approver name, an absolute
filesystem path, and any user prose (reflection notes, intents, findings text,
artifact titles beyond what an id already carries).

Scope note: the commit's **diff** contains hearth-relative paths and the
artifact content itself. That is the tree, and this mechanism does not change
what the tree contains. The contract governs what the engine **adds**.

---

## The journal, and what recovery may and may not do

The atomic unit is the pair (filesystem effects, commit). A process can die
between them, so every transaction **leg** is journalled before the ref moves.

```text
<guarded-hearth>/.git/anvil/journal/<operation_id>/<leg>/{phase,manifest.yaml,index}
```

Inside `.git/`, so it is outside the worktree and the path declaration below
never sees it. Under the **guarded** hearth even for a leg targeting another
repository: an `owner_home` repository may be written only as somebody else's
second leg and may never itself be the hearth of an RPC, so a recovery keyed on
"before this hearth serves a write" would leave its journal unrolled forever.
Legs are numbered rather than labelled, because two legs of one transaction can
target repositories whose basenames collide.

| Phase | Meaning | What recovery does |
|---|---|---|
| `prepared` | The manifest is written; **no ref has moved**, because the phase flips to `applying` before the ref update. | Discard the journal. |
| `applying` | A tree and a commit object may exist, and the ref update may or may not have landed. | If a commit carrying this `operation_id` is reachable from the ref tip, verify and clear. Otherwise re-derive from disk and commit. |
| `committed` | The ref update landed. Only the durable row and the journal clear remain. | Verify the commit is reachable, then clear. |

**Exactly-once is bought with the operation id, not with the phase file.** The
manifest carries every field the commit message renders from, `at` included, so
a rolled-forward commit is **byte-identical** to the one an uninterrupted run
would have written; and `Anvil-Operation-Id` is read back off the lineage to
decide whether the commit already landed. Clearing the journal is what makes a
second recovery pass harmless in the ordinary case, but the `applying`-with-a-
landed-commit case has no cleared journal to rely on — there, the id is the
only guard.

**Recovery never rewinds disk.** The manifest stores each path's **content
hash** and never its content, so a roll-forward must read the surviving bytes
off disk; it cannot invent them. A path whose live bytes disagree with the
journal's expectation is a **reported conflict**, the journal is left in place,
and nothing on disk is touched — the refuse-rather-than-guess posture of the
backlog journal.

**Out of scope, stated so nobody mistakes crash-atomic for durable:** power
loss. There is no `fsync` in this codebase, so `atomic_write`'s rename may be
ordered before the data reaches disk, and the same is true of the phase file.

### The injected crash point, and why it cannot fire in production

Reaching the recovery branches at all requires an injected crash point, so it
is a requirement of the mechanism rather than a test convenience. It is armed
only when **all three** hold, and each is separately mutation-checked:

1. `cfg!(debug_assertions)` — the kit ships built `--release`, where this is
   false and **neither environment variable is even read**.
2. `ANVIL_TEST_MODE=1`, set explicitly. A stray
   `ANVIL_TEST_CHANGE_RECORD_CRASH_AFTER` does nothing on its own.
3. The hearth resolves **under the platform temporary directory**. A hearth
   anywhere else is an **error**, not a silent un-arming: an armed process
   pointed at a real hearth refuses loudly rather than running on looking
   un-armed and proving nothing.

The token comes from a **closed set** — `journalled`, `tree_written`,
`before_ref_update`, `after_ref_update`, `before_row` — and anything else is
refused, so a typo cannot degrade into "no crash point".

### The rendezvous barrier, on the same three gates

Proving that a lost compare-and-swap retries rather than forces needs a race
that is **certain**, not likely. `ANVIL_TEST_CHANGE_RECORD_CAS_BARRIER=<file>`
names a barrier file and is gated by exactly the three conditions above — same
function, one copy, so the two hooks cannot drift apart.

The **first** writer to reach the ref update claims the hold by creating
`<file>.waiting` with `create_new`, and blocks there until `<file>` appears; it
has already read its tip, so its observed-old is stale by the time it attempts
its update. Every later arrival — the other side of the race, and the held
writer's own retry — finds the marker present and passes straight through. The
one-shot claim is load-bearing: a barrier that held both writers would deadlock
them against each other and turn a race scenario into a timeout. Nothing here
orders the race with a sleep.

---

## The path declaration

Declared **once**, in `anvil-core/src/domain/change_record/paths.rs`, as a pure
fold `classify(hearth_relative) -> PathClass`. The per-transaction writer and
the divergence report both call that one function; a second copy anywhere is a
review failure. Two divergent copies of "which paths count" is a failure this
codebase has already had.

The declaration is **exhaustive over the hearth root**: every path resolves to
exactly one class, and a path matching none of the four declared categories is
a **counted, reported residual** — never silently absent.

| Class | Contents |
|---|---|
| `Recorded` | The governance state. Artifact directories under the registry roots (`tracks/`, `proposals/`, `decisions/`, `initiatives/`, `learnings/`, `milestones/`, `playbooks/`, `backlog_items/`, `playbook_generations/`, `workflow_generations/`) — `status.yaml`, `transitions/*.yaml`, authored documents, `*_reflection/`, `carry-forward.md`, `*.amendments.yaml`. The registry files (`tracks.md`, `proposals.md`, `decisions.md`, `initiatives.md`, `learnings.md`, `milestones.md`, `workflows.md`). `projections/`. `backlog_items/` **including** its `.transactions/` journal. |
| `ExcludedSink` | The durable append-only sinks: `activity-log.jsonl`, `routing-activity.jsonl`, `step-measurement.jsonl`, `transition-measurement.jsonl`, `review-verdict.jsonl`, `playbook-measurement.jsonl`, `delivery-log.jsonl`, `abstentions/`, and `change-record.jsonl` — this mechanism's own sink. Excluded because they are ledgers, not the governance tree, and a per-append commit would produce one commit per route turn whose diff is dominated by ledger tails. The exclusion is **declared and counted** in the divergence report, because an undeclared exclusion is how a divergence distribution lies. |
| `HumanContent` | Human notes and research the engine never writes: `research/`, `coordination/`, `context/`, `tools/`, `experiments/`, `__unattributed__/`, and loose root-level `.md` documents that are not registry files. Not recorded, and the exclusion is declared and counted exactly as the sinks' is. |
| `NeverRecorded` | Secrets and engine config, in a category of their own — never recorded in any stage, **including baseline import**, and counted without ever being read: `.telemetry-salt`, `.hearth`, `hearth.yaml`, `engine-flags.env`, and `.git/` (which also holds this mechanism's own journals). Recording the salt into the same object store as the hashes it protects would defeat the no-raw-identities contract by construction. |
| `Residual` | Everything else, counted and reported. A residual bucket that is empty on today's hearths is not a formality: it is the only thing that will notice the next directory somebody adds to a hearth root. |

---

## `change-record.jsonl`

A durable append-only sink at the hearth root, hand-rendered over a declared
key allowlist so the redaction guarantee is auditable by reading the
`format!`. It is itself `ExcludedSink` — were it recorded, a transaction's
commit would contain the row describing that same commit.

Two row kinds share the file.

### `kind: "change_record"` — one row per transaction **leg**

| Key | Notes |
|---|---|
| `kind` | `"change_record"` |
| `command` | the RPC command |
| `artifact_kind` | |
| `event_kinds` | the routed event variant names |
| `outcome` | `recorded` \| `failed` \| `no_baseline` |
| `commit` | hex object id, empty when there is none |
| `operation_id` | shared across every leg of one transaction |
| `repository_label` | basename-only label of the repository **this leg** wrote — never a path |
| `paths_recorded` | a count |
| `duration_ms` | |
| `at` | |
| correlation keys | `actor_hash`, `conversation_hash`, `project_label`, `playbook_run_id` |

A transaction spanning two repositories writes **two rows sharing one
`operation_id`**, distinguished by `repository_label`.

### `kind: "authority_flip"`

| Key | Notes |
|---|---|
| `kind` | `"authority_flip"` |
| `hearth_label` | basename-only |
| `direction` | `to_authority` \| `to_shadow` |
| `parity_report_id` | the **recomputed** report id, never an id typed by hand |
| `actor_hash` | |
| `at` | |

---

## Rollout flags

Both live in the per-hearth `engine-flags.env`
(`anvil-engine/src/engine_flags.rs`), read per resolved request hearth and
non-mutatively, under that file's existing value grammar — trimmed `1` / `true`
/ `on` is on; absence and everything else is off. Both are `ANVIL_`-prefixed
because the parser honors nothing else, and both are excluded from the global
flag installer so a flip is per-hearth rather than process-global.

| Key | Meaning |
|---|---|
| `ANVIL_CHANGE_RECORD_SHADOW` | Shadow recording on for this hearth. Default off. In shadow the current bookkeeping stays authoritative: a git-write failure never fails the RPC and never alters a byte of what is written to disk — it is counted on the row. |
| `ANVIL_CHANGE_RECORD_AUTHORITY` | Authority for this hearth. The observable difference is the failure posture: a git-write failure fails the RPC, naming the unrecorded change. Reversible; `to_shadow` writes `=off` rather than deleting the line, so a reversal is visible in the file rather than looking like a hearth that was never flipped. |

`engine-flags.env` is itself `NeverRecorded` content.

---

## Amendment log

Each phase of the implementing track amends the section it lands. The first
version carried the ref layout, the never-touch-HEAD allowlist, the commit
message and trailer allowlist, the path declaration, both row allowlists and
the two flags. **P2** made the message shape real (`render_commit_message` and
the trailer allowlist are now one declaration in
`anvil-core/src/domain/change_record/message.rs`, asserted over the rendered
commit). **P3** added the journal section above. The divergence report shape and
the `report_id` formula are added when they land.
