# Revise Mode — Respond to Review Findings or Apply Amendments

Revise mode has two sub-operations. Check which applies:

- **Review response** — `review.md` has unaddressed findings from a reviewer's engine-served review. This is the common case.
- **Amendment application** — `amendments.md` has a reviewed and approved amendment to apply.

If both apply (unlikely but possible), review response takes priority.

The `snapshot` MCP tool transition to `revision` was already fired by the protocol.

---

## Review response

1. Read `definition.md` and `review.md`
2. Append a response timestamped via `date -u +%Y-%m-%dT%H:%M:%SZ` to the review document with explicit dispositions for every finding:
   - **"Will address"** — describe the change
   - **"Acknowledged, not addressing"** — explain why
   No finding may be silently skipped, regardless of severity.
3. Revise `definition.md` as needed
4. Commit: `initiative(forge): revise {name} — address review findings`
5. The human or reviewer advances state back to `review` when ready for re-review.

---

## Amendment application

When an amendment to `amendments.md` has been reviewed and approved:

1. Read the approved amendment
2. Update `definition.md` to reflect the approved change — this is the projection update
3. Commit: `initiative(forge): revise {name} — apply approved amendment`
